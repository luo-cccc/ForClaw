use agent_writer_lib::headless::{
    AnalyzeChapterRequest, AskAgentRequest, AskProjectBrainRequest, BatchGenerateChapterRequest,
    HeadlessBackend, MetacognitiveRecoveryRequest, ParallelDraftRequest,
};
use serde_json::{json, Value};

use crate::{server_manifest, tool_error_result, tool_result, ErrorKind, ToolCallParams};

pub(crate) fn classify_error(_tool_name: &str, error: &str) -> ErrorKind {
    let lower = error.to_ascii_lowercase();
    if lower.contains("invalid") && (lower.contains("request") || lower.contains("tool call")) {
        return ErrorKind::Validation;
    }
    if lower.contains("required")
        || lower.contains("must not be empty")
        || lower.contains("missing")
    {
        return ErrorKind::Validation;
    }
    if lower.contains("llm call failed")
        || lower.contains("http request failed")
        || lower.contains("stream read error")
        || lower.contains("rate limit")
        || lower.contains("429")
    {
        return ErrorKind::Provider;
    }
    if lower.contains("approval")
        || lower.contains("permission")
        || lower.contains("denied")
        || lower.contains("read-only")
    {
        return ErrorKind::Permission;
    }
    if lower.contains("budget") || lower.contains("exceeded") || lower.contains("quota") {
        return ErrorKind::Budget;
    }
    if lower.contains("context overflow")
        || lower.contains("token limit")
        || lower.contains("max tokens")
        || lower.contains("context length")
    {
        return ErrorKind::ContextOverflow;
    }
    if lower.contains("storage")
        || lower.contains("persist failed")
        || lower.contains("save failed")
        || lower.contains("sqlite")
        || lower.contains("disk")
    {
        return ErrorKind::Storage;
    }
    ErrorKind::Backend
}

