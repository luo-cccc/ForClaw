
pub fn lorebook_path(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    let target = active_project_data_dir(app)?.join("lorebook.json");
    let legacy = app_data_dir(app)?.join("lorebook.json");
    migrate_legacy_file_if_needed(&target, &legacy)?;
    Ok(target)
}

pub fn load_lorebook(app: &tauri::AppHandle) -> Result<Vec<LoreEntry>, String> {
    let path = lorebook_path(app)?;
    if !path.exists() {
        return Ok(vec![]);
    }
    let data = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    parse_json_list(&data, &path, "lorebook")
}

pub fn save_lorebook(app: &tauri::AppHandle, entries: &[LoreEntry]) -> Result<(), String> {
    let path = lorebook_path(app)?;
    let json = serde_json::to_string_pretty(entries).map_err(|e| e.to_string())?;
    std::fs::write(&path, json).map_err(|e| e.to_string())
}

pub fn upsert_lore_entry(
    app: &tauri::AppHandle,
    keyword: String,
    content: String,
) -> Result<Vec<LoreEntry>, String> {
    let mut entries = load_lorebook(app)?;
    if let Some(entry) = entries.iter_mut().find(|e| e.keyword == keyword) {
        entry.content = content;
    } else {
        let id = (entries.len() + 1).to_string();
        entries.push(LoreEntry {
            id,
            keyword,
            content,
        });
    }
    save_lorebook(app, &entries)?;
    Ok(entries)
}

pub fn remove_lore_entry(app: &tauri::AppHandle, id: String) -> Result<Vec<LoreEntry>, String> {
    let mut entries = load_lorebook(app)?;
    entries.retain(|e| e.id != id);
    save_lorebook(app, &entries)?;
    Ok(entries)
}

pub fn project_dir(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    let dir = active_project_data_dir(app)?.join("chapters");
    let legacy = app_data_dir(app)?.join("project");
    migrate_legacy_dir_if_needed(&dir, &legacy)?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("Failed to create project dir: {}", e))?;
    Ok(dir)
}

pub fn chapter_filename(title: &str) -> String {
    let mut safe = String::new();
    let mut last_was_dash = false;
    for ch in title.trim().to_lowercase().chars() {
        let next = if ch.is_ascii_alphanumeric() || is_cjk(ch) {
            Some(ch)
        } else if ch == ' ' || ch == '-' || ch == '_' {
            Some('-')
        } else {
            None
        };

        if let Some(ch) = next {
            if ch == '-' {
                if last_was_dash {
                    continue;
                }
                last_was_dash = true;
            } else {
                last_was_dash = false;
            }
            safe.push(ch);
        }
    }

    let safe = safe.trim_matches('-');
    let stem = if safe.is_empty() { "untitled" } else { safe };
    format!("{}.md", stem)
}

pub fn chapter_path(app: &tauri::AppHandle, title: &str) -> Result<std::path::PathBuf, String> {
    Ok(project_dir(app)?.join(chapter_filename(title)))
}

fn is_cjk(ch: char) -> bool {
    matches!(
        ch as u32,
        0x3400..=0x4DBF
            | 0x4E00..=0x9FFF
            | 0xF900..=0xFAFF
            | 0x20000..=0x2A6DF
            | 0x2A700..=0x2B73F
            | 0x2B740..=0x2B81F
            | 0x2B820..=0x2CEAF
    )
}

pub fn content_revision(content: &str) -> String {
    const FNV_OFFSET: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;

    let mut hash = FNV_OFFSET;
    for byte in content.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    format!("{:016x}-{}", hash, content.len())
}

pub fn chapter_revision(app: &tauri::AppHandle, title: &str) -> Result<String, String> {
    let path = chapter_path(app, title)?;
    if !path.exists() {
        return Ok("missing".to_string());
    }
    let content = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    Ok(content_revision(&content))
}

pub fn read_project_dir(app: &tauri::AppHandle) -> Result<Vec<ChapterInfo>, String> {
    let dir = project_dir(app)?;
    let mut chapters = Vec::new();
    let entries = std::fs::read_dir(&dir).map_err(|e| e.to_string())?;
    for entry in entries {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        if path.extension().map(|e| e == "md").unwrap_or(false) {
            let stem = path
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            let title = stem.replace('-', " ");
            chapters.push(ChapterInfo {
                filename: path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string(),
                title,
            });
        }
    }
    chapters.sort_by(|a, b| a.title.cmp(&b.title));
    Ok(chapters)
}

pub fn create_chapter(app: &tauri::AppHandle, title: String) -> Result<ChapterInfo, String> {
    let filename = chapter_filename(&title);
    let path = chapter_path(app, &title)?;
    if !path.exists() {
        atomic_write(&path, "")?;
    }
    Ok(ChapterInfo { title, filename })
}

pub fn save_chapter_content(
    app: &tauri::AppHandle,
    title: &str,
    content: &str,
) -> Result<(), String> {
    let path = chapter_path(app, title)?;
    atomic_write(&path, content)
}

