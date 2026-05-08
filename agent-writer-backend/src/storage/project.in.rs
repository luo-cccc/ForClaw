
pub fn app_data_dir(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to get app data dir: {}", e))?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("Failed to create dir: {}", e))?;
    Ok(dir)
}

fn project_manifest_path(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    Ok(app_data_dir(app)?.join("active_project.json"))
}

fn generated_local_project_id(data_dir: &std::path::Path) -> String {
    let fingerprint = content_revision(&data_dir.to_string_lossy());
    let hash = fingerprint.split('-').next().unwrap_or("0000000000000000");
    format!("local-{}", hash)
}

fn valid_project_id(id: &str) -> bool {
    !id.trim().is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

pub fn active_project_manifest(app: &tauri::AppHandle) -> Result<ProjectManifest, String> {
    let path = project_manifest_path(app)?;
    if path.exists() {
        let data = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
        let manifest: ProjectManifest = serde_json::from_str(&data)
            .map_err(|e| format!("Failed to parse project manifest: {}", e))?;
        if !valid_project_id(&manifest.id) {
            return Err(format!("Invalid project id in manifest: {}", manifest.id));
        }
        return Ok(manifest);
    }

    let data_dir = app_data_dir(app)?;
    let manifest = ProjectManifest {
        id: generated_local_project_id(&data_dir),
        name: "Local Project".to_string(),
    };
    let json = serde_json::to_string_pretty(&manifest).map_err(|e| e.to_string())?;
    atomic_write(&path, &json)?;
    Ok(manifest)
}

pub fn active_project_id(app: &tauri::AppHandle) -> Result<String, String> {
    Ok(active_project_manifest(app)?.id)
}

pub fn active_project_data_dir(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    let manifest = active_project_manifest(app)?;
    let dir = app_data_dir(app)?.join("projects").join(manifest.id);
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("Failed to create active project dir: {}", e))?;
    Ok(dir)
}

fn migrate_legacy_file_if_needed(
    target: &std::path::Path,
    legacy: &std::path::Path,
) -> Result<(), String> {
    if target.exists() || !legacy.exists() || target == legacy {
        return Ok(());
    }

    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            format!(
                "Failed to create parent dir for '{}': {}",
                target.display(),
                e
            )
        })?;
    }
    std::fs::copy(legacy, target).map_err(|e| {
        format!(
            "Failed to migrate '{}' to '{}': {}",
            legacy.display(),
            target.display(),
            e
        )
    })?;
    Ok(())
}

fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) -> Result<(), String> {
    std::fs::create_dir_all(dst)
        .map_err(|e| format!("Failed to create directory '{}': {}", dst.display(), e))?;
    for entry in std::fs::read_dir(src)
        .map_err(|e| format!("Failed to read directory '{}': {}", src.display(), e))?
    {
        let entry = entry.map_err(|e| e.to_string())?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            if let Some(parent) = dst_path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| {
                    format!(
                        "Failed to create parent dir for '{}': {}",
                        dst_path.display(),
                        e
                    )
                })?;
            }
            std::fs::copy(&src_path, &dst_path).map_err(|e| {
                format!(
                    "Failed to copy '{}' to '{}': {}",
                    src_path.display(),
                    dst_path.display(),
                    e
                )
            })?;
        }
    }
    Ok(())
}

fn migrate_legacy_dir_if_needed(
    target: &std::path::Path,
    legacy: &std::path::Path,
) -> Result<(), String> {
    if target.exists() || !legacy.exists() || target == legacy {
        return Ok(());
    }
    copy_dir_recursive(legacy, target)
}

fn parse_json_list<T: for<'de> Deserialize<'de>>(
    data: &str,
    path: &std::path::Path,
    label: &str,
) -> Result<Vec<T>, String> {
    serde_json::from_str(data)
        .map_err(|e| format!("Failed to parse {} at '{}': {}", label, path.display(), e))
}
