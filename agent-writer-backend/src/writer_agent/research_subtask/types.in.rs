#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WriterSubtaskKind {
    Research,
    Diagnostic,
    Drafting,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WriterSubtaskWorkspace {
    pub subtask_id: String,
    pub kind: WriterSubtaskKind,
    pub workspace_dir: String,
    pub artifact_dir: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WriterSubtaskResult {
    pub subtask_id: String,
    pub kind: WriterSubtaskKind,
    pub objective: String,
    pub summary: String,
    pub evidence_refs: Vec<EvidenceRef>,
    pub artifact_refs: Vec<String>,
    pub blocked_operation_kinds: Vec<String>,
    pub created_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WriterSubtaskRunEventPayload {
    pub subtask_id: String,
    pub kind: WriterSubtaskKind,
    pub status: String,
    pub objective: String,
    pub summary: String,
    pub evidence_count: usize,
    pub artifact_count: usize,
    pub blocked_operation_count: usize,
    pub evidence_refs: Vec<String>,
    pub artifact_refs: Vec<String>,
    pub blocked_operation_kinds: Vec<String>,
    pub tool_policy: WriterSubtaskToolPolicySummary,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WriterSubtaskToolPolicySummary {
    pub max_side_effect_level: String,
    pub allow_approval_required: bool,
    pub required_tool_tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WriterSubtaskProviderBudgetInput {
    pub subtask_id: String,
    pub kind: WriterSubtaskKind,
    pub model: String,
    pub objective: String,
    pub query: String,
    pub context_chars: usize,
    pub requested_output_tokens: u64,
}
