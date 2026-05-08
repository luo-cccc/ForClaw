//! Operation execution — typed operation dispatch.
//! Extracted from kernel.rs.

use super::helpers::{
    normalize_chapter_mission_status, validate_chapter_mission_summary,
    validate_story_contract_summary,
};
use super::memory_candidates::{
    style_preference_memory_key, validate_style_preference_with_memory, MemoryCandidateQuality,
};
use crate::writer_agent::memory::{ChapterMissionSummary, StoryContractSummary, WriterMemory};
use crate::writer_agent::operation::{execute_text_operation, OperationResult, WriterOperation};

pub(crate) fn execute_writer_operation(
    memory: &mut WriterMemory,
    active_chapter: &Option<String>,
    operation: WriterOperation,
    current_content: &str,
    current_revision: &str,
) -> Result<OperationResult, String> {
    let result: Result<OperationResult, String> = match &operation {
        WriterOperation::TextInsert { .. } | WriterOperation::TextReplace { .. } => {
            match execute_text_operation(&operation, current_content, current_revision) {
                Ok((_new_content, new_revision)) => Ok(OperationResult {
                    success: true,
                    operation,
                    error: None,
                    revision_after: Some(new_revision),
                }),
                Err(e) => Ok(OperationResult {
                    success: false,
                    operation,
                    error: Some(e),
                    revision_after: None,
                }),
            }
        }
        WriterOperation::TextAnnotate {
            chapter,
            from,
            to,
            message,
            severity,
        } => {
            let source = format!("text:{}:{}-{}", chapter, from, to);
            memory
                .record_decision(
                    chapter,
                    &format!("Annotation: {:?}", severity),
                    "annotated_text",
                    &[],
                    message,
                    &[source],
                )
                .map_err(|e| format!("annotation: {}", e))?;
            Ok(OperationResult {
                success: true,
                operation,
                error: None,
                revision_after: None,
            })
        }
        WriterOperation::CanonUpsertEntity { entity } => {
            if entity.kind == "character" {
                // Upsert into characters table (new authoritative source)
                memory
                    .upsert_character(&entity.name, &entity.aliases, "supporting", &entity.summary)
                    .map_err(|e| format!("character: {}", e))?;
            }
            // Always maintain canon_entities row for backward compatibility
            memory
                .upsert_canon_entity(
                    &entity.kind,
                    &entity.name,
                    &entity.aliases,
                    &entity.summary,
                    &entity.attributes,
                    entity.confidence,
                )
                .map_err(|e| format!("canon: {}", e))?;
            Ok(OperationResult {
                success: true,
                operation,
                error: None,
                revision_after: None,
            })
        }
        WriterOperation::CanonUpdateAttribute {
            entity,
            attribute,
            value,
            confidence,
        } => {
            let rationale = format!(
                "Author confirmed canon update: {}.{} = {}",
                entity, attribute, value
            );
            memory
                .update_canon_attribute(entity, attribute, value, *confidence)
                .map_err(|e| format!("canon: {}", e))?;
            memory
                .record_decision(
                    active_chapter.as_deref().unwrap_or("project"),
                    &format!("Canon update: {}", entity),
                    "updated_canon",
                    &[],
                    &rationale,
                    &[format!("canon:{}:{}", entity, attribute)],
                )
                .ok();
            Ok(OperationResult {
                success: true,
                operation,
                error: None,
                revision_after: None,
            })
        }
        WriterOperation::CanonUpsertRule { rule } => {
            memory
                .upsert_canon_rule(
                    &rule.rule,
                    &rule.category,
                    rule.priority,
                    "writer_operation",
                )
                .map_err(|e| format!("canon rule: {}", e))?;
            memory
                .record_decision(
                    active_chapter.as_deref().unwrap_or("project"),
                    &format!("Canon rule: {}", rule.category),
                    "upserted_canon_rule",
                    &[],
                    &rule.rule,
                    &[format!("canon_rule:{}", rule.category)],
                )
                .ok();
            Ok(OperationResult {
                success: true,
                operation,
                error: None,
                revision_after: None,
            })
        }
        WriterOperation::PromiseAdd { promise } => {
            memory
                .add_promise_with_entities(
                    &promise.kind,
                    &promise.title,
                    &promise.description,
                    &promise.introduced_chapter,
                    &promise.expected_payoff,
                    promise.priority,
                    &promise.related_entities,
                )
                .map_err(|e| format!("promise: {}", e))?;
            Ok(OperationResult {
                success: true,
                operation,
                error: None,
                revision_after: None,
            })
        }
        WriterOperation::PromiseResolve {
            promise_id,
            chapter,
        } => {
            let id = promise_id
                .parse::<i64>()
                .map_err(|_| format!("promise: invalid promise id '{}'", promise_id))?;
            let resolved = memory
                .resolve_promise(id, chapter)
                .map_err(|e| format!("promise: {}", e))?;
            Ok(OperationResult {
                success: resolved,
                operation,
                error: if resolved {
                    None
                } else {
                    Some(super::operation::OperationError::invalid(
                        "Promise is already resolved or does not exist",
                    ))
                },
                revision_after: None,
            })
        }
        WriterOperation::PromiseDefer {
            promise_id,
            chapter,
            expected_payoff,
        } => {
            let id = promise_id
                .parse::<i64>()
                .map_err(|_| format!("promise: invalid promise id '{}'", promise_id))?;
            let deferred = memory
                .defer_promise(id, expected_payoff)
                .map_err(|e| format!("promise: {}", e))?;
            if deferred {
                memory
                    .record_decision(
                        chapter,
                        &format!("Defer promise {}", promise_id),
                        "deferred_promise",
                        &[],
                        &format!(
                            "Author deferred promise {} to {}",
                            promise_id, expected_payoff
                        ),
                        &[format!("promise:{}", promise_id)],
                    )
                    .ok();
            }
            Ok(OperationResult {
                success: deferred,
                operation,
                error: if deferred {
                    None
                } else {
                    Some(super::operation::OperationError::invalid(
                        "Promise is already closed or does not exist",
                    ))
                },
                revision_after: None,
            })
        }
        WriterOperation::PromiseAbandon {
            promise_id,
            chapter,
            reason,
        } => {
            let id = promise_id
                .parse::<i64>()
                .map_err(|_| format!("promise: invalid promise id '{}'", promise_id))?;
            let abandoned = memory
                .abandon_promise(id)
                .map_err(|e| format!("promise: {}", e))?;
            if abandoned {
                memory
                    .record_decision(
                        chapter,
                        &format!("Abandon promise {}", promise_id),
                        "abandoned_promise",
                        &["resolve".to_string(), "defer".to_string()],
                        reason,
                        &[format!("promise:{}", promise_id)],
                    )
                    .ok();
            }
            Ok(OperationResult {
                success: abandoned,
                operation,
                error: if abandoned {
                    None
                } else {
                    Some(super::operation::OperationError::invalid(
                        "Promise is already closed or does not exist",
                    ))
                },
                revision_after: None,
            })
        }
        WriterOperation::CharacterUpsert {
            name,
            aliases,
            role_type,
            summary,
        } => {
            memory
                .upsert_character(name, aliases, role_type, summary)
                .map_err(|e| format!("character: {}", e))?;
            memory
                .record_decision(
                    active_chapter.as_deref().unwrap_or("project"),
                    &format!("Character upsert: {}", name),
                    "upserted_character",
                    &[],
                    summary,
                    &[format!("character:{}", name)],
                )
                .ok();
            Ok(OperationResult {
                success: true,
                operation,
                error: None,
                revision_after: None,
            })
        }
        WriterOperation::CharacterStateUpsert {
            character_id,
            valid_from_chapter,
            core_commitments,
            goal_state,
            identity_state,
            source_ref,
        } => {
            memory
                .upsert_character_state(
                    *character_id,
                    valid_from_chapter,
                    core_commitments,
                    goal_state,
                    identity_state,
                    &[],
                    source_ref,
                )
                .map_err(|e| format!("character_state: {}", e))?;
            memory
                .record_decision(
                    valid_from_chapter,
                    &format!("Character state upsert: character {}", character_id),
                    "upserted_character_state",
                    &[],
                    &format!("State updated from {}", source_ref),
                    &[format!("character_state:{}", character_id)],
                )
                .ok();
            Ok(OperationResult {
                success: true,
                operation,
                error: None,
                revision_after: None,
            })
        }
        WriterOperation::RelationshipUpsert {
            character_a_id,
            character_b_id,
            relation_type,
            visibility,
            valid_from_chapter,
            source_ref,
        } => {
            memory
                .upsert_relationship(
                    *character_a_id,
                    *character_b_id,
                    relation_type,
                    visibility,
                    valid_from_chapter,
                    source_ref,
                )
                .map_err(|e| format!("relationship: {}", e))?;
            memory
                .record_decision(
                    valid_from_chapter,
                    &format!(
                        "Relationship upsert: {} <-> {}",
                        character_a_id, character_b_id
                    ),
                    "upserted_relationship",
                    &[],
                    &format!("{}: {} from {}", relation_type, visibility, source_ref),
                    &[format!(
                        "relationship:{}-{}",
                        character_a_id, character_b_id
                    )],
                )
                .ok();
            Ok(OperationResult {
                success: true,
                operation,
                error: None,
                revision_after: None,
            })
        }
        WriterOperation::PromiseBindSubject {
            promise_id,
            subject_ids,
            subject_type,
        } => {
            memory
                .bind_promise_subject(*promise_id, subject_ids, subject_type)
                .map_err(|e| format!("promise.bind_subject: {}", e))?;
            memory
                .record_decision(
                    active_chapter.as_deref().unwrap_or("project"),
                    &format!("Bind subjects to promise {}", promise_id),
                    "bound_promise_subjects",
                    &[],
                    &format!(
                        "Bound {} {:?} to promise {}",
                        subject_type, subject_ids, promise_id
                    ),
                    &[format!("promise:{}", promise_id)],
                )
                .ok();
            Ok(OperationResult {
                success: true,
                operation,
                error: None,
                revision_after: None,
            })
        }
        WriterOperation::StyleUpdatePreference { key, value } => {
            match validate_style_preference_with_memory(key, value, memory) {
                MemoryCandidateQuality::Acceptable => {}
                MemoryCandidateQuality::Vague { reason } => {
                    return Ok(OperationResult {
                        success: false,
                        operation,
                        error: Some(super::operation::OperationError::invalid(&format!(
                            "Style preference is too vague: {}",
                            reason
                        ))),
                        revision_after: None,
                    });
                }
                MemoryCandidateQuality::Duplicate { existing_name } => {
                    return Ok(OperationResult {
                        success: false,
                        operation,
                        error: Some(super::operation::OperationError::invalid(&format!(
                            "Style preference '{}' already exists",
                            existing_name
                        ))),
                        revision_after: None,
                    });
                }
                MemoryCandidateQuality::MergeableAttributes { .. } => {}
                MemoryCandidateQuality::Conflict {
                    existing_name,
                    reason,
                } => {
                    return Ok(OperationResult {
                        success: false,
                        operation,
                        error: Some(super::operation::OperationError::invalid(&format!(
                            "Style preference '{}' conflicts: {}",
                            existing_name, reason
                        ))),
                        revision_after: None,
                    });
                }
            }
            let memory_key = style_preference_memory_key(key, value);
            memory
                .upsert_style_preference(&memory_key, value, true)
                .map_err(|e| format!("style preference: {}", e))?;
            memory
                .record_decision(
                    active_chapter.as_deref().unwrap_or("project"),
                    &format!("Style preference: {}", memory_key),
                    "updated_style_preference",
                    &[],
                    value,
                    &[memory_key],
                )
                .ok();
            Ok(OperationResult {
                success: true,
                operation,
                error: None,
                revision_after: None,
            })
        }
        WriterOperation::StoryContractUpsert { contract } => {
            let mut summary = StoryContractSummary {
                project_id: contract.project_id.clone(),
                title: contract.title.clone(),
                genre: contract.genre.clone(),
                target_reader: contract.target_reader.clone(),
                reader_promise: contract.reader_promise.clone(),
                first_30_chapter_promise: contract.first_30_chapter_promise.clone(),
                main_conflict: contract.main_conflict.clone(),
                structural_boundary: contract.structural_boundary.clone(),
                tone_contract: contract.tone_contract.clone(),
                updated_at: String::new(),
                quality: String::new(),
                quality_gaps: Vec::new(),
            };
            summary.fill_quality();
            if let Some(error) = validate_story_contract_summary(&summary) {
                Ok(OperationResult {
                    success: false,
                    operation,
                    error: Some(super::operation::OperationError::invalid(&error)),
                    revision_after: None,
                })
            } else {
                memory
                    .upsert_story_contract(&summary)
                    .map_err(|e| format!("story contract: {}", e))?;
                memory
                    .record_decision(
                        "project",
                        "Story contract",
                        "updated_story_contract",
                        &[],
                        &summary.render_for_context(),
                        &[format!("story_contract:{}", summary.project_id)],
                    )
                    .ok();
                Ok(OperationResult {
                    success: true,
                    operation,
                    error: None,
                    revision_after: None,
                })
            }
        }
        WriterOperation::ChapterMissionUpsert { mission } => {
            let normalized_status = normalize_chapter_mission_status(&mission.status);
            let summary = ChapterMissionSummary {
                id: 0,
                project_id: mission.project_id.clone(),
                chapter_title: mission.chapter_title.clone(),
                mission: mission.mission.clone(),
                must_include: mission.must_include.clone(),
                must_not: mission.must_not.clone(),
                expected_ending: mission.expected_ending.clone(),
                status: normalized_status,
                source_ref: mission.source_ref.clone(),
                updated_at: String::new(),
                blocked_reason: mission.blocked_reason.clone(),
                retired_history: mission.retired_history.clone(),
                ..Default::default()
            };
            if let Some(error) = validate_chapter_mission_summary(&summary) {
                Ok(OperationResult {
                    success: false,
                    operation,
                    error: Some(super::operation::OperationError::invalid(&error)),
                    revision_after: None,
                })
            } else {
                memory
                    .upsert_chapter_mission(&summary)
                    .map_err(|e| format!("chapter mission: {}", e))?;
                memory
                    .record_decision(
                        &summary.chapter_title,
                        "Chapter mission",
                        "updated_chapter_mission",
                        &[],
                        &summary.render_for_context(),
                        &[format!(
                            "chapter_mission:{}:{}",
                            summary.project_id, summary.chapter_title
                        )],
                    )
                    .ok();
                Ok(OperationResult {
                    success: true,
                    operation,
                    error: None,
                    revision_after: None,
                })
            }
        }
        WriterOperation::KnowledgeUpsert { topic, truth_state } => {
            memory
                .upsert_knowledge_item(topic, truth_state, "writer_operation")
                .map_err(|e| format!("knowledge.upsert: {}", e))?;
            memory
                .record_decision(
                    active_chapter.as_deref().unwrap_or("project"),
                    &format!("Knowledge upsert: {}", topic),
                    "upserted_knowledge",
                    &[],
                    &format!("Truth state '{}' for topic '{}'", truth_state, topic),
                    &[format!("knowledge:{}", topic)],
                )
                .ok();
            Ok(OperationResult {
                success: true,
                operation,
                error: None,
                revision_after: None,
            })
        }
        WriterOperation::KnowledgeOwnershipUpsert {
            knowledge_id,
            holder_type,
            holder_id,
            knowledge_mode,
            valid_from_chapter,
        } => {
            memory
                .upsert_knowledge_ownership(
                    *knowledge_id,
                    holder_type,
                    *holder_id,
                    knowledge_mode,
                    valid_from_chapter,
                    "writer_operation",
                )
                .map_err(|e| format!("knowledge_ownership.upsert: {}", e))?;
            memory
                .record_decision(
                    valid_from_chapter,
                    &format!(
                        "Knowledge ownership: {} by {}({})",
                        knowledge_id, holder_type, holder_id
                    ),
                    "upserted_knowledge_ownership",
                    &[],
                    &format!(
                        "{} mode for knowledge {} by {}({})",
                        knowledge_mode, knowledge_id, holder_type, holder_id
                    ),
                    &[format!("knowledge_ownership:{}", knowledge_id)],
                )
                .ok();
            Ok(OperationResult {
                success: true,
                operation,
                error: None,
                revision_after: None,
            })
        }
        WriterOperation::IdentityLayerUpsert {
            character_id,
            public_identity,
            private_identity,
            valid_from_chapter,
        } => {
            memory
                .upsert_identity_layer(
                    *character_id,
                    public_identity,
                    private_identity,
                    &[],
                    valid_from_chapter,
                )
                .map_err(|e| format!("identity_layer.upsert: {}", e))?;
            memory
                .record_decision(
                    valid_from_chapter,
                    &format!("Identity layer: character {}", character_id),
                    "upserted_identity_layer",
                    &[],
                    &format!(
                        "Public: {} | Private: {}",
                        public_identity, private_identity
                    ),
                    &[format!("identity_layer:{}", character_id)],
                )
                .ok();
            Ok(OperationResult {
                success: true,
                operation,
                error: None,
                revision_after: None,
            })
        }
        WriterOperation::RevealEventRecord {
            subject_id,
            reveal_type,
            revealed_to,
            chapter,
        } => {
            memory
                .record_reveal_event(
                    *subject_id,
                    reveal_type,
                    revealed_to,
                    chapter,
                    "writer_operation",
                )
                .map_err(|e| format!("reveal_event.record: {}", e))?;
            memory
                .record_decision(
                    chapter,
                    &format!("Reveal: {} to {}", reveal_type, revealed_to),
                    "recorded_reveal_event",
                    &[],
                    &format!("{} revealed {} to {}", subject_id, reveal_type, revealed_to),
                    &[format!("reveal_event:{}", subject_id)],
                )
                .ok();
            Ok(OperationResult {
                success: true,
                operation,
                error: None,
                revision_after: None,
            })
        }
        WriterOperation::SceneUpsert {
            chapter_title,
            sequence,
            scene_type,
            summary,
        } => {
            let scene_id = memory
                .upsert_scene(chapter_title, *sequence, scene_type, summary)
                .map_err(|e| format!("scene.upsert: {}", e))?;
            memory
                .record_decision(
                    chapter_title,
                    &format!("Scene upsert: {} seq {}", chapter_title, sequence),
                    "upserted_scene",
                    &[],
                    summary,
                    &[format!("scene:{}", scene_id)],
                )
                .ok();
            Ok(OperationResult {
                success: true,
                operation,
                error: None,
                revision_after: None,
            })
        }
        WriterOperation::SceneStateUpsert {
            scene_id,
            objective,
            participants,
            location_ref,
            entry_state,
            exit_state,
        } => {
            memory
                .upsert_scene_state(
                    *scene_id,
                    objective,
                    participants,
                    location_ref,
                    entry_state,
                    exit_state,
                )
                .map_err(|e| format!("scene_state.upsert: {}", e))?;
            memory
                .record_decision(
                    active_chapter.as_deref().unwrap_or("project"),
                    &format!("Scene state upsert: {}", scene_id),
                    "upserted_scene_state",
                    &[],
                    objective,
                    &[format!("scene:state:{}", scene_id)],
                )
                .ok();
            Ok(OperationResult {
                success: true,
                operation,
                error: None,
                revision_after: None,
            })
        }
        WriterOperation::SceneObligationUpsert {
            scene_id,
            promise_ids,
            mission_refs,
            payoff_targets,
        } => {
            memory
                .upsert_scene_obligations(*scene_id, promise_ids, mission_refs, payoff_targets)
                .map_err(|e| format!("scene_obligation.upsert: {}", e))?;
            memory
                .record_decision(
                    active_chapter.as_deref().unwrap_or("project"),
                    &format!("Scene obligation upsert: {}", scene_id),
                    "upserted_scene_obligations",
                    &[],
                    &format!("Promises: {:?}, Missions: {:?}", promise_ids, mission_refs),
                    &[format!("scene:obligation:{}", scene_id)],
                )
                .ok();
            Ok(OperationResult {
                success: true,
                operation,
                error: None,
                revision_after: None,
            })
        }
        WriterOperation::SceneResultRecord {
            scene_id,
            outcome,
            consequence,
        } => {
            memory
                .record_scene_result(*scene_id, outcome, consequence, "writer_operation")
                .map_err(|e| format!("scene_result.record: {}", e))?;
            memory
                .record_decision(
                    active_chapter.as_deref().unwrap_or("project"),
                    &format!("Scene result record: {}", scene_id),
                    "recorded_scene_result",
                    &[],
                    outcome,
                    &[format!("scene:result:{}", scene_id)],
                )
                .ok();
            Ok(OperationResult {
                success: true,
                operation,
                error: None,
                revision_after: None,
            })
        }
        WriterOperation::TimeSliceUpsert {
            label,
            relative_order,
        } => {
            memory
                .upsert_time_slice(label, *relative_order, "", "")
                .map_err(|e| format!("time_slice.upsert: {}", e))?;
            memory
                .record_decision(
                    active_chapter.as_deref().unwrap_or("project"),
                    &format!("Time slice upsert: {}", label),
                    "upserted_time_slice",
                    &[],
                    &format!("order: {}", relative_order),
                    &[format!("timeline:{}", label)],
                )
                .ok();
            Ok(OperationResult {
                success: true,
                operation,
                error: None,
                revision_after: None,
            })
        }
        WriterOperation::ChapterTimeMappingUpsert {
            chapter_title,
            scene_id,
            time_slice_id,
            narrative_mode,
        } => {
            memory
                .upsert_chapter_time_mapping(
                    chapter_title,
                    *scene_id,
                    *time_slice_id,
                    narrative_mode,
                )
                .map_err(|e| format!("chapter_time_mapping.upsert: {}", e))?;
            memory
                .record_decision(
                    chapter_title,
                    &format!(
                        "Chapter time mapping: {} -> time slice {}",
                        chapter_title, time_slice_id
                    ),
                    "upserted_chapter_time_mapping",
                    &[],
                    &format!("narrative_mode: {}", narrative_mode),
                    &[format!("timeline:chapter_time_mapping:{}", chapter_title)],
                )
                .ok();
            Ok(OperationResult {
                success: true,
                operation,
                error: None,
                revision_after: None,
            })
        }
        WriterOperation::TimelineEventRecord {
            subject_ids,
            event_type,
            time_slice_id,
        } => {
            memory
                .record_timeline_event(subject_ids, event_type, *time_slice_id, "writer_operation")
                .map_err(|e| format!("timeline_event.record: {}", e))?;
            memory
                .record_decision(
                    active_chapter.as_deref().unwrap_or("project"),
                    &format!(
                        "Timeline event: {} in time slice {}",
                        event_type, time_slice_id
                    ),
                    "recorded_timeline_event",
                    &[],
                    &format!("subjects: {:?}", subject_ids),
                    &[format!("timeline:event:{}:{}", event_type, time_slice_id)],
                )
                .ok();
            Ok(OperationResult {
                success: true,
                operation,
                error: None,
                revision_after: None,
            })
        }
        WriterOperation::OutlineUpdate { .. } => Ok(OperationResult {
            success: false,
            operation,
            error: Some(super::operation::OperationError::invalid(
                "outline.update requires project storage runtime",
            )),
            revision_after: None,
        }),
    };
    result
}
