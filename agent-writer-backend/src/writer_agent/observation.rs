//! WriterObservation — the agent's sensory input from the editor.
//! Replaces ad-hoc payloads with a single typed observation struct.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WriterObservation {
    pub id: String,
    pub created_at: u64,
    pub source: ObservationSource,
    pub reason: ObservationReason,
    pub project_id: String,
    pub chapter_title: Option<String>,
    pub chapter_revision: Option<String>,
    pub cursor: Option<TextRange>,
    pub selection: Option<TextSelection>,
    pub prefix: String,
    pub suffix: String,
    pub paragraph: String,
    #[serde(rename = "fullTextDigest")]
    pub full_text_digest: Option<String>,
    #[serde(rename = "editorDirty")]
    pub editor_dirty: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ObservationSource {
    Editor,
    Outline,
    Lorebook,
    #[serde(rename = "chapter_save")]
    ChapterSave,
    #[serde(rename = "manual_request")]
    ManualRequest,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ObservationReason {
    Typed,
    Idle,
    Selection,
    #[serde(rename = "chapter_switch")]
    ChapterSwitch,
    Save,
    Explicit,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TextRange {
    pub from: usize,
    pub to: usize,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextSelection {
    pub from: usize,
    pub to: usize,
    pub text: String,
}

impl WriterObservation {
    pub fn has_selection(&self) -> bool {
        self.selection
            .as_ref()
            .map(|s| s.from < s.to)
            .unwrap_or(false)
    }

    pub fn selected_text(&self) -> &str {
        self.selection
            .as_ref()
            .map(|s| s.text.as_str())
            .unwrap_or("")
    }
}
