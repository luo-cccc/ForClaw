use tauri::Manager;

use crate::{storage, AppState};

pub(crate) fn audit_project_file_write(
    app: &tauri::AppHandle,
    scope: &str,
    title: &str,
    decision: &str,
    rationale: &str,
    sources: &[String],
) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    let Ok(kernel) = state.writer_kernel.lock() else {
        return;
    };
    if let Err(e) = kernel
        .memory
        .record_decision(scope, title, decision, &[], rationale, sources)
    {
        tracing::warn!("WriterAgent file-write audit failed: {}", e);
    }
}

pub(crate) fn backup_target_label(target: &storage::BackupTarget) -> String {
    match target {
        storage::BackupTarget::Lorebook => "lorebook".to_string(),
        storage::BackupTarget::Outline => "outline".to_string(),
        storage::BackupTarget::ProjectBrain => "project_brain".to_string(),
        storage::BackupTarget::Chapter { title } => format!("chapter:{}", title),
    }
}
