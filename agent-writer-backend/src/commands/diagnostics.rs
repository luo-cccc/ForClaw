//! Diagnostic and export Tauri commands.

use crate::AppState;

#[tauri::command]
pub fn export_diagnostic_logs(app: tauri::AppHandle) -> Result<String, String> {
    use std::io::Write;

    let log_dir = crate::log_dir()?;
    let out_path = log_dir.join("diagnostic-export.zip");
    let file = std::fs::File::create(&out_path).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipWriter::new(file);
    let opts = zip::write::SimpleFileOptions::default();

    if let Ok(entries) = std::fs::read_dir(&log_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map(|e| e == "log").unwrap_or(false) {
                let name = path.file_name().unwrap_or_default().to_string_lossy();
                let content = std::fs::read_to_string(&path).unwrap_or_default();
                zip.start_file(&*name, opts).map_err(|e| e.to_string())?;
                zip.write_all(content.as_bytes())
                    .map_err(|e| e.to_string())?;
            }
        }
    }

    let storage_snapshot = match crate::storage::project_storage_diagnostics(&app) {
        Ok(snapshot) => serde_json::to_string_pretty(&snapshot).map_err(|e| e.to_string())?,
        Err(e) => serde_json::json!({
            "healthy": false,
            "error": e,
        })
        .to_string(),
    };
    zip.start_file("project-storage-diagnostics.json", opts)
        .map_err(|e| e.to_string())?;
    zip.write_all(storage_snapshot.as_bytes())
        .map_err(|e| e.to_string())?;

    zip.finish().map_err(|e| e.to_string())?;
    Ok(out_path.to_string_lossy().to_string())
}

#[tauri::command]
pub fn export_writer_agent_trajectory(
    state: tauri::State<'_, AppState>,
    limit: Option<usize>,
    format: Option<String>,
) -> Result<String, String> {
    let kernel = state.writer_kernel.lock().map_err(|e| e.to_string())?;
    let export = kernel.export_trajectory(limit.unwrap_or(200).min(1_000));
    let dir = crate::log_dir()?.join("trajectory");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let trace_viewer_format = matches!(
        format.as_deref(),
        Some("trace_viewer" | "claude_code" | "hf_agent_trace_viewer")
    );
    let file_name = format!(
        "writer-agent-{}-{}{}.jsonl",
        crate::safe_filename_component(&export.project_id),
        crate::agent_runtime::now_ms(),
        if trace_viewer_format {
            "-trace-viewer"
        } else {
            ""
        }
    );
    let path = dir.join(file_name);
    let jsonl = if trace_viewer_format {
        export.trace_viewer_jsonl
    } else {
        export.jsonl
    };
    std::fs::write(&path, jsonl).map_err(|e| e.to_string())?;
    Ok(path.to_string_lossy().to_string())
}
