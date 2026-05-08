use tauri::{Emitter, Manager};

use crate::{agent_runtime, chapter_generation, events, storage, writer_agent, AppState};

pub(crate) fn observe_chapter_save(
    app: &tauri::AppHandle,
    title: &str,
    content: &str,
    revision: &str,
) -> Result<(), String> {
    observe_chapter_save_with_result(app, title, content, revision, None)
}

pub(crate) fn observe_chapter_save_with_result(
    app: &tauri::AppHandle,
    title: &str,
    content: &str,
    revision: &str,
    precomputed_result: Option<writer_agent::memory::ChapterResultSummary>,
) -> Result<(), String> {
    let project_id = storage::active_project_id(app)?;
    let text = html_to_plain_text(content);
    let paragraph = last_meaningful_paragraph(&text).unwrap_or_else(|| char_tail(&text, 400));
    let cursor = text.chars().count();
    let observation = writer_agent::observation::WriterObservation {
        id: format!("save-{}", agent_runtime::now_ms()),
        created_at: agent_runtime::now_ms(),
        source: writer_agent::observation::ObservationSource::ChapterSave,
        reason: writer_agent::observation::ObservationReason::Save,
        project_id,
        chapter_title: Some(title.to_string()),
        chapter_revision: Some(revision.to_string()),
        cursor: Some(writer_agent::observation::TextRange {
            from: cursor,
            to: cursor,
        }),
        selection: None,
        prefix: text.clone(),
        suffix: String::new(),
        paragraph,
        full_text_digest: Some(storage::content_revision(&text)),
        editor_dirty: false,
    };

    let state = app.state::<AppState>();
    let proposals = {
        let mut kernel = state.writer_kernel.lock().map_err(|e| e.to_string())?;
        refresh_kernel_canon_from_lorebook(app, &mut kernel);
        if let Some(result) = precomputed_result {
            kernel.observe_save_result(observation.clone(), result)?
        } else {
            kernel.observe(observation.clone())?
        }
    };
    for proposal in proposals {
        app.emit(events::AGENT_PROPOSAL, proposal)
            .map_err(|e| format!("Failed to emit agent proposal: {}", e))?;
    }

    Ok(())
}

pub(crate) fn observe_generated_chapter_result(
    app: &tauri::AppHandle,
    saved: &chapter_generation::SaveGeneratedChapterOutput,
    generated_content: &str,
    settlement_delta: Option<&chapter_generation::ChapterSettlementDelta>,
) {
    let precomputed_result = settlement_delta.and_then(|delta| {
        let project_id = storage::active_project_id(app).ok()?;
        let memory_path = storage::active_project_data_dir(app)
            .ok()?
            .join(storage::WRITER_MEMORY_DB_FILENAME);
        let memory = writer_agent::memory::WriterMemory::open(&memory_path).ok()?;
        let created_at = memory
            .latest_chapter_result(&project_id, &delta.chapter_title)
            .ok()
            .flatten()
            .filter(|result| result.chapter_revision == delta.chapter_revision)
            .map(|result| result.created_at)
            .unwrap_or_else(agent_runtime::now_ms);
        Some(writer_agent::memory::ChapterResultSummary {
            id: 0,
            project_id,
            chapter_title: delta.chapter_title.clone(),
            chapter_revision: delta.chapter_revision.clone(),
            summary: delta.chapter_result.summary.clone(),
            state_changes: delta.chapter_result.state_changes.clone(),
            character_progress: delta.chapter_result.character_progress.clone(),
            new_conflicts: delta.chapter_result.new_conflicts.clone(),
            new_clues: delta.chapter_result.new_clues.clone(),
            promise_updates: delta.chapter_result.promise_updates.clone(),
            canon_updates: delta.chapter_result.canon_updates.clone(),
            source_ref: format!(
                "chapter_settlement:{}:{}",
                delta.chapter_title, delta.chapter_revision
            ),
            created_at,
        })
    });
    if let Err(e) = observe_chapter_save_with_result(
        app,
        &saved.chapter_title,
        generated_content,
        &saved.new_revision,
        precomputed_result,
    ) {
        tracing::warn!(
            "WriterAgent generated-chapter result feedback failed for '{}': {}",
            saved.chapter_title,
            e
        );
    }
}