pub(crate) async fn call_tool(backend: &HeadlessBackend, params: Value) -> Result<Value, String> {
    let call: ToolCallParams = match serde_json::from_value(params) {
        Ok(c) => c,
        Err(error) => {
            return Ok(tool_error_result(
                ErrorKind::Validation,
                format!("Invalid tool call: {}", error),
            ));
        }
    };
    let arguments = if call.arguments.is_null() {
        json!({})
    } else {
        call.arguments
    };

    let result = match call.name.as_str() {
        "forge_backend_call" => {
            let action = arguments
                .get("action")
                .and_then(Value::as_str)
                .ok_or_else(|| "action is required".to_string())?;
            let params = arguments
                .get("params")
                .cloned()
                .unwrap_or_else(|| json!({}));
            call_backend_action(backend, action, params).await
        }
        "forge_manifest" => Ok(server_manifest()),
        "forge_project_manifest" => serde_json::to_value(backend.project())
            .map_err(|error| format!("Failed to serialize project manifest: {}", error)),
        "forge_project_paths" => backend.dispatch("project_paths", json!({})),
        "forge_agent_tools" => backend.dispatch("agent_tools", json!({})),
        "forge_effective_tool_inventory" => {
            backend.dispatch("effective_agent_tool_inventory", json!({}))
        }
        "forge_agent_kernel_status" => backend.dispatch("agent_kernel_status", json!({})),
        "forge_agent_domain_profile" => backend.dispatch("agent_domain_profile", json!({})),
        "forge_status" => backend.dispatch("status", json!({})),
        "forge_list_chapters" => backend.dispatch("list_chapters", json!({})),
        "forge_create_chapter" => backend.dispatch("create_chapter", arguments),
        "forge_load_chapter" => backend.dispatch("load_chapter", arguments),
        "forge_save_chapter" => backend.dispatch("save_chapter", arguments),
        "forge_chapter_revision" => backend.dispatch("chapter_revision", arguments),
        "forge_rename_chapter_file" => backend.dispatch("rename_chapter_file", arguments),
        "forge_lorebook" => backend.dispatch("load_lorebook", json!({})),
        "forge_save_lore_entry" => backend.dispatch("save_lore_entry", arguments),
        "forge_delete_lore_entry" => backend.dispatch("delete_lore_entry", arguments),
        "forge_outline" => backend.dispatch("load_outline", json!({})),
        "forge_save_outline_node" => backend.dispatch("save_outline_node", arguments),
        "forge_delete_outline_node" => backend.dispatch("delete_outline_node", arguments),
        "forge_update_outline_status" => backend.dispatch("update_outline_status", arguments),
        "forge_reorder_outline_nodes" => backend.dispatch("reorder_outline_nodes", arguments),
        "forge_list_volumes" => backend.dispatch("list_volumes", json!({})),
        "forge_save_volume" => backend.dispatch("save_volume", arguments),
        "forge_delete_volume" => backend.dispatch("delete_volume", arguments),
        "forge_get_volume_snapshot" => backend.dispatch("get_volume_snapshot", arguments),
        "forge_save_volume_snapshot" => backend.dispatch("save_volume_snapshot", arguments),
        "forge_get_book_state" => backend.dispatch("get_book_state", json!({})),
        "forge_save_book_state" => backend.dispatch("save_book_state", arguments),
        "forge_observe" => {
            let observation = arguments.get("observation").cloned().unwrap_or(arguments);
            backend.dispatch("observe", observation)
        }
        "forge_ledger" => backend.dispatch("ledger", json!({})),
        "forge_today_five" => backend.dispatch("today_five", json!({})),
        "forge_pending_proposals" => backend.dispatch("pending_proposals", json!({})),
        "forge_story_review_queue" => backend.dispatch("story_review_queue", json!({})),
        "forge_story_debt" => backend.dispatch("story_debt", json!({})),
        "forge_reader_compensation_review_chain" => {
            backend.dispatch("reader_compensation_review_chain", json!({}))
        }
        "forge_trace" => backend.dispatch("trace", arguments),
        "forge_inspector_timeline" => backend.dispatch("inspector_timeline", arguments),
        "forge_companion_timeline_summary" => {
            backend.dispatch("companion_timeline_summary", json!({}))
        }
        "forge_apply_feedback" => backend.dispatch("apply_feedback", arguments),
        "forge_record_implicit_ghost_rejection" => {
            backend.dispatch("record_implicit_ghost_rejection", arguments)
        }
        "forge_approve_writer_operation" => backend.dispatch("approve_writer_operation", arguments),
        "forge_record_writer_operation_durable_save" => {
            backend.dispatch("record_writer_operation_durable_save", arguments)
        }
        "forge_ambient_entity_hints" => backend.dispatch("ambient_entity_hints", arguments),
        "forge_ask_agent" => {
            let request: AskAgentRequest = serde_json::from_value(arguments)
                .map_err(|error| format!("Invalid ask-agent request: {}", error))?;
            serde_json::to_value(backend.ask_agent(request).await?)
                .map_err(|error| error.to_string())
        }
        "forge_batch_generate_chapter" => {
            let request: BatchGenerateChapterRequest = serde_json::from_value(arguments)
                .map_err(|error| format!("Invalid batch generation request: {}", error))?;
            serde_json::to_value(backend.batch_generate_chapter(request).await?)
                .map_err(|error| error.to_string())
        }
        "forge_generate_chapter_autonomous" => {
            let request = serde_json::from_value(arguments)
                .map_err(|error| format!("Invalid chapter generation request: {}", error))?;
            serde_json::to_value(backend.generate_chapter_autonomous(request).await?)
                .map_err(|error| error.to_string())
        }
        "forge_repair_chapter_state" => {
            let chapter_title = arguments
                .get("chapterTitle")
                .or_else(|| arguments.get("chapter_title"))
                .and_then(Value::as_str)
                .ok_or_else(|| "chapterTitle is required".to_string())?
                .to_string();
            serde_json::to_value(backend.repair_chapter_state(chapter_title)?)
                .map_err(|error| error.to_string())
        }
        "forge_analyze_chapter" => {
            let request: AnalyzeChapterRequest = serde_json::from_value(arguments)
                .map_err(|error| format!("Invalid analyze-chapter request: {}", error))?;
            serde_json::to_value(backend.analyze_chapter(request).await?)
                .map_err(|error| error.to_string())
        }
        "forge_generate_parallel_drafts" => {
            let request: ParallelDraftRequest = serde_json::from_value(arguments)
                .map_err(|error| format!("Invalid parallel-draft request: {}", error))?;
            serde_json::to_value(backend.generate_parallel_drafts(request).await?)
                .map_err(|error| error.to_string())
        }
        "forge_analyze_pacing" => backend
            .analyze_pacing(
                arguments
                    .get("summaries")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
            )
            .await
            .and_then(|value| serde_json::to_value(value).map_err(|error| error.to_string())),
        "forge_ask_project_brain" => {
            let request: AskProjectBrainRequest = serde_json::from_value(arguments)
                .map_err(|error| format!("Invalid project-brain request: {}", error))?;
            serde_json::to_value(backend.ask_project_brain(request).await?)
                .map_err(|error| error.to_string())
        }
        "forge_project_brain_knowledge_graph" => {
            backend.dispatch("get_project_brain_knowledge_graph", json!({}))
        }
        "forge_compare_project_brain_source_revisions" => {
            backend.dispatch("compare_project_brain_source_revisions", arguments)
        }
        "forge_restore_project_brain_source_revision" => {
            backend.dispatch("restore_project_brain_source_revision", arguments)
        }
        "forge_cross_reference_brain_nodes" => {
            backend.dispatch("cross_reference_brain_nodes", arguments)
        }
        "forge_ingest_external_research" => backend.dispatch("ingest_external_research", arguments),
        "forge_run_metacognitive_recovery" => {
            let request: MetacognitiveRecoveryRequest = serde_json::from_value(arguments)
                .map_err(|error| format!("Invalid metacognitive recovery request: {}", error))?;
            serde_json::to_value(backend.run_metacognitive_recovery(request).await?)
                .map_err(|error| error.to_string())
        }
        "forge_craft_library" => backend.dispatch("craft_library", json!({})),
        "forge_craft_memory_stats" => backend.dispatch("craft_memory_stats", json!({})),
        "forge_record_manual_craft_edit_feedback" => {
            backend.dispatch("record_manual_craft_edit_feedback", arguments)
        }
        "forge_chapter_quality_report" => backend.dispatch("chapter_quality_report", arguments),
        "forge_context_quality_report" => backend.dispatch("context_quality_report", arguments),
        "forge_budget_calibration" => backend.dispatch("budget_calibration", json!({})),
        "forge_execution_plan" => backend.dispatch("execution_plan", json!({})),
        "forge_start_sprint" => backend.dispatch("start_sprint", arguments),
        "forge_sprint_plan" => backend.dispatch("sprint_plan", json!({})),
        "forge_sprint_progress" => backend.dispatch("sprint_progress", json!({})),
        "forge_pause_sprint" => backend.dispatch("pause_sprint", json!({})),
        "forge_resume_sprint" => backend.dispatch("resume_sprint", json!({})),
        "forge_cancel_sprint" => backend.dispatch("cancel_sprint", json!({})),
        "forge_checkpoint_sprint" => backend.dispatch("checkpoint_sprint", arguments),
        "forge_record_sprint_budget_usage" => {
            backend.dispatch("record_sprint_budget_usage", arguments)
        }
        "forge_set_sprint_quality_gate" => backend.dispatch("set_sprint_quality_gate", arguments),
        "forge_project_graph_data" => backend.dispatch("project_graph_data", json!({})),
        "forge_project_storage_diagnostics" => {
            backend.dispatch("project_storage_diagnostics", json!({}))
        }
        "forge_export_writer_agent_trajectory" => {
            backend.dispatch("export_writer_agent_trajectory", arguments)
        }
        "forge_export_diagnostic_logs" => backend.dispatch("export_diagnostic_logs", json!({})),
        "forge_list_file_backups" => backend.dispatch("list_file_backups", arguments),
        "forge_restore_file_backup" => backend.dispatch("restore_file_backup", arguments),
        "forge_set_api_key" => backend.dispatch("set_api_key", arguments),
        "forge_check_api_key" => backend.dispatch("check_api_key", arguments),
        other => {
            return Ok(tool_error_result(
                ErrorKind::Validation,
                format!("Unknown tool: {}", other),
            ));
        }
    };

    match result {
        Ok(value) => Ok(tool_result(value, false)),
        Err(error) => {
            let kind = classify_error(&call.name, &error);
            Ok(tool_error_result(kind, error))
        }
    }
}

