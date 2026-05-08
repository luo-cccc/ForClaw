use crate::chapter_generation::{
    ChapterBookStateDeltaBucket, ChapterPromiseDeltaAction, ChapterPromiseDeltaEntry,
    ChapterSettlementApplyResult, ChapterSettlementDelta,
};
use crate::writer_agent::memory::{BookStateSummary, ChapterResultSummary, WriterMemory};

pub fn apply_chapter_settlement_delta(
    memory: &WriterMemory,
    project_id: &str,
    delta: &ChapterSettlementDelta,
) -> Result<ChapterSettlementApplyResult, String> {
    let source_ref = format!(
        "chapter_settlement:{}:{}",
        delta.chapter_title, delta.chapter_revision
    );
    let chapter_result = ChapterResultSummary {
        id: 0,
        project_id: project_id.to_string(),
        chapter_title: delta.chapter_title.clone(),
        chapter_revision: delta.chapter_revision.clone(),
        summary: delta.chapter_result.summary.clone(),
        state_changes: delta.chapter_result.state_changes.clone(),
        character_progress: delta.chapter_result.character_progress.clone(),
        new_conflicts: delta.chapter_result.new_conflicts.clone(),
        new_clues: delta.chapter_result.new_clues.clone(),
        promise_updates: delta.chapter_result.promise_updates.clone(),
        canon_updates: delta.chapter_result.canon_updates.clone(),
        source_ref: source_ref.clone(),
        created_at: crate::agent_runtime::now_ms(),
    };
    let chapter_result_snapshot_id = memory
        .upsert_chapter_result(&chapter_result)
        .map_err(|e| e.to_string())?;

    let mut promise_created = 0usize;
    let mut promise_advanced = 0usize;
    let mut promise_resolved = 0usize;
    let mut promise_deferred = 0usize;
    let mut promise_abandoned = 0usize;
    let mut warnings = Vec::new();

    for update in &delta.promise_updates {
        match update.action {
            ChapterPromiseDeltaAction::Introduced => match memory.find_open_promise_by_identity(
                &update.kind,
                &update.title,
                &update.description,
            ) {
                Ok(Some(existing)) => {
                    memory
                        .touch_promise_last_seen(existing.id, &update.chapter, &update.source_ref)
                        .map_err(|e| e.to_string())?;
                    memory
                        .update_promise_status_flags(
                            existing.id,
                            &update.blocked_reason,
                            existing.promoted || update.promoted,
                            existing.core || update.core,
                        )
                        .map_err(|e| e.to_string())?;
                    promise_advanced += 1;
                }
                Ok(None) => {
                    memory
                        .add_promise_with_status_flags(
                            &update.kind,
                            &update.title,
                            &update.description,
                            &update.chapter,
                            &update.source_ref,
                            &update.expected_payoff,
                            update.priority,
                            &update.related_entities,
                            &update.blocked_reason,
                            update.promoted,
                            update.core,
                        )
                        .map_err(|e| e.to_string())?;
                    promise_created += 1;
                }
                Err(error) => return Err(error.to_string()),
            },
            ChapterPromiseDeltaAction::Advanced => {
                if let Some(promise_id) = resolve_promise_id(memory, update)? {
                    memory
                        .touch_promise_last_seen(promise_id, &update.chapter, &update.source_ref)
                        .map_err(|e| e.to_string())?;
                    memory
                        .update_promise_status_flags(
                            promise_id,
                            &update.blocked_reason,
                            update.promoted,
                            update.core,
                        )
                        .map_err(|e| e.to_string())?;
                    if !update.expected_payoff.trim().is_empty() {
                        memory
                            .defer_promise(promise_id, &update.expected_payoff)
                            .map_err(|e| e.to_string())?;
                    }
                    promise_advanced += 1;
                } else {
                    warnings.push(format!("promise advance skipped: {}", update.title));
                }
            }
            ChapterPromiseDeltaAction::Resolved => {
                if let Some(promise_id) = resolve_promise_id(memory, update)? {
                    if memory
                        .resolve_promise(promise_id, &update.chapter)
                        .map_err(|e| e.to_string())?
                    {
                        promise_resolved += 1;
                    } else {
                        warnings.push(format!("promise already closed: {}", update.title));
                    }
                } else {
                    warnings.push(format!("promise resolve skipped: {}", update.title));
                }
            }
            ChapterPromiseDeltaAction::Deferred => {
                if let Some(promise_id) = resolve_promise_id(memory, update)? {
                    if memory
                        .defer_promise(promise_id, &update.expected_payoff)
                        .map_err(|e| e.to_string())?
                    {
                        memory
                            .touch_promise_last_seen(
                                promise_id,
                                &update.chapter,
                                &update.source_ref,
                            )
                            .map_err(|e| e.to_string())?;
                        memory
                            .update_promise_status_flags(
                                promise_id,
                                &update.blocked_reason,
                                update.promoted,
                                update.core,
                            )
                            .map_err(|e| e.to_string())?;
                        promise_deferred += 1;
                    }
                } else {
                    warnings.push(format!("promise defer skipped: {}", update.title));
                }
            }
            ChapterPromiseDeltaAction::Abandoned => {
                if let Some(promise_id) = resolve_promise_id(memory, update)? {
                    if memory
                        .abandon_promise(promise_id)
                        .map_err(|e| e.to_string())?
                    {
                        promise_abandoned += 1;
                    }
                } else {
                    warnings.push(format!("promise abandon skipped: {}", update.title));
                }
            }
        }
    }

    // Apply character state deltas
    let mut character_state_applied = 0usize;
    for delta in &delta.character_state_deltas {
        if let Ok(Some(character)) = memory.get_character_by_name(&delta.character_name) {
            let _ = memory.close_active_states_for_character(character.id, &delta.chapter_title);
            if let Ok(state_id) = memory.upsert_character_state(
                character.id,
                &delta.chapter_title,
                &serde_json::json!(delta.core_commitments),
                &delta.goal_state,
                &serde_json::json!({}),
                &[],
                &delta.source_ref,
            ) {
                character_state_applied += 1;
                let _ = state_id;
            }
        }
    }

    // Apply relationship deltas
    let mut relationship_applied = 0usize;
    for delta in &delta.relationship_deltas {
        if !delta.character_a_name.is_empty() && !delta.character_b_name.is_empty() {
            if let (Ok(Some(a)), Ok(Some(b))) = (
                memory.get_character_by_name(&delta.character_a_name),
                memory.get_character_by_name(&delta.character_b_name),
            ) {
                match memory.upsert_relationship(
                    a.id,
                    b.id,
                    &delta.relation_type,
                    &delta.visibility,
                    &delta.chapter_title,
                    &delta.source_ref,
                ) {
                    Ok(_) => {
                        relationship_applied += 1;
                    }
                    Err(e) => warnings.push(format!("relationship upsert failed: {}", e)),
                }
            }
        }
    }

    // Apply knowledge deltas
    let mut knowledge_applied = 0usize;
    for delta in &delta.knowledge_deltas {
        if let Ok(knowledge_id) =
            memory.upsert_knowledge_item(&delta.topic, &delta.truth_state, &delta.source_ref)
        {
            match memory.upsert_knowledge_ownership(
                knowledge_id,
                &delta.holder_type,
                delta.holder_id,
                &delta.knowledge_mode,
                &delta.chapter_title,
                &delta.source_ref,
            ) {
                Ok(_) => {
                    if delta.knowledge_mode == "aware" {
                        let _ = memory.record_reveal_event(
                            knowledge_id,
                            "knowledge",
                            "public",
                            &delta.chapter_title,
                            &delta.source_ref,
                        );
                    }
                    knowledge_applied += 1;
                }
                Err(e) => warnings.push(format!("knowledge_ownership upsert failed: {}", e)),
            }
        }
    }

    // Apply identity deltas
    let mut identity_applied = 0usize;
    for delta in &delta.identity_deltas {
        if let Ok(Some(character)) = memory.get_character_by_name(&delta.character_name) {
            // Close any existing active identity layer for this character
            if let Ok(Some(existing)) =
                memory.get_active_identity(character.id, &delta.chapter_title)
            {
                let _ = memory.close_identity_layer(existing.id, &delta.chapter_title);
            }
            match memory.upsert_identity_layer(
                character.id,
                &delta.public_identity,
                &delta.private_identity,
                &delta.revealed_to,
                &delta.chapter_title,
            ) {
                Ok(_) => {
                    identity_applied += 1;
                }
                Err(e) => warnings.push(format!("identity_layer upsert failed: {}", e)),
            }
        }
    }

    // Apply scene deltas
    let mut scene_applied = 0usize;
    for proj in &delta.scene_deltas {
        if proj.scene_id == 0 {
            continue;
        }
        match memory.record_scene_result(
            proj.scene_id,
            &proj.outcome,
            &proj.consequence,
            &proj.source_ref,
        ) {
            Ok(_) => {
                scene_applied += 1;
            }
            Err(e) => warnings.push(format!("scene_result failed: {}", e)),
        }
    }

    // Apply fact deltas with cross-chapter dedup
    let mut fact_applied = 0usize;
    {
        let known_entities = memory.get_canon_entity_names().unwrap_or_default();
        for fact_line in &delta.chapter_fact_delta {
            let mut inserted = false;
            for entity_name in &known_entities {
                if fact_line.contains(entity_name.as_str()) {
                    let fact_key = format!("fact-{}", crate::storage::content_revision(fact_line));
                    if let Ok(_) =
                        memory.update_canon_attribute(entity_name, &fact_key, fact_line, 0.5)
                    {
                        inserted = true;
                    }
                    break; // One entity per fact line
                }
            }
            if inserted {
                fact_applied += 1;
            }
        }
    }

    let existing = memory
        .get_book_state(project_id)
        .map_err(|e| e.to_string())?
        .unwrap_or(BookStateSummary {
            project_id: project_id.to_string(),
            title: project_id.to_string(),
            long_term_constraints: Vec::new(),
            mega_promises: Vec::new(),
            irreversible_changes: Vec::new(),
            source_ref: source_ref.clone(),
            updated_at: String::new(),
        });
    let mut book_state = existing.clone();
    let before = (
        book_state.long_term_constraints.clone(),
        book_state.mega_promises.clone(),
        book_state.irreversible_changes.clone(),
    );
    for update in &delta.book_state_updates {
        let target = match update.bucket {
            ChapterBookStateDeltaBucket::LongTermConstraint => {
                &mut book_state.long_term_constraints
            }
            ChapterBookStateDeltaBucket::MegaPromise => &mut book_state.mega_promises,
            ChapterBookStateDeltaBucket::IrreversibleChange => &mut book_state.irreversible_changes,
        };
        if !target.iter().any(|item| item == &update.value) {
            target.push(update.value.clone());
        }
    }
    book_state.source_ref = source_ref.clone();
    let book_state_updated = before
        != (
            book_state.long_term_constraints.clone(),
            book_state.mega_promises.clone(),
            book_state.irreversible_changes.clone(),
        );
    if book_state_updated {
        memory
            .upsert_book_state(&book_state)
            .map_err(|e| e.to_string())?;
    }

    memory
        .record_decision(
            &delta.chapter_title,
            "Chapter settlement applied",
            "applied_chapter_settlement_delta",
            &[],
            &format!(
                "Applied typed settlement delta for {} at {}.",
                delta.chapter_title, delta.chapter_revision
            ),
            &[source_ref],
        )
        .map_err(|e| e.to_string())?;

    Ok(ChapterSettlementApplyResult {
        applied: true,
        chapter_result_snapshot_id: Some(chapter_result_snapshot_id),
        promise_created,
        promise_advanced,
        promise_resolved,
        promise_deferred,
        promise_abandoned,
        book_state_updated,
        character_state_applied,
        relationship_applied,
        knowledge_applied,
        identity_applied,
        scene_applied,
        fact_applied,
        warnings,
    })
}

fn resolve_promise_id(
    memory: &WriterMemory,
    update: &ChapterPromiseDeltaEntry,
) -> Result<Option<i64>, String> {
    if let Some(id) = update.promise_id {
        return Ok(Some(id));
    }
    Ok(memory
        .find_open_promise_by_identity(&update.kind, &update.title, &update.description)
        .map_err(|e| e.to_string())?
        .map(|promise| promise.id))
}
