//! Writer agent Tauri commands — status, ledger, proposals, feedback, approval, observe.

use crate::AppState;
use tauri::Emitter;

#[tauri::command]
pub fn get_agent_tools() -> Result<Vec<crate::agent_runtime::AgentToolDescriptor>, String> {
    Ok(crate::agent_runtime::registered_tools())
}

#[tauri::command]
pub fn get_effective_agent_tool_inventory(
) -> Result<agent_harness_core::EffectiveToolInventory, String> {
    Ok(crate::agent_runtime::effective_tool_inventory())
}

#[tauri::command]
pub fn get_agent_kernel_status() -> Result<crate::AgentKernelStatus, String> {
    let registry = agent_harness_core::default_writing_tool_registry();
    let tools = registry.list();
    let inventory = crate::agent_runtime::effective_tool_inventory();
    let domain = agent_harness_core::writing_domain_profile();

    Ok(crate::AgentKernelStatus {
        tool_generation: registry.generation(),
        tool_count: tools.len(),
        effective_tool_count: inventory.allowed.len(),
        blocked_tool_count: inventory.blocked.len(),
        model_callable_tool_count: inventory.openai_callable_allowed_count(),
        approval_required_tool_count: tools.iter().filter(|tool| tool.requires_approval).count(),
        write_tool_count: tools
            .iter()
            .filter(|tool| tool.side_effect_level == agent_harness_core::ToolSideEffectLevel::Write)
            .count(),
        domain_id: domain.id,
        capability_count: domain.capabilities.len(),
        quality_gate_count: domain.quality_gates.len(),
        trace_enabled: true,
    })
}

#[tauri::command]
pub fn get_agent_domain_profile() -> Result<agent_harness_core::AgentDomainProfile, String> {
    Ok(agent_harness_core::writing_domain_profile())
}

#[tauri::command]
pub fn get_writer_agent_status(
    state: tauri::State<'_, AppState>,
) -> Result<crate::writer_agent::WriterAgentStatus, String> {
    let kernel = state.writer_kernel.lock().map_err(|e| e.to_string())?;
    Ok(kernel.status())
}

#[tauri::command]
pub fn get_writer_agent_ledger(
    state: tauri::State<'_, AppState>,
) -> Result<crate::writer_agent::kernel::WriterAgentLedgerSnapshot, String> {
    let kernel = state.writer_kernel.lock().map_err(|e| e.to_string())?;
    Ok(kernel.ledger_snapshot())
}

