use crate::storage::ChapterInfo;
use std::sync::Mutex;
use tauri::Manager;

#[tauri::command]
pub fn read_project_dir(app: tauri::AppHandle) -> Result<Vec<ChapterInfo>, String> {
    crate::storage::read_project_dir(&app)
}

#[tauri::command]
pub fn create_chapter(app: tauri::AppHandle, title: String) -> Result<ChapterInfo, String> {
    let chapter = crate::storage::create_chapter(&app, title.clone())?;
    crate::audit_project_file_write(
        &app,
        &title,
        &format!("Chapter created: {}", title),
        "created_chapter",
        &format!("Author created chapter '{}'.", title),
        &[format!("chapter:{}", title)],
    );
    Ok(chapter)
}

#[tauri::command]
pub fn load_chapter(app: tauri::AppHandle, title: String) -> Result<String, String> {
    crate::storage::load_chapter(&app, title)
}

#[tauri::command]
pub fn get_chapter_revision(app: tauri::AppHandle, title: String) -> Result<String, String> {
    crate::storage::chapter_revision(&app, &title)
}

#[tauri::command]
pub fn rename_chapter_file(
    app: tauri::AppHandle,
    old_name: String,
    new_name: String,
) -> Result<(), String> {
    crate::storage::rename_chapter_file(&app, old_name.clone(), new_name.clone())?;
    crate::audit_project_file_write(
        &app,
        &new_name,
        &format!("Chapter renamed: {} -> {}", old_name, new_name),
        "renamed_chapter",
        &format!("Author renamed chapter '{}' to '{}'.", old_name, new_name),
        &[
            format!("chapter:{}", old_name),
            format!("chapter:{}", new_name),
        ],
    );
    Ok(())
}

#[tauri::command]
pub fn save_chapter(
    app: tauri::AppHandle,
    title: String,
    content: String,
) -> Result<String, String> {
    let revision = crate::storage::save_chapter_content_and_revision(&app, &title, &content)?;
    crate::audit_project_file_write(
        &app,
        &title,
        &format!("Chapter saved: {}", title),
        "saved_chapter",
        &format!(
            "Chapter '{}' saved with revision {} ({} chars).",
            title,
            revision,
            crate::html_to_plain_text(&content).chars().count()
        ),
        &[format!("chapter:{}:{}", title, revision)],
    );
    if let Err(e) = crate::observe_chapter_save(&app, &title, &content, &revision) {
        tracing::warn!("WriterAgent chapter-save observation failed: {}", e);
    }

    if let Some(bus_state) = app.try_state::<Mutex<agent_harness_core::AmbientEventBus>>() {
        if let Ok(bus) = bus_state.lock() {
            let _ = bus.publish(agent_harness_core::ambient::EditorEvent::ChapterSaved {
                chapter: title.clone(),
                content_length: content.chars().count(),
                revision: revision.clone(),
            });
        }
    }

    let app_clone = app.clone();
    let title_clone = title.clone();
    let content_clone = content.clone();
    tauri::async_runtime::spawn(async move {
        crate::auto_embed_chapter(&app_clone, &title_clone, &content_clone).await;
    });

    Ok(revision)
}
