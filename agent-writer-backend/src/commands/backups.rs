//! Backup and storage diagnostic Tauri commands.

use crate::storage;

#[tauri::command]
pub fn get_project_storage_diagnostics(
    app: tauri::AppHandle,
) -> Result<storage::ProjectStorageDiagnostics, String> {
    storage::project_storage_diagnostics(&app)
}

#[tauri::command]
pub fn list_file_backups(
    app: tauri::AppHandle,
    target: storage::BackupTarget,
) -> Result<Vec<storage::FileBackupInfo>, String> {
    storage::list_file_backups(&app, target)
}

#[tauri::command]
pub fn restore_file_backup(
    app: tauri::AppHandle,
    target: storage::BackupTarget,
    backup_id: String,
) -> Result<(), String> {
    let label = crate::backup_target_label(&target);
    storage::restore_file_backup(&app, target, backup_id.clone())?;
    crate::audit_project_file_write(
        &app,
        &label,
        &format!("Backup restored: {}", label),
        "restored_file_backup",
        &format!("Author restored backup '{}' for {}.", backup_id, label),
        &[format!("backup:{}:{}", label, backup_id)],
    );
    Ok(())
}
