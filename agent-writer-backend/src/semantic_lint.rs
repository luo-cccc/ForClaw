use serde::{Deserialize, Serialize};
use tauri::Manager;

use crate::{lock_hermes, storage, writer_agent, AppState};

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SemanticLintPayload {
    pub(crate) request_id: String,
    pub(crate) paragraph: String,
    pub(crate) paragraph_from: usize,
    pub(crate) cursor_position: usize,
    pub(crate) chapter_title: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EditorSemanticLint {
    pub(crate) request_id: String,
    pub(crate) cursor_position: usize,
    pub(crate) from: usize,
    pub(crate) to: usize,
    pub(crate) message: String,
    pub(crate) severity: String,
}

pub(crate) fn semantic_lint_enabled() -> bool {
    std::env::var("AGENT_WRITER_AMBIENT_LINTER")
        .map(|value| {
            let normalized = value.trim().to_lowercase();
            !matches!(normalized.as_str(), "0" | "false" | "off" | "disabled")
        })
        .unwrap_or(true)
}

pub(crate) fn find_semantic_lint(
    app: &tauri::AppHandle,
    payload: &SemanticLintPayload,
) -> Option<EditorSemanticLint> {
    let paragraph = payload.paragraph.trim();
    if paragraph.chars().count() < 8 {
        return None;
    }
    let chapter_label = payload
        .chapter_title
        .as_deref()
        .filter(|title| !title.trim().is_empty())
        .unwrap_or("当前章节");

    if let Some(lint) = find_writer_agent_diagnostic_lint(app, payload, chapter_label) {
        return Some(lint);
    }

    let lore_entries = match storage::load_lorebook(app) {
        Ok(entries) => entries,
        Err(e) => {
            tracing::warn!(
                "Semantic lint skipped lorebook because it failed to load: {}",
                e
            );
            Vec::new()
        }
    };
    for entry in lore_entries {
        if let Some((from, to, message)) =
            build_lore_conflict_hint(paragraph, &entry.keyword, &entry.content)
        {
            return Some(EditorSemanticLint {
                request_id: payload.request_id.clone(),
                cursor_position: payload.cursor_position,
                from: payload.paragraph_from + from,
                to: payload.paragraph_from + to,
                message: format!("{}：{}", chapter_label, message),
                severity: "warning".to_string(),
            });
        }
    }

    let state = app.state::<AppState>();
    let Ok(db) = lock_hermes(&state) else {
        return None;
    };
    let skills = db.get_active_skills().unwrap_or_default();
    drop(db);

    for skill in skills {
        if let Some((from, to, message)) =
            build_lore_conflict_hint(paragraph, &skill.category, &skill.skill)
        {
            return Some(EditorSemanticLint {
                request_id: payload.request_id.clone(),
                cursor_position: payload.cursor_position,
                from: payload.paragraph_from + from,
                to: payload.paragraph_from + to,
                message: format!("{}：{}", chapter_label, message),
                severity: "warning".to_string(),
            });
        }
    }

    None
}

fn build_lore_conflict_hint(
    paragraph: &str,
    lore_keyword: &str,
    lore_content: &str,
) -> Option<(usize, usize, String)> {
    let keyword_present = !lore_keyword.trim().is_empty() && paragraph.contains(lore_keyword);
    if !keyword_present {
        return None;
    }

    let content = lore_content.to_lowercase();
    let weapon_conflicts: [(&str, &[&str]); 3] = [
        ("剑", &["刀", "弯刀", "短刀", "长刀", "匕首"]),
        ("长剑", &["刀", "弯刀", "短刀", "长刀", "匕首"]),
        ("枪", &["刀", "剑", "弓"]),
    ];

    for (draft_term, lore_terms) in weapon_conflicts {
        if !paragraph.contains(draft_term) {
            continue;
        }

        if let Some(preferred) = lore_terms.iter().find(|term| content.contains(*term)) {
            let (start, end) = find_char_range(paragraph, draft_term)?;
            return Some((
                start,
                end,
                format!(
                    "设定冲突：{} 的设定更接近使用{}，这里写成{}可能需要确认。",
                    lore_keyword, preferred, draft_term
                ),
            ));
        }
    }

    let contradiction_markers = ["不会", "不擅长", "不能", "从不", "禁止", "忌用"];
    for marker in contradiction_markers {
        let Some(marker_byte) = lore_content.find(marker) else {
            continue;
        };
        let after_marker = &lore_content[marker_byte + marker.len()..];
        let term: String = after_marker
            .chars()
            .skip_while(|c| c.is_whitespace() || *c == '用' || *c == '使')
            .take_while(|c| c.is_alphanumeric() || ('\u{4e00}'..='\u{9fff}').contains(c))
            .take(4)
            .collect();

        if term.chars().count() >= 1 && paragraph.contains(&term) {
            let (start, end) = find_char_range(paragraph, &term)?;
            return Some((
                start,
                end,
                format!(
                    "设定冲突：{} 的设定里提到“{}{}”。",
                    lore_keyword, marker, term
                ),
            ));
        }
    }

    None
}

fn find_writer_agent_diagnostic_lint(
    app: &tauri::AppHandle,
    payload: &SemanticLintPayload,
    chapter_label: &str,
) -> Option<EditorSemanticLint> {
    let state = app.state::<AppState>();
    let kernel = state.writer_kernel.lock().ok()?;
    let diagnostics = kernel.diagnose_paragraph(
        &payload.paragraph,
        payload.paragraph_from,
        payload.chapter_title.as_deref().unwrap_or("Chapter-1"),
    );
    drop(kernel);

    let diagnostic = diagnostics.into_iter().next()?;
    let severity = match diagnostic.severity {
        writer_agent::diagnostics::DiagnosticSeverity::Error => "error",
        writer_agent::diagnostics::DiagnosticSeverity::Warning => "warning",
        writer_agent::diagnostics::DiagnosticSeverity::Info => "info",
    };

    Some(EditorSemanticLint {
        request_id: payload.request_id.clone(),
        cursor_position: payload.cursor_position,
        from: diagnostic.from,
        to: diagnostic.to.max(diagnostic.from + 1),
        message: format!("{}：{}", chapter_label, diagnostic.message),
        severity: severity.to_string(),
    })
}

fn byte_to_char_index(text: &str, byte_index: usize) -> usize {
    text[..byte_index.min(text.len())].chars().count()
}

fn find_char_range(text: &str, needle: &str) -> Option<(usize, usize)> {
    let start_byte = text.find(needle)?;
    let start = byte_to_char_index(text, start_byte);
    let end = start + needle.chars().count();
    Some((start, end))
}
