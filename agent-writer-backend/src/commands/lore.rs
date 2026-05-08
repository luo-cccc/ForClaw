use crate::storage::LoreEntry;

#[tauri::command]
pub fn get_lorebook(app: tauri::AppHandle) -> Result<Vec<LoreEntry>, String> {
    crate::storage::load_lorebook(&app)
}

#[tauri::command]
pub fn save_lore_entry(
    app: tauri::AppHandle,
    keyword: String,
    content: String,
) -> Result<Vec<LoreEntry>, String> {
    let entries = crate::storage::upsert_lore_entry(&app, keyword.clone(), content.clone())?;
    crate::audit_project_file_write(
        &app,
        "lorebook",
        &format!("Lore saved: {}", keyword),
        "saved_lore_entry",
        &format!(
            "Author saved lore entry '{}' ({} chars).",
            keyword,
            content.chars().count()
        ),
        &[format!("lore:{}", keyword)],
    );
    Ok(entries)
}

#[tauri::command]
pub fn delete_lore_entry(app: tauri::AppHandle, id: String) -> Result<Vec<LoreEntry>, String> {
    let entries = crate::storage::remove_lore_entry(&app, id.clone())?;
    crate::audit_project_file_write(
        &app,
        "lorebook",
        &format!("Lore deleted: {}", id),
        "deleted_lore_entry",
        &format!("Author deleted lore entry '{}'.", id),
        &[format!("lore:{}", id)],
    );
    Ok(entries)
}