#[tauri::command]
pub fn get_writer_agent_pending_proposals(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<crate::writer_agent::proposal::AgentProposal>, String> {
    let kernel = state.writer_kernel.lock().map_err(|e| e.to_string())?;
    Ok(kernel.pending_proposals())
}

#[tauri::command]
pub fn get_story_review_queue(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<crate::writer_agent::kernel::StoryReviewQueueEntry>, String> {
    let kernel = state.writer_kernel.lock().map_err(|e| e.to_string())?;
    Ok(kernel.story_review_queue())
}

#[tauri::command]
pub fn get_story_debt_snapshot(
    state: tauri::State<'_, AppState>,
) -> Result<crate::writer_agent::kernel::StoryDebtSnapshot, String> {
    let kernel = state.writer_kernel.lock().map_err(|e| e.to_string())?;
    Ok(kernel.story_debt_snapshot())
}

#[tauri::command]
pub fn get_writer_agent_trace(
    state: tauri::State<'_, AppState>,
    limit: Option<usize>,
) -> Result<crate::writer_agent::kernel::WriterAgentTraceSnapshot, String> {
    let kernel = state.writer_kernel.lock().map_err(|e| e.to_string())?;
    Ok(kernel.trace_snapshot(limit.unwrap_or(20).min(100)))
}

#[tauri::command]
pub fn get_writer_agent_inspector_timeline(
    state: tauri::State<'_, AppState>,
    limit: Option<usize>,
) -> Result<crate::writer_agent::kernel::WriterInspectorTimeline, String> {
    let kernel = state.writer_kernel.lock().map_err(|e| e.to_string())?;
    Ok(kernel.inspector_timeline(limit.unwrap_or(50).min(200)))
}

#[tauri::command]
pub fn get_writer_agent_companion_timeline_summary(
    state: tauri::State<'_, AppState>,
) -> Result<crate::writer_agent::kernel::WriterInspectorTimeline, String> {
    let kernel = state.writer_kernel.lock().map_err(|e| e.to_string())?;
    Ok(kernel.companion_timeline_summary())
}

#[tauri::command]
pub fn get_writer_agent_today_five(
    state: tauri::State<'_, AppState>,
) -> Result<crate::writer_agent::kernel::TodayFiveSummary, String> {
    let kernel = state.writer_kernel.lock().map_err(|e| e.to_string())?;
    Ok(kernel.today_five_summary())
}

#[tauri::command]
pub fn get_reader_compensation_review_chain(
    state: tauri::State<'_, AppState>,
) -> Result<crate::writer_agent::kernel::ReaderCompensationReviewChain, String> {
    let kernel = state.writer_kernel.lock().map_err(|e| e.to_string())?;
    Ok(kernel.reader_compensation_review_chain())
}

#[tauri::command]
pub fn apply_proposal_feedback(
    state: tauri::State<'_, AppState>,
    feedback: crate::writer_agent::ProposalFeedback,
) -> Result<crate::writer_agent::WriterAgentStatus, String> {
    let mut kernel = state.writer_kernel.lock().map_err(|e| e.to_string())?;
    kernel.apply_feedback(feedback)?;
    Ok(kernel.status())
}

#[tauri::command]
pub fn record_implicit_ghost_rejection(
    state: tauri::State<'_, AppState>,
    proposal_id: String,
    created_at: u64,
) -> Result<bool, String> {
    let mut kernel = state.writer_kernel.lock().map_err(|e| e.to_string())?;
    kernel.record_implicit_ghost_rejection(&proposal_id, created_at)
}

#[tauri::command]
pub fn record_writer_operation_durable_save(
    state: tauri::State<'_, AppState>,
    proposal_id: Option<String>,
    operation: crate::writer_agent::operation::WriterOperation,
    save_result: String,
    saved_content: Option<String>,
    chapter_title: Option<String>,
    chapter_revision: Option<String>,
) -> Result<(), String> {
    let mut kernel = state.writer_kernel.lock().map_err(|e| e.to_string())?;
    let saved_text = saved_content.map(|content| crate::html_to_plain_text(&content));
    kernel.record_operation_durable_save_with_post_write(
        proposal_id,
        operation,
        save_result,
        saved_text,
        chapter_title,
        chapter_revision,
    )
}

#[tauri::command]
pub fn approve_writer_operation(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    operation: crate::writer_agent::operation::WriterOperation,
    current_revision: String,
    approval: Option<crate::writer_agent::operation::OperationApproval>,
) -> Result<crate::writer_agent::operation::OperationResult, String> {
    use crate::writer_agent::operation::WriterOperation;
    if let WriterOperation::OutlineUpdate { node_id, patch } = &operation {
        {
            let mut kernel = state.writer_kernel.lock().map_err(|e| e.to_string())?;
            let preflight = kernel.approve_editor_operation_with_approval(
                operation.clone(),
                &current_revision,
                approval.as_ref(),
            )?;
            if preflight
                .error
                .as_ref()
                .is_none_or(|error| error.code != "invalid")
            {
                return Ok(preflight);
            }
        }
        let result = approve_outline_update_operation(
            &app,
            operation.clone(),
            node_id,
            patch.clone(),
            approval.as_ref(),
        )?;
        let mut kernel = state.writer_kernel.lock().map_err(|e| e.to_string())?;
        if !result.success {
            let save_result = result
                .error
                .as_ref()
                .map(|error| format!("{}:{}", error.code, error.message))
                .unwrap_or_else(|| "outline_storage:failed".to_string());
            kernel.record_operation_durable_save(
                approval
                    .as_ref()
                    .and_then(|context| context.proposal_id.clone()),
                operation,
                save_result,
            )?;
            return Ok(result);
        }
        if let Some(context) = approval
            .as_ref()
            .filter(|context| context.is_valid_for_write())
        {
            kernel.record_operation_durable_save(
                context.proposal_id.clone(),
                operation,
                "outline_storage:ok".to_string(),
            )?;
        }
        return Ok(result);
    }

    let mut kernel = state.writer_kernel.lock().map_err(|e| e.to_string())?;
    kernel.approve_editor_operation_with_approval(operation, &current_revision, approval.as_ref())
}

fn approve_outline_update_operation(
    app: &tauri::AppHandle,
    operation: crate::writer_agent::operation::WriterOperation,
    node_id: &str,
    patch: serde_json::Value,
    approval: Option<&crate::writer_agent::operation::OperationApproval>,
) -> Result<crate::writer_agent::operation::OperationResult, String> {
    if !approval.is_some_and(|context| context.is_valid_for_write()) {
        return Ok(crate::writer_agent::operation::OperationResult {
            success: false,
            operation,
            error: Some(
                crate::writer_agent::operation::OperationError::approval_required(
                    "outline.update requires an explicit surfaced approval context",
                ),
            ),
            revision_after: None,
        });
    }

    match crate::storage::patch_outline_node(app, node_id.to_string(), patch) {
        Ok(_) => Ok(crate::writer_agent::operation::OperationResult {
            success: true,
            operation,
            error: None,
            revision_after: None,
        }),
        Err(e) => Ok(crate::writer_agent::operation::OperationResult {
            success: false,
            operation,
            error: Some(crate::writer_agent::operation::OperationError::invalid(&e)),
            revision_after: None,
        }),
    }
}

#[tauri::command]
pub fn agent_observe(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    observation: crate::agent_runtime::AgentObservation,
) -> Result<crate::agent_runtime::AgentObserveResult, String> {
    let request_id = format!("agent-{}", crate::agent_runtime::now_ms());
    let now = crate::agent_runtime::now_ms();
    let decision = crate::agent_runtime::attention_policy(&observation, now);
    let observation_id = observation.id.clone();

    let mut emitted_proposal_id = None;
    if matches!(observation.mode, crate::agent_runtime::AgentMode::Proactive) {
        let project_id = crate::storage::active_project_id(&app)?;
        let writer_observation = crate::to_writer_observation(&observation, &project_id);
        let writer_observation_for_llm = writer_observation.clone();
        let proposals = {
            let mut kernel = state.writer_kernel.lock().map_err(|e| e.to_string())?;
            crate::refresh_kernel_canon_from_lorebook(&app, &mut kernel);
            kernel.observe(writer_observation)?
        };
        let should_spawn_llm = proposals
            .iter()
            .any(|proposal| proposal.kind == crate::writer_agent::proposal::ProposalKind::Ghost);
        let context_pack_for_llm = if should_spawn_llm && crate::resolve_api_key().is_some() {
            let kernel = state.writer_kernel.lock().map_err(|e| e.to_string())?;
            Some(kernel.ghost_context_pack(&writer_observation_for_llm))
        } else {
            None
        };

        for proposal in proposals {
            emitted_proposal_id.get_or_insert_with(|| proposal.id.clone());
            app.emit(crate::events::AGENT_PROPOSAL, proposal)
                .map_err(|e| format!("Failed to emit agent proposal: {}", e))?;
        }

        if let Some(context_pack) = context_pack_for_llm {
            crate::spawn_llm_ghost_proposal(
                app.clone(),
                writer_observation_for_llm,
                context_pack,
                None,
            );
        }
    }

    if emitted_proposal_id.is_some() {
        return Ok(crate::agent_runtime::AgentObserveResult {
            request_id,
            observation_id,
            decision: "writer_proposal".to_string(),
            reason: decision.reason,
            suggestion_id: emitted_proposal_id,
        });
    }

    if !decision.should_suggest {
        return Ok(crate::agent_runtime::AgentObserveResult {
            request_id,
            observation_id,
            decision: "noop".to_string(),
            reason: decision.reason,
            suggestion_id: None,
        });
    }

    let outline_summary = observation
        .chapter_title
        .as_ref()
        .and_then(|chapter_title| match crate::storage::load_outline(&app) {
            Ok(nodes) => nodes
                .into_iter()
                .find(|node| &node.chapter_title == chapter_title)
                .map(|node| node.summary)
                .filter(|summary| !summary.trim().is_empty()),
            Err(e) => {
                tracing::warn!("Agent observe skipped outline summary: {}", e);
                None
            }
        });

    let paragraph_lower = observation.current_paragraph.to_lowercase();
    let nearby_lower = observation.nearby_text.to_lowercase();
    let lore_entries = match crate::storage::load_lorebook(&app) {
        Ok(entries) => entries,
        Err(e) => {
            tracing::warn!("Agent observe skipped lore hits: {}", e);
            Vec::new()
        }
    };
    let lore_hits = lore_entries
        .into_iter()
        .filter(|entry| {
            let keyword = entry.keyword.to_lowercase();
            !keyword.is_empty()
                && (paragraph_lower.contains(&keyword) || nearby_lower.contains(&keyword))
        })
        .map(|entry| (entry.keyword, entry.content))
        .collect::<Vec<_>>();

    let profile_count = crate::collect_user_profile_entries(&app)
        .map(|entries| entries.len())
        .unwrap_or(0);
    let source_summaries = crate::agent_runtime::build_source_summaries(
        &observation,
        outline_summary,
        lore_hits,
        profile_count,
    );
    let suggestion = crate::agent_runtime::build_suggestion(
        &observation,
        request_id.clone(),
        &decision,
        source_summaries,
    );
    let suggestion_id = suggestion.id.clone();
    app.emit(crate::events::AGENT_SUGGESTION, suggestion)
        .map_err(|e| format!("Failed to emit agent suggestion: {}", e))?;

    Ok(crate::agent_runtime::AgentObserveResult {
        request_id,
        observation_id,
        decision: "suggestion".to_string(),
        reason: decision.reason,
        suggestion_id: Some(suggestion_id),
    })
}

#[derive(serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AmbientEntityHint {
    keyword: String,
    content: String,
    chapter: String,
}

#[tauri::command]
pub fn get_ambient_entity_hints(
    state: tauri::State<'_, AppState>,
    paragraph: String,
    chapter: String,
) -> Result<Vec<AmbientEntityHint>, String> {
    let kernel = state.writer_kernel.lock().map_err(|e| e.to_string())?;
    let names = kernel
        .ledger_snapshot()
        .canon_entities
        .iter()
        .map(|e| e.name.clone())
        .collect::<Vec<_>>();

    let mut hints = Vec::new();
    for name in names {
        if name.len() < 2 || !paragraph.contains(&name) {
            continue;
        }
        if hints.iter().any(|h: &AmbientEntityHint| h.keyword == name) {
            continue;
        }
        let facts = kernel
            .memory
            .get_canon_facts_for_entity(&name)
            .unwrap_or_default();
        let summary = if facts.is_empty() {
            "Canon entity".to_string()
        } else {
            facts
                .iter()
                .take(3)
                .map(|(k, v)| format!("{}: {}", k, v))
                .collect::<Vec<_>>()
                .join(" · ")
        };
        hints.push(AmbientEntityHint {
            keyword: name.clone(),
            content: summary,
            chapter: chapter.clone(),
        });
    }
    Ok(hints)
}