pub(crate) async fn call_backend_action(
    backend: &HeadlessBackend,
    action: &str,
    params: Value,
) -> Result<Value, String> {
    match action {
        "manifest" => Ok(server_manifest()),
        "project_manifest" => serde_json::to_value(backend.project())
            .map_err(|error| format!("Failed to serialize project manifest: {}", error)),
        "ask_agent" => {
            let request: AskAgentRequest = serde_json::from_value(params)
                .map_err(|error| format!("Invalid ask-agent request: {}", error))?;
            serde_json::to_value(backend.ask_agent(request).await?)
                .map_err(|error| error.to_string())
        }
        "batch_generate_chapter" => {
            let request: BatchGenerateChapterRequest = serde_json::from_value(params)
                .map_err(|error| format!("Invalid batch generation request: {}", error))?;
            serde_json::to_value(backend.batch_generate_chapter(request).await?)
                .map_err(|error| error.to_string())
        }
        "generate_chapter_autonomous" => {
            let request = serde_json::from_value(params)
                .map_err(|error| format!("Invalid chapter generation request: {}", error))?;
            serde_json::to_value(backend.generate_chapter_autonomous(request).await?)
                .map_err(|error| error.to_string())
        }
        "analyze_chapter" => {
            let request: AnalyzeChapterRequest = serde_json::from_value(params)
                .map_err(|error| format!("Invalid analyze-chapter request: {}", error))?;
            serde_json::to_value(backend.analyze_chapter(request).await?)
                .map_err(|error| error.to_string())
        }
        "repair_chapter_state" => {
            let chapter_title = params
                .get("chapterTitle")
                .or_else(|| params.get("chapter_title"))
                .and_then(Value::as_str)
                .ok_or_else(|| "chapterTitle is required".to_string())?
                .to_string();
            serde_json::to_value(backend.repair_chapter_state(chapter_title)?)
                .map_err(|error| error.to_string())
        }
        "generate_parallel_drafts" => {
            let request: ParallelDraftRequest = serde_json::from_value(params)
                .map_err(|error| format!("Invalid parallel-draft request: {}", error))?;
            serde_json::to_value(backend.generate_parallel_drafts(request).await?)
                .map_err(|error| error.to_string())
        }
        "analyze_pacing" => {
            let summaries = params
                .get("summaries")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            serde_json::to_value(backend.analyze_pacing(summaries).await?)
                .map_err(|error| error.to_string())
        }
        "ask_project_brain" => {
            let request: AskProjectBrainRequest = serde_json::from_value(params)
                .map_err(|error| format!("Invalid project-brain request: {}", error))?;
            serde_json::to_value(backend.ask_project_brain(request).await?)
                .map_err(|error| error.to_string())
        }
        "run_metacognitive_recovery" => {
            let request: MetacognitiveRecoveryRequest = serde_json::from_value(params)
                .map_err(|error| format!("Invalid metacognitive recovery request: {}", error))?;
            serde_json::to_value(backend.run_metacognitive_recovery(request).await?)
                .map_err(|error| error.to_string())
        }
        _ => backend.dispatch(action, params),
    }
}
