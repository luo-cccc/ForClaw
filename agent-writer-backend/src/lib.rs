mod agent_runtime;
mod agent_status;
mod ambient_agents;
#[cfg(test)]
mod api_integration_tests;
mod api_key;
mod app_paths;
mod app_state;
pub mod brain_service;
pub mod chapter_generation;
mod commands;
mod editor_realtime;
mod event_payloads;
mod events;
pub mod headless;
pub mod llm_runtime;
mod manual_agent;
mod memory_context;
mod observation_bridge;
mod project_audit;
mod semantic_lint;
mod storage;
#[cfg(test)]
mod tests;
mod tool_bridge;
pub mod writer_agent;
mod writer_observer;
pub use agent_status::AgentKernelStatus;
pub(crate) use api_key::{require_api_key, resolve_api_key};
pub(crate) use app_paths::{log_dir, safe_filename_component};
pub(crate) use app_state::{
    lock_editor_prediction, lock_harness_state, lock_hermes, startup_error, AppState,
    EditorPredictionTask, HarnessState,
};
pub(crate) use editor_realtime::{
    abort_editor_prediction_task, emit_ambient_output, emit_editor_ghost_end,
    emit_writer_ghost_proposal, realtime_cowrite_enabled, spawn_llm_ghost_proposal,
    EditorGhostRenderTarget,
};
pub(crate) use event_payloads::{InlineWriterOperationEvent, StreamChunk, StreamEnd};
pub(crate) use memory_context::{
    auto_embed_chapter, build_context_injection, collect_user_profile_entries,
    extract_skills_from_recent,
};
pub(crate) use observation_bridge::{
    build_manual_writer_observation, build_writer_observation_from_editor_state,
    to_writer_observation, AskAgentContext, AskAgentMode, EditorStatePayload,
};
#[cfg(test)]
pub(crate) use observation_bridge::{split_context_for_cursor, test_editor_state_payload};
pub(crate) use project_audit::{audit_project_file_write, backup_target_label};
pub(crate) use semantic_lint::{
    find_semantic_lint, semantic_lint_enabled, EditorSemanticLint, SemanticLintPayload,
};
pub(crate) use writer_observer::{
    char_tail, html_to_plain_text, observe_chapter_save, observe_generated_chapter_result,
    refresh_kernel_canon_from_lorebook, render_writer_context_pack,
};

use tauri::Manager;

