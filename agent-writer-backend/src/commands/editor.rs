//! Editor realtime co-writing and lint commands.

use std::sync::Mutex;

use tauri::{Emitter, Manager};
use tokio_util::sync::CancellationToken;

use crate::{
    events, storage, writer_agent, AppState, EditorGhostRenderTarget, EditorPredictionTask,
    EditorStatePayload, SemanticLintPayload,
};

#[tauri::command]
pub fn abort_editor_prediction(
    app: tauri::AppHandle,
    request_id: Option<String>,
) -> Result<bool, String> {
    let state = app.state::<AppState>();
    let aborted = crate::abort_editor_prediction_task(&state, request_id.as_deref())?;
    if aborted {
        if let Some(bus_state) = app.try_state::<Mutex<agent_harness_core::AmbientEventBus>>() {
            if let Ok(mut bus) = bus_state.lock() {
                bus.abort_agent("co-writer");
            }
        }
    }
    Ok(aborted)
}

#[tauri::command]
pub async fn report_editor_state(
    app: tauri::AppHandle,
    payload: EditorStatePayload,
) -> Result<(), String> {
    if !crate::realtime_cowrite_enabled() {
        return Ok(());
    }

    let prefix = payload.prefix.trim_end();
    if prefix.chars().count() < 12 {
        return Ok(());
    }

    let request_id = payload.request_id.clone();
    let cursor_position = payload.cursor_position;
    let cancel = CancellationToken::new();
    let render_target = EditorGhostRenderTarget {
        request_id: request_id.clone(),
        cursor_position,
    };

    {
        let state = app.state::<AppState>();
        crate::abort_editor_prediction_task(&state, None)?;
        let mut task = crate::lock_editor_prediction(&state)?;
        *task = Some(EditorPredictionTask {
            request_id: request_id.clone(),
            cancel: cancel.clone(),
        });
    }

    let project_id = storage::active_project_id(&app)?;
    let observation = crate::build_writer_observation_from_editor_state(&payload, &project_id);
    let (proposals, context_pack_for_llm) = {
        let state = app.state::<AppState>();
        let mut kernel = state.writer_kernel.lock().map_err(|e| e.to_string())?;
        crate::refresh_kernel_canon_from_lorebook(&app, &mut kernel);
        let proposals = kernel.observe(observation.clone())?;
        let context_pack = if proposals
            .iter()
            .any(|proposal| proposal.kind == writer_agent::proposal::ProposalKind::Ghost)
            && crate::resolve_api_key().is_some()
        {
            Some(kernel.ghost_context_pack(&observation))
        } else {
            None
        };
        (proposals, context_pack)
    };

    for proposal in &proposals {
        app.emit(events::AGENT_PROPOSAL, proposal.clone())
            .map_err(|e| format!("Failed to emit agent proposal: {}", e))?;
    }

    if let Some(proposal) = proposals
        .iter()
        .find(|proposal| proposal.kind == writer_agent::proposal::ProposalKind::Ghost)
    {
        crate::emit_writer_ghost_proposal(
            &app,
            &render_target,
            proposal,
            false,
            context_pack_for_llm.is_none(),
        )?;
    } else {
        crate::emit_editor_ghost_end(&app, &render_target, "complete")?;
    }

    if let Some(context_pack) = context_pack_for_llm {
        crate::spawn_llm_ghost_proposal(
            app.clone(),
            observation,
            context_pack,
            Some(render_target),
        );
        return Ok(());
    }

    drop(cancel);

    Ok(())
}

#[tauri::command]
pub async fn report_semantic_lint_state(
    app: tauri::AppHandle,
    payload: SemanticLintPayload,
) -> Result<(), String> {
    if !crate::semantic_lint_enabled() {
        return Ok(());
    }

    let app_clone = app.clone();
    tauri::async_runtime::spawn(async move {
        let _intent = agent_harness_core::Intent::Linter;
        if let Some(lint) = crate::find_semantic_lint(&app_clone, &payload) {
            let _ = app_clone.emit(events::EDITOR_SEMANTIC_LINT, lint);
        }
    });

    Ok(())
}
