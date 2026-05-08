use crate::storage::OutlineNode;
use crate::writer_agent::memory::{BookStateSummary, VolumeSnapshotSummary, VolumeSummary};
use tauri::Manager;

#[tauri::command]
pub fn get_outline(app: tauri::AppHandle) -> Result<Vec<OutlineNode>, String> {
    crate::storage::load_outline(&app)
}

#[tauri::command]
pub fn save_outline_node(
    app: tauri::AppHandle,
    chapter_title: String,
    summary: String,
) -> Result<Vec<OutlineNode>, String> {
    let nodes = crate::storage::upsert_outline_node(&app, chapter_title.clone(), summary.clone())?;
    crate::audit_project_file_write(
        &app,
        &chapter_title,
        &format!("Outline saved: {}", chapter_title),
        "saved_outline_node",
        &format!(
            "Author saved outline for '{}' ({} chars).",
            chapter_title,
            summary.chars().count()
        ),
        &[format!("outline:{}", chapter_title)],
    );
    Ok(nodes)
}

#[tauri::command]
pub fn delete_outline_node(
    app: tauri::AppHandle,
    chapter_title: String,
) -> Result<Vec<OutlineNode>, String> {
    let nodes = crate::storage::remove_outline_node(&app, chapter_title.clone())?;
    crate::audit_project_file_write(
        &app,
        &chapter_title,
        &format!("Outline deleted: {}", chapter_title),
        "deleted_outline_node",
        &format!("Author deleted outline node '{}'.", chapter_title),
        &[format!("outline:{}", chapter_title)],
    );
    Ok(nodes)
}

#[tauri::command]
pub fn update_outline_status(
    app: tauri::AppHandle,
    chapter_title: String,
    status: String,
) -> Result<Vec<OutlineNode>, String> {
    let nodes = crate::storage::update_outline_status(&app, chapter_title.clone(), status.clone())?;
    crate::audit_project_file_write(
        &app,
        &chapter_title,
        &format!("Outline status: {}", chapter_title),
        "updated_outline_status",
        &format!(
            "Author set outline status for '{}' to '{}'.",
            chapter_title, status
        ),
        &[format!("outline:{}", chapter_title)],
    );
    Ok(nodes)
}

#[tauri::command]
pub fn reorder_outline_nodes(
    app: tauri::AppHandle,
    ordered_titles: Vec<String>,
) -> Result<Vec<OutlineNode>, String> {
    let nodes = crate::storage::reorder_outline_nodes(&app, ordered_titles.clone())?;
    crate::audit_project_file_write(
        &app,
        "outline",
        "Outline reordered",
        "reordered_outline",
        &format!("Author reordered {} outline nodes.", ordered_titles.len()),
        &["outline:order".to_string()],
    );
    Ok(nodes)
}

#[tauri::command]
pub fn list_volumes(app: tauri::AppHandle) -> Result<Vec<VolumeSummary>, String> {
    let state = app.state::<crate::AppState>();
    let kernel = state
        .writer_kernel
        .lock()
        .map_err(|_| "Writer kernel lock poisoned".to_string())?;
    kernel
        .memory
        .list_volumes(&kernel.project_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn save_volume(app: tauri::AppHandle, volume: VolumeSummary) -> Result<VolumeSummary, String> {
    let state = app.state::<crate::AppState>();
    let kernel = state
        .writer_kernel
        .lock()
        .map_err(|_| "Writer kernel lock poisoned".to_string())?;
    kernel
        .memory
        .upsert_volume(&volume)
        .map_err(|e| e.to_string())?;
    crate::audit_project_file_write(
        &app,
        &format!("volume:{}", volume.id),
        &format!("Volume saved: {}", volume.title),
        "saved_volume",
        &format!(
            "Author saved volume '{}' (chapters {}-{}, status '{}').",
            volume.title, volume.start_chapter, volume.end_chapter, volume.status
        ),
        &[format!("volume:{}", volume.id)],
    );
    Ok(volume)
}

#[tauri::command]
pub fn delete_volume(app: tauri::AppHandle, volume_id: String) -> Result<bool, String> {
    let state = app.state::<crate::AppState>();
    let kernel = state
        .writer_kernel
        .lock()
        .map_err(|_| "Writer kernel lock poisoned".to_string())?;
    let deleted = kernel
        .memory
        .delete_volume(&kernel.project_id, &volume_id)
        .map_err(|e| e.to_string())?;
    if deleted {
        crate::audit_project_file_write(
            &app,
            &format!("volume:{}", volume_id),
            "Volume deleted",
            "deleted_volume",
            &format!("Author deleted volume '{}'.", volume_id),
            &[format!("volume:{}", volume_id)],
        );
    }
    Ok(deleted)
}

#[tauri::command]
pub fn get_volume_snapshot(
    app: tauri::AppHandle,
    volume_id: String,
) -> Result<Option<VolumeSnapshotSummary>, String> {
    let state = app.state::<crate::AppState>();
    let kernel = state
        .writer_kernel
        .lock()
        .map_err(|_| "Writer kernel lock poisoned".to_string())?;
    kernel
        .memory
        .get_latest_volume_snapshot(&kernel.project_id, &volume_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn save_volume_snapshot(
    app: tauri::AppHandle,
    snapshot: VolumeSnapshotSummary,
) -> Result<i64, String> {
    let state = app.state::<crate::AppState>();
    let kernel = state
        .writer_kernel
        .lock()
        .map_err(|_| "Writer kernel lock poisoned".to_string())?;
    let id = kernel
        .memory
        .upsert_volume_snapshot(&snapshot)
        .map_err(|e| e.to_string())?;
    crate::audit_project_file_write(
        &app,
        &format!("volume_snapshot:{}", snapshot.volume_id),
        "Volume snapshot saved",
        "saved_volume_snapshot",
        &format!("Author saved volume snapshot for '{}'.", snapshot.volume_id),
        &[format!("volume_snapshot:{}", snapshot.volume_id)],
    );
    Ok(id)
}

#[tauri::command]
pub fn get_book_state(app: tauri::AppHandle) -> Result<Option<BookStateSummary>, String> {
    let state = app.state::<crate::AppState>();
    let kernel = state
        .writer_kernel
        .lock()
        .map_err(|_| "Writer kernel lock poisoned".to_string())?;
    kernel
        .memory
        .get_book_state(&kernel.project_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn save_book_state(
    app: tauri::AppHandle,
    book_state: BookStateSummary,
) -> Result<BookStateSummary, String> {
    let state = app.state::<crate::AppState>();
    let kernel = state
        .writer_kernel
        .lock()
        .map_err(|_| "Writer kernel lock poisoned".to_string())?;
    kernel
        .memory
        .upsert_book_state(&book_state)
        .map_err(|e| e.to_string())?;
    crate::audit_project_file_write(
        &app,
        "book_state",
        "Book state saved",
        "saved_book_state",
        &format!("Author saved book state '{}'.", book_state.title),
        &["book_state".to_string()],
    );
    Ok(book_state)
}