use commands::backups::{get_project_storage_diagnostics, list_file_backups, restore_file_backup};
use commands::chapters::{
    create_chapter, get_chapter_revision, load_chapter, read_project_dir, rename_chapter_file,
    save_chapter,
};
use commands::diagnostics::{export_diagnostic_logs, export_writer_agent_trajectory};
use commands::editor::{abort_editor_prediction, report_editor_state, report_semantic_lint_state};
use commands::generation::{
    analyze_chapter, analyze_pacing, ask_project_brain, batch_generate_chapter,
    cancel_supervised_sprint, checkpoint_supervised_sprint, generate_chapter_autonomous,
    generate_parallel_drafts, get_supervised_sprint_plan, get_supervised_sprint_progress,
    pause_supervised_sprint, record_supervised_sprint_budget_usage, repair_chapter_state,
    resume_supervised_sprint, start_supervised_sprint,
};
use commands::graph::{
    compare_project_brain_source_revisions, cross_reference_brain_nodes,
    get_project_brain_knowledge_graph, get_project_graph_data, ingest_external_research,
    restore_project_brain_source_revision,
};
use commands::lore::{delete_lore_entry, get_lorebook, save_lore_entry};
use commands::manual_agent::ask_agent;
use commands::metacognitive_recovery::run_metacognitive_recovery;
use commands::outline::{
    delete_outline_node, delete_volume, get_book_state, get_outline, get_volume_snapshot,
    list_volumes, reorder_outline_nodes, save_book_state, save_outline_node, save_volume,
    save_volume_snapshot, update_outline_status,
};
use commands::settings::{check_api_key, set_api_key};
use commands::writer_agent::{
    agent_observe, apply_proposal_feedback, approve_writer_operation, get_agent_domain_profile,
    get_agent_kernel_status, get_agent_tools, get_ambient_entity_hints,
    get_effective_agent_tool_inventory, get_reader_compensation_review_chain,
    get_story_debt_snapshot, get_story_review_queue, get_writer_agent_companion_timeline_summary,
    get_writer_agent_inspector_timeline, get_writer_agent_ledger,
    get_writer_agent_pending_proposals, get_writer_agent_status, get_writer_agent_today_five,
    get_writer_agent_trace, record_implicit_ghost_rejection, record_writer_operation_durable_save,
};
pub(crate) use manual_agent::ManualAgentTurn;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    dotenvy::dotenv().ok();

    // Shared context cache for ambient agents
    let cache: std::sync::Arc<tokio::sync::Mutex<ambient_agents::context_fetcher::ContextCache>> =
        std::sync::Arc::new(tokio::sync::Mutex::new(
            ambient_agents::context_fetcher::ContextCache::default(),
        ));

    // Capture clones before the setup closure
    let cache1 = cache.clone();
    let cache_for_state = cache.clone();

    if let Err(e) = tauri::Builder::default()
        .setup(move |app| {
            let app_state = AppState::open(app.handle()).map_err(startup_error)?;

            let mut event_bus = agent_harness_core::AmbientEventBus::new(256);
            let app_handle = app.handle().clone();
            let output_app = app_handle.clone();
            event_bus.set_output_handler(std::sync::Arc::new(move |output| {
                emit_ambient_output(&output_app, output);
            }));

            app.manage(std::sync::Mutex::new(event_bus));
            app.manage(app_state);
            app.manage(cache_for_state);

            let agent_app = app_handle.clone();
            tauri::async_runtime::spawn(async move {
                if let Some(bus_state) =
                    agent_app.try_state::<std::sync::Mutex<agent_harness_core::AmbientEventBus>>()
                {
                    match bus_state.lock() {
                        Ok(mut event_bus) => {
                            event_bus.spawn_agent(std::sync::Arc::new(
                                ambient_agents::context_fetcher::ContextFetcherAgent {
                                    app: agent_app.clone(),
                                    cache: cache1,
                                },
                            ));
                        }
                        Err(error) => {
                            tracing::error!(
                                "Ambient event bus lock poisoned during startup: {error}"
                            );
                        }
                    }
                } else {
                    tracing::error!("Ambient event bus state was unavailable during startup");
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            abort_editor_prediction,
            report_editor_state,
            report_semantic_lint_state,
            ask_agent,
            run_metacognitive_recovery,
            agent_observe,
            get_agent_domain_profile,
            get_agent_kernel_status,
            get_project_storage_diagnostics,
            list_file_backups,
            restore_file_backup,
            get_writer_agent_status,
            get_ambient_entity_hints,
            get_writer_agent_ledger,
            get_writer_agent_pending_proposals,
            get_story_review_queue,
            get_story_debt_snapshot,
            get_writer_agent_trace,
            get_writer_agent_inspector_timeline,
            get_reader_compensation_review_chain,
            get_writer_agent_companion_timeline_summary,
            get_writer_agent_today_five,
            apply_proposal_feedback,
            record_implicit_ghost_rejection,
            approve_writer_operation,
            record_writer_operation_durable_save,
            get_agent_tools,
            get_effective_agent_tool_inventory,
            get_lorebook,
            save_lore_entry,
            delete_lore_entry,
            read_project_dir,
            create_chapter,
            save_chapter,
            load_chapter,
            get_chapter_revision,
            get_outline,
            save_outline_node,
            delete_outline_node,
            update_outline_status,
            reorder_outline_nodes,
            list_volumes,
            save_volume,
            delete_volume,
            get_volume_snapshot,
            save_volume_snapshot,
            get_book_state,
            save_book_state,
            batch_generate_chapter,
            generate_chapter_autonomous,
            start_supervised_sprint,
            get_supervised_sprint_plan,
            get_supervised_sprint_progress,
            pause_supervised_sprint,
            resume_supervised_sprint,
            cancel_supervised_sprint,
            checkpoint_supervised_sprint,
            record_supervised_sprint_budget_usage,
            repair_chapter_state,
            analyze_chapter,
            ask_project_brain,
            generate_parallel_drafts,
            get_project_brain_knowledge_graph,
            compare_project_brain_source_revisions,
            restore_project_brain_source_revision,
            cross_reference_brain_nodes,
            ingest_external_research,
            get_project_graph_data,
            analyze_pacing,
            rename_chapter_file,
            set_api_key,
            check_api_key,
            export_diagnostic_logs,
            export_writer_agent_trajectory
        ])
        .run(tauri::generate_context!())
    {
        let message = format!("Error while running Tauri application: {}", e);
        tracing::error!("{}", message);
        let _ = msgbox::create("Agent-Writer Error", &message, msgbox::IconType::Error);
    }
}