pub(crate) fn last_meaningful_paragraph(text: &str) -> Option<String> {
    text.split('\n')
        .rev()
        .map(str::trim)
        .find(|line| line.chars().count() >= 8)
        .map(ToString::to_string)
}

pub(crate) fn html_to_plain_text(html: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    let mut entity = String::new();
    let mut in_entity = false;

    for ch in html.chars() {
        if in_tag {
            if ch == '>' {
                in_tag = false;
                if !out.ends_with('\n') {
                    out.push('\n');
                }
            }
            continue;
        }

        if in_entity {
            if ch == ';' {
                out.push_str(&decode_html_entity(&entity));
                entity.clear();
                in_entity = false;
            } else if entity.chars().count() < 12 {
                entity.push(ch);
            } else {
                out.push('&');
                out.push_str(&entity);
                out.push(ch);
                entity.clear();
                in_entity = false;
            }
            continue;
        }

        match ch {
            '<' => in_tag = true,
            '&' => in_entity = true,
            '\r' => {}
            _ => out.push(ch),
        }
    }

    if in_entity {
        out.push('&');
        out.push_str(&entity);
    }

    out.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

pub(crate) fn decode_html_entity(entity: &str) -> String {
    match entity {
        "amp" => "&".to_string(),
        "lt" => "<".to_string(),
        "gt" => ">".to_string(),
        "quot" => "\"".to_string(),
        "apos" => "'".to_string(),
        "nbsp" => " ".to_string(),
        entity if entity.starts_with("#x") || entity.starts_with("#X") => {
            u32::from_str_radix(&entity[2..], 16)
                .ok()
                .and_then(char::from_u32)
                .map(|c| c.to_string())
                .unwrap_or_else(|| format!("&{};", entity))
        }
        entity if entity.starts_with('#') => entity[1..]
            .parse::<u32>()
            .ok()
            .and_then(char::from_u32)
            .map(|c| c.to_string())
            .unwrap_or_else(|| format!("&{};", entity)),
        _ => format!("&{};", entity),
    }
}

pub(crate) fn refresh_kernel_canon_from_lorebook(
    app: &tauri::AppHandle,
    kernel: &mut writer_agent::WriterAgentKernel,
) {
    let entries = match storage::load_lorebook(app) {
        Ok(entries) => entries,
        Err(e) => {
            tracing::warn!("WriterAgent canon refresh skipped lorebook: {}", e);
            return;
        }
    };

    for entry in entries {
        let keyword = entry.keyword.trim();
        if keyword.is_empty() {
            continue;
        }

        let mut attributes = serde_json::Map::new();
        if let Some(weapon) = extract_weapon_from_lore(&entry.content) {
            attributes.insert("weapon".to_string(), serde_json::Value::String(weapon));
        }

        if attributes.is_empty() {
            continue;
        }

        let summary: String = entry.content.chars().take(240).collect();
        let aliases = Vec::<String>::new();
        let _ = kernel
            .memory
            .upsert_character(keyword, &aliases, "supporting", &summary);
    }
}

pub(crate) fn render_writer_context_pack(
    pack: &writer_agent::context::WritingContextPack,
) -> String {
    writer_agent::kernel::render_context_pack_for_prompt(pack)
}

pub(crate) fn char_tail(text: &str, max_chars: usize) -> String {
    let mut chars = text.chars().rev().take(max_chars).collect::<Vec<_>>();
    chars.reverse();
    chars.into_iter().collect()
}

fn extract_weapon_from_lore(content: &str) -> Option<String> {
    if !["武器", "惯用", "用刀", "用剑", "佩刀", "佩剑", "兵器"]
        .iter()
        .any(|cue| content.contains(cue))
    {
        return None;
    }

    [
        "寒影刀",
        "长剑",
        "短剑",
        "匕首",
        "弓",
        "枪",
        "棍",
        "鞭",
        "斧",
        "刀",
        "剑",
    ]
    .iter()
    .find(|weapon| content.contains(**weapon))
    .map(|weapon| (*weapon).to_string())
}
