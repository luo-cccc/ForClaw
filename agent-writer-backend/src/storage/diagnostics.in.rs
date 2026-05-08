
pub fn brain_path(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    let target = active_project_data_dir(app)?.join("project_brain.json");
    let legacy = app_data_dir(app)?.join("project_brain.json");
    migrate_legacy_file_if_needed(&target, &legacy)?;
    Ok(target)
}

pub fn project_storage_diagnostics(
    app: &tauri::AppHandle,
) -> Result<ProjectStorageDiagnostics, String> {
    let manifest = active_project_manifest(app)?;
    let app_data_dir = app_data_dir(app)?;
    let project_data_dir = active_project_data_dir(app)?;
    let chapters_dir = project_dir(app)?;

    let files = vec![
        diagnose_json_array_file::<LoreEntry>("lorebook", &lorebook_path(app)?),
        diagnose_json_array_file::<OutlineNode>("outline", &outline_path(app)?),
        diagnose_json_array_file::<agent_harness_core::vector_db::Chunk>(
            "project_brain",
            &brain_path(app)?,
        ),
        diagnose_chapters_directory(&chapters_dir),
    ];
    let databases = vec![
        diagnose_sqlite_database(
            "hermes_memory",
            &project_data_dir.join(HERMES_DB_FILENAME),
            HERMES_DIAGNOSTIC_TABLES,
        ),
        diagnose_sqlite_database(
            "writer_memory",
            &project_data_dir.join(WRITER_MEMORY_DB_FILENAME),
            WRITER_MEMORY_DIAGNOSTIC_TABLES,
        ),
    ];
    let healthy = files.iter().all(|file| file.status != "error")
        && databases.iter().all(|db| db.status == "ok");

    Ok(ProjectStorageDiagnostics {
        project_id: manifest.id,
        project_name: manifest.name,
        app_data_dir: app_data_dir.to_string_lossy().to_string(),
        project_data_dir: project_data_dir.to_string_lossy().to_string(),
        checked_at: unix_time_ms(),
        healthy,
        files,
        databases,
    })
}

pub fn list_file_backups(
    app: &tauri::AppHandle,
    target: BackupTarget,
) -> Result<Vec<FileBackupInfo>, String> {
    let target_path = backup_target_path(app, &target)?;
    let backup_dir = backup_dir_for(&target_path)?;
    if !backup_dir.exists() {
        return Ok(vec![]);
    }
    let mut backups = std::fs::read_dir(&backup_dir)
        .map_err(|e| {
            format!(
                "Failed to read backup dir '{}': {}",
                backup_dir.display(),
                e
            )
        })?
        .flatten()
        .filter(|entry| entry.path().is_file())
        .filter_map(|entry| backup_info(entry).ok())
        .collect::<Vec<_>>();
    backups.sort_by_key(|b| std::cmp::Reverse(b.modified_at));
    Ok(backups)
}

pub fn restore_file_backup(
    app: &tauri::AppHandle,
    target: BackupTarget,
    backup_id: String,
) -> Result<(), String> {
    let target_path = backup_target_path(app, &target)?;
    let backup_dir = backup_dir_for(&target_path)?;
    let backup_path = safe_backup_file_path(&backup_dir, &backup_id)?;
    if !backup_path.exists() {
        return Err(format!("Backup '{}' not found", backup_id));
    }
    let content = std::fs::read_to_string(&backup_path)
        .map_err(|e| format!("Failed to read backup '{}': {}", backup_path.display(), e))?;
    validate_backup_content(&target, &content, &backup_path)?;
    atomic_write(&target_path, &content)
}

fn backup_target_path(
    app: &tauri::AppHandle,
    target: &BackupTarget,
) -> Result<std::path::PathBuf, String> {
    match target {
        BackupTarget::Lorebook => lorebook_path(app),
        BackupTarget::Outline => outline_path(app),
        BackupTarget::ProjectBrain => brain_path(app),
        BackupTarget::Chapter { title } => chapter_path(app, title),
    }
}

fn validate_backup_content(
    target: &BackupTarget,
    content: &str,
    backup_path: &std::path::Path,
) -> Result<(), String> {
    match target {
        BackupTarget::Lorebook => serde_json::from_str::<Vec<LoreEntry>>(content)
            .map(|_| ())
            .map_err(|e| format!("Invalid lorebook backup '{}': {}", backup_path.display(), e)),
        BackupTarget::Outline => serde_json::from_str::<Vec<OutlineNode>>(content)
            .map(|_| ())
            .map_err(|e| format!("Invalid outline backup '{}': {}", backup_path.display(), e)),
        BackupTarget::ProjectBrain => {
            serde_json::from_str::<Vec<agent_harness_core::vector_db::Chunk>>(content)
                .map(|_| ())
                .map_err(|e| {
                    format!(
                        "Invalid project brain backup '{}': {}",
                        backup_path.display(),
                        e
                    )
                })
        }
        BackupTarget::Chapter { .. } => Ok(()),
    }
}

fn diagnose_json_array_file<T: for<'de> Deserialize<'de>>(
    label: &str,
    path: &std::path::Path,
) -> StorageFileDiagnostic {
    let mut diagnostic = base_file_diagnostic(label, path);
    if !diagnostic.exists {
        diagnostic.status = "missing".to_string();
        return diagnostic;
    }

    match std::fs::read_to_string(path) {
        Ok(data) => match serde_json::from_str::<Vec<T>>(&data) {
            Ok(rows) => {
                diagnostic.record_count = Some(rows.len());
                diagnostic.status = "ok".to_string();
            }
            Err(e) => {
                diagnostic.status = "error".to_string();
                diagnostic.error = Some(format!("JSON parse failed: {}", e));
            }
        },
        Err(e) => {
            diagnostic.status = "error".to_string();
            diagnostic.error = Some(format!("Read failed: {}", e));
        }
    }

    diagnostic
}

fn diagnose_chapters_directory(path: &std::path::Path) -> StorageFileDiagnostic {
    let mut diagnostic = base_file_diagnostic("chapters", path);
    if !diagnostic.exists {
        diagnostic.status = "missing".to_string();
        return diagnostic;
    }
    if !path.is_dir() {
        diagnostic.status = "error".to_string();
        diagnostic.error = Some("Expected a directory".to_string());
        return diagnostic;
    }

    match std::fs::read_dir(path) {
        Ok(entries) => {
            let mut count = 0usize;
            let mut bytes = 0u64;
            for entry in entries.flatten() {
                let entry_path = entry.path();
                if entry_path.extension().map(|e| e == "md").unwrap_or(false) {
                    count += 1;
                    if let Ok(metadata) = entry.metadata() {
                        bytes = bytes.saturating_add(metadata.len());
                    }
                }
            }
            diagnostic.record_count = Some(count);
            diagnostic.bytes = Some(bytes);
            diagnostic.status = "ok".to_string();
        }
        Err(e) => {
            diagnostic.status = "error".to_string();
            diagnostic.error = Some(format!("Read directory failed: {}", e));
        }
    }

    diagnostic
}

