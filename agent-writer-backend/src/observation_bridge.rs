use serde::Deserialize;

use crate::{agent_runtime, storage, writer_agent};

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EditorStatePayload {
    pub(crate) request_id: String,
    pub(crate) prefix: String,
    pub(crate) suffix: String,
    pub(crate) cursor_position: usize,
    pub(crate) text_cursor_position: Option<usize>,
    pub(crate) paragraph: String,
    pub(crate) chapter_title: Option<String>,
    pub(crate) chapter_revision: Option<String>,
    pub(crate) editor_dirty: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AskAgentContext {
    pub(crate) chapter_title: Option<String>,
    pub(crate) chapter_revision: Option<String>,
    pub(crate) cursor_position: Option<usize>,
    pub(crate) dirty: Option<bool>,
    pub(crate) mode: Option<AskAgentMode>,
    pub(crate) request_id: Option<String>,
    pub(crate) provider_budget_approval:
        Option<writer_agent::provider_budget::WriterProviderBudgetApproval>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AskAgentMode {
    Chat,
    InlineOperation,
}

pub(crate) fn to_writer_observation(
    observation: &agent_runtime::AgentObservation,
    project_id: &str,
) -> writer_agent::observation::WriterObservation {
    let reason = match observation.reason {
        agent_runtime::AgentObservationReason::UserTyped => {
            if observation.idle_ms >= 900 {
                writer_agent::observation::ObservationReason::Idle
            } else {
                writer_agent::observation::ObservationReason::Typed
            }
        }
        agent_runtime::AgentObservationReason::SelectionChange => {
            writer_agent::observation::ObservationReason::Selection
        }
        agent_runtime::AgentObservationReason::ChapterSwitch => {
            writer_agent::observation::ObservationReason::ChapterSwitch
        }
        agent_runtime::AgentObservationReason::IdleTick => {
            writer_agent::observation::ObservationReason::Idle
        }
    };

    writer_agent::observation::WriterObservation {
        id: observation.id.clone(),
        created_at: observation.created_at,
        source: writer_agent::observation::ObservationSource::Editor,
        reason,
        project_id: project_id.to_string(),
        chapter_title: observation.chapter_title.clone(),
        chapter_revision: observation.chapter_revision.clone(),
        cursor: Some(writer_agent::observation::TextRange {
            from: observation.cursor_position,
            to: observation.cursor_position,
        }),
        selection: observation.selection.as_ref().map(|selection| {
            writer_agent::observation::TextSelection {
                from: selection.from,
                to: selection.to,
                text: selection.text.clone(),
            }
        }),
        prefix: observation.nearby_text.clone(),
        suffix: String::new(),
        paragraph: observation.current_paragraph.clone(),
        full_text_digest: None,
        editor_dirty: observation.dirty,
    }
}

pub(crate) fn build_writer_observation_from_editor_state(
    payload: &EditorStatePayload,
    project_id: &str,
) -> writer_agent::observation::WriterObservation {
    let cursor = payload
        .text_cursor_position
        .unwrap_or_else(|| payload.prefix.chars().count());
    let paragraph = if payload.paragraph.trim().is_empty() {
        paragraph_hint(&payload.prefix)
    } else {
        payload.paragraph.clone()
    };

    writer_agent::observation::WriterObservation {
        id: format!("fim-{}", payload.request_id),
        created_at: agent_runtime::now_ms(),
        source: writer_agent::observation::ObservationSource::Editor,
        reason: writer_agent::observation::ObservationReason::Idle,
        project_id: project_id.to_string(),
        chapter_title: payload.chapter_title.clone(),
        chapter_revision: payload.chapter_revision.clone(),
        cursor: Some(writer_agent::observation::TextRange {
            from: cursor,
            to: cursor,
        }),
        selection: None,
        prefix: payload.prefix.clone(),
        suffix: payload.suffix.clone(),
        paragraph,
        full_text_digest: Some(storage::content_revision(&format!(
            "{}{}",
            payload.prefix, payload.suffix
        ))),
        editor_dirty: payload.editor_dirty.unwrap_or(true),
    }
}

#[cfg(test)]
pub(crate) fn test_editor_state_payload(
    prefix: &str,
    suffix: &str,
    paragraph: &str,
    cursor_position: usize,
    text_cursor_position: Option<usize>,
) -> EditorStatePayload {
    EditorStatePayload {
        request_id: "test-request".to_string(),
        prefix: prefix.to_string(),
        suffix: suffix.to_string(),
        cursor_position,
        text_cursor_position,
        paragraph: paragraph.to_string(),
        chapter_title: Some("Chapter-1".to_string()),
        chapter_revision: Some("rev-1".to_string()),
        editor_dirty: Some(true),
    }
}

pub(crate) fn split_context_for_cursor(
    context: &str,
    cursor_position: usize,
    prefix_chars: usize,
    suffix_chars: usize,
) -> (String, String) {
    let cursor_position = cursor_position.min(context.chars().count());
    let prefix: String = context.chars().take(cursor_position).collect();
    let suffix: String = context
        .chars()
        .skip(cursor_position)
        .take(suffix_chars)
        .collect();
    (crate::char_tail(&prefix, prefix_chars), suffix)
}

pub(crate) fn build_manual_writer_observation(
    message: &str,
    context: &str,
    paragraph: &str,
    selected_text: &str,
    payload: Option<&AskAgentContext>,
    project_id: &str,
) -> writer_agent::observation::WriterObservation {
    let cursor_position = payload
        .and_then(|payload| payload.cursor_position)
        .unwrap_or_else(|| context.chars().count());
    let chapter_title = payload
        .and_then(|payload| payload.chapter_title.clone())
        .filter(|title| !title.trim().is_empty())
        .or_else(|| Some("manual".to_string()));
    let chapter_revision = payload
        .and_then(|payload| payload.chapter_revision.clone())
        .filter(|revision| !revision.trim().is_empty())
        .or_else(|| Some(storage::content_revision(context)));
    let paragraph = if paragraph.trim().is_empty() {
        if selected_text.trim().is_empty() {
            message.to_string()
        } else {
            selected_text.to_string()
        }
    } else {
        paragraph.to_string()
    };
    let (prefix, suffix) = split_context_for_cursor(context, cursor_position, 3_000, 1_000);

    writer_agent::observation::WriterObservation {
        id: format!("manual-{}", agent_runtime::now_ms()),
        created_at: agent_runtime::now_ms(),
        source: writer_agent::observation::ObservationSource::ManualRequest,
        reason: writer_agent::observation::ObservationReason::Explicit,
        project_id: project_id.to_string(),
        chapter_title,
        chapter_revision,
        cursor: Some(writer_agent::observation::TextRange {
            from: cursor_position,
            to: cursor_position,
        }),
        selection: selected_text_range(context, selected_text),
        prefix,
        suffix,
        paragraph,
        full_text_digest: Some(storage::content_revision(context)),
        editor_dirty: payload.and_then(|payload| payload.dirty).unwrap_or(false),
    }
}

fn paragraph_hint(paragraph: &str) -> String {
    let trimmed = paragraph.trim();
    if trimmed.is_empty() {
        String::new()
    } else {
        format!("\nCurrent paragraph:\n{}\n", trimmed)
    }
}

fn selected_text_range(
    context: &str,
    selected_text: &str,
) -> Option<writer_agent::observation::TextSelection> {
    let selected = selected_text.trim();
    if selected.is_empty() {
        return None;
    }
    let (from, to) = find_char_range(context, selected).unwrap_or((0, selected.chars().count()));
    Some(writer_agent::observation::TextSelection {
        from,
        to,
        text: selected.to_string(),
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