pub fn save_chapter_content_and_revision(
    app: &tauri::AppHandle,
    title: &str,
    content: &str,
) -> Result<String, String> {
    save_chapter_content(app, title, content)?;
    Ok(content_revision(content))
}

pub fn load_chapter(app: &tauri::AppHandle, title: String) -> Result<String, String> {
    let path = chapter_path(app, &title)?;
    if !path.exists() {
        return Err(format!("Chapter '{}' not found", title));
    }
    std::fs::read_to_string(&path).map_err(|e| e.to_string())
}

pub fn outline_path(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    let target = active_project_data_dir(app)?.join("outline.json");
    let legacy = app_data_dir(app)?.join("outline.json");
    migrate_legacy_file_if_needed(&target, &legacy)?;
    Ok(target)
}

pub fn load_outline(app: &tauri::AppHandle) -> Result<Vec<OutlineNode>, String> {
    let path = outline_path(app)?;
    if !path.exists() {
        return Ok(vec![]);
    }
    let data = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    parse_json_list(&data, &path, "outline")
}

pub fn save_outline(app: &tauri::AppHandle, nodes: &[OutlineNode]) -> Result<(), String> {
    let path = outline_path(app)?;
    let json = serde_json::to_string_pretty(nodes).map_err(|e| e.to_string())?;
    atomic_write(&path, &json)
}

pub fn upsert_outline_node(
    app: &tauri::AppHandle,
    chapter_title: String,
    summary: String,
) -> Result<Vec<OutlineNode>, String> {
    let mut nodes = load_outline(app)?;
    let status = if let Some(existing) = nodes.iter().find(|n| n.chapter_title == chapter_title) {
        existing.status.clone()
    } else {
        "empty".to_string()
    };
    if let Some(node) = nodes.iter_mut().find(|n| n.chapter_title == chapter_title) {
        node.summary = summary;
    } else {
        nodes.push(OutlineNode {
            chapter_title,
            summary,
            status,
        });
    }
    save_outline(app, &nodes)?;
    Ok(nodes)
}

pub fn remove_outline_node(
    app: &tauri::AppHandle,
    chapter_title: String,
) -> Result<Vec<OutlineNode>, String> {
    let mut nodes = load_outline(app)?;
    nodes.retain(|n| n.chapter_title != chapter_title);
    save_outline(app, &nodes)?;
    Ok(nodes)
}

pub fn update_outline_status(
    app: &tauri::AppHandle,
    chapter_title: String,
    status: String,
) -> Result<Vec<OutlineNode>, String> {
    let mut nodes = load_outline(app)?;
    if let Some(node) = nodes.iter_mut().find(|n| n.chapter_title == chapter_title) {
        node.status = status;
    }
    save_outline(app, &nodes)?;
    Ok(nodes)
}

pub fn patch_outline_node(
    app: &tauri::AppHandle,
    chapter_title: String,
    patch: serde_json::Value,
) -> Result<Vec<OutlineNode>, String> {
    let mut nodes = load_outline(app)?;
    apply_outline_patch(&mut nodes, &chapter_title, &patch)?;
    save_outline(app, &nodes)?;
    Ok(nodes)
}

fn apply_outline_patch(
    nodes: &mut [OutlineNode],
    chapter_title: &str,
    patch: &serde_json::Value,
) -> Result<(), String> {
    let patch = patch
        .as_object()
        .ok_or_else(|| "outline patch must be an object".to_string())?;
    let node = nodes
        .iter_mut()
        .find(|node| node.chapter_title == chapter_title)
        .ok_or_else(|| format!("Outline node '{}' not found", chapter_title))?;

    for (key, value) in patch {
        match key.as_str() {
            "chapterTitle" | "chapter_title" => {
                let next = value
                    .as_str()
                    .ok_or_else(|| "outline chapterTitle must be a string".to_string())?
                    .trim();
                if next.is_empty() {
                    return Err("outline chapterTitle cannot be empty".to_string());
                }
                node.chapter_title = next.to_string();
            }
            "summary" => {
                node.summary = value
                    .as_str()
                    .ok_or_else(|| "outline summary must be a string".to_string())?
                    .to_string();
            }
            "status" => {
                node.status = value
                    .as_str()
                    .ok_or_else(|| "outline status must be a string".to_string())?
                    .to_string();
            }
            other => return Err(format!("Unsupported outline patch field '{}'", other)),
        }
    }

    Ok(())
}

pub fn reorder_outline_nodes(
    app: &tauri::AppHandle,
    ordered_titles: Vec<String>,
) -> Result<Vec<OutlineNode>, String> {
    let nodes = load_outline(app)?;
    let mut reordered = Vec::with_capacity(nodes.len());

    for title in &ordered_titles {
        if let Some(node) = nodes.iter().find(|n| &n.chapter_title == title) {
            reordered.push(node.clone());
        }
    }

    for node in nodes {
        if !ordered_titles
            .iter()
            .any(|title| title == &node.chapter_title)
        {
            reordered.push(node);
        }
    }

    save_outline(app, &reordered)?;
    Ok(reordered)
}
