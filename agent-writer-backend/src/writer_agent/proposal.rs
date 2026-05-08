//! AgentProposal — typed, evidenced, feedback-tracked suggestions.
//! Replaces vague suggestion cards with structured agent output.

use super::observation::TextRange;
use super::operation::WriterOperation;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentProposal {
    pub id: String,
    #[serde(rename = "observationId")]
    pub observation_id: String,
    pub kind: ProposalKind,
    pub priority: ProposalPriority,
    pub target: Option<TextRange>,
    pub preview: String,
    pub operations: Vec<WriterOperation>,
    pub rationale: String,
    pub evidence: Vec<EvidenceRef>,
    pub risks: Vec<String>,
    #[serde(default)]
    pub alternatives: Vec<ProposalAlternative>,
    pub confidence: f64,
    #[serde(rename = "expiresAt")]
    pub expires_at: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProposalAlternative {
    pub id: String,
    pub label: String,
    pub preview: String,
    pub operation: Option<WriterOperation>,
    pub rationale: String,
    #[serde(default)]
    pub evidence: Vec<EvidenceRef>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ProposalKind {
    Ghost,
    #[serde(rename = "parallel_draft")]
    ParallelDraft,
    #[serde(rename = "continuity_warning")]
    ContinuityWarning,
    #[serde(rename = "canon_update")]
    CanonUpdate,
    #[serde(rename = "style_note")]
    StyleNote,
    #[serde(rename = "plot_promise")]
    PlotPromise,
    #[serde(rename = "chapter_structure")]
    ChapterStructure,
    #[serde(rename = "story_contract")]
    StoryContract,
    #[serde(rename = "chapter_mission")]
    ChapterMission,
    Question,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ProposalPriority {
    Ambient,
    Normal,
    Urgent,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceRef {
    pub source: EvidenceSource,
    pub reference: String,
    pub snippet: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceSource {
    Lorebook,
    Outline,
    #[serde(rename = "chapter_text")]
    ChapterText,
    Canon,
    #[serde(rename = "style_ledger")]
    StyleLedger,
    #[serde(rename = "promise_ledger")]
    PromiseLedger,
    #[serde(rename = "story_contract")]
    StoryContract,
    #[serde(rename = "chapter_mission")]
    ChapterMission,
    #[serde(rename = "author_feedback")]
    AuthorFeedback,
    #[serde(rename = "story_impact_radius")]
    StoryImpactRadius,
}
