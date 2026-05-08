use agent_harness_core::context_quality::ContextQualityReport;
use agent_harness_core::provider::LlmMessage;
use agent_harness_core::{
    FeedbackContract, Intent, RequiredContext, TaskBelief, TaskPacket, TaskScope,
    ToolPolicyContract, ToolSideEffectLevel, VectorDB,
};
use crate::writer_agent::input_governance::CompiledInput;

use crate::writer_agent::context_relevance::{format_text_chunk_relevance, rerank_text_chunks};
use crate::writer_agent::provider_budget::{
    apply_provider_budget_approval, evaluate_provider_budget, WriterProviderBudgetApproval,
    WriterProviderBudgetDecision, WriterProviderBudgetReport, WriterProviderBudgetRequest,
    WriterProviderBudgetTask,
};
use crate::writer_agent::task_receipt::{
    WriterFailureCategory, WriterFailureEvidenceBundle, WriterTaskReceipt,
};
use crate::{llm_runtime, storage};

pub trait ChapterGenerationProject {
    fn project_id(&self) -> &str;
    fn project_data_dir(&self) -> &std::path::Path;
    fn memory_path(&self) -> &std::path::Path;
    fn brain_path(&self) -> &std::path::Path;
    fn load_outline(&self) -> Result<Vec<storage::OutlineNode>, String>;
    fn save_outline(&self, nodes: &[storage::OutlineNode]) -> Result<(), String>;
    fn load_lorebook(&self) -> Result<Vec<storage::LoreEntry>, String>;
    fn load_chapter(&self, title: &str) -> Result<String, String>;
    fn chapter_revision(&self, title: &str) -> Result<String, String>;
    fn save_chapter_content_and_revision(
        &self,
        title: &str,
        content: &str,
    ) -> Result<String, String>;
    fn open_memory_db(&self) -> Option<rusqlite::Connection> {
        None // default: no DB access
    }
}

pub const PHASE_STARTED: &str = "chapter_generation_started";
pub const PHASE_CONTEXT_BUILT: &str = "chapter_generation_context_built";
pub const PHASE_SCENE_PLAN: &str = "chapter_generation_scene_plan";
pub const PHASE_CONTINUATION: &str = "chapter_generation_continuation";
pub const PHASE_COMPRESS: &str = "chapter_generation_compress";
pub const PHASE_LENGTH_VALIDATE: &str = "chapter_generation_length_validate";
pub const PHASE_PROGRESS: &str = "chapter_generation_progress";
pub const PHASE_CONFLICT: &str = "chapter_generation_conflict";
pub const PHASE_COMPLETED: &str = "chapter_generation_completed";
pub const PHASE_FAILED: &str = "chapter_generation_failed";

pub const PHASE_PREFLIGHT: &str = "preflight";
pub const PHASE_SEGMENT_DRAFT: &str = "segment_draft";
pub const PHASE_MERGE: &str = "merge";
pub const PHASE_POLISH: &str = "polish";
pub const PHASE_SAVE: &str = "save";

const DEFAULT_TOTAL_CONTEXT_CHARS: usize = 24_000;
const DEFAULT_INSTRUCTION_CHARS: usize = 1_000;
const DEFAULT_OUTLINE_CHARS: usize = 6_000;
const DEFAULT_PREVIOUS_CHAPTERS_CHARS: usize = 5_000;
const DEFAULT_NEXT_CHAPTER_CHARS: usize = 2_000;
const DEFAULT_TARGET_EXISTING_CHARS: usize = 3_000;
const DEFAULT_LOREBOOK_CHARS: usize = 5_000;
const DEFAULT_USER_PROFILE_CHARS: usize = 4_000;
const DEFAULT_RAG_CHARS: usize = 4_000;
const DEFAULT_PREVIOUS_CHAPTER_COUNT: usize = 2;
const DEFAULT_NEXT_CHAPTER_COUNT: usize = 1;
const DEFAULT_LOREBOOK_ENTRY_COUNT: usize = 4;
const DEFAULT_USER_PROFILE_ENTRY_COUNT: usize = 6;
const DEFAULT_RAG_CHUNK_COUNT: usize = 5;
const DEFAULT_TARGET_CHARS: usize = 3_500;
const DEFAULT_MIN_CHARS: usize = 3_000;
const DEFAULT_MAX_CHARS: usize = 4_000;
const DEFAULT_SAVE_HARD_FLOOR_CHARS: usize = 2_800;
const DEFAULT_SAVE_HARD_CEILING_CHARS: usize = 4_300;
const DEFAULT_OUTPUT_HARD_CAP_CHARS: usize = 30_000;
const PROVIDER_TIMEOUT_SECS: u64 = 120;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateChapterAutonomousPayload {
    #[serde(default)]
    pub request_id: Option<String>,
    #[serde(default)]
    pub target_chapter_title: Option<String>,
    #[serde(default)]
    pub target_chapter_number: Option<usize>,
    pub user_instruction: String,
    #[serde(default)]
    pub budget: Option<ChapterContextBudget>,
    #[serde(default)]
    pub frontend_state: Option<FrontendChapterStateSnapshot>,
    #[serde(default)]
    pub save_mode: SaveMode,
    #[serde(default)]
    pub chapter_summary_override: Option<String>,
    #[serde(default)]
    pub chapter_contract: Option<ChapterContract>,
    #[serde(default)]
    pub provider_budget_approval: Option<WriterProviderBudgetApproval>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FrontendChapterStateSnapshot {
    #[serde(default)]
    pub open_chapter_title: Option<String>,
    #[serde(default)]
    pub open_chapter_revision: Option<String>,
    #[serde(default)]
    pub dirty: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SaveMode {
    CreateIfMissing,
    #[default]
    ReplaceIfClean,
    SaveAsDraft,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ChapterContract {
    #[serde(default = "default_target_chars")]
    pub target_chars: usize,
    #[serde(default = "default_min_chars")]
    pub min_chars: usize,
    #[serde(default = "default_max_chars")]
    pub max_chars: usize,
    #[serde(default = "default_save_hard_floor_chars")]
    pub save_hard_floor_chars: usize,
    #[serde(default = "default_save_hard_ceiling_chars")]
    pub save_hard_ceiling_chars: usize,
}

impl Default for ChapterContract {
    fn default() -> Self {
        Self {
            target_chars: DEFAULT_TARGET_CHARS,
            min_chars: DEFAULT_MIN_CHARS,
            max_chars: DEFAULT_MAX_CHARS,
            save_hard_floor_chars: DEFAULT_SAVE_HARD_FLOOR_CHARS,
            save_hard_ceiling_chars: DEFAULT_SAVE_HARD_CEILING_CHARS,
        }
    }
}

impl ChapterContract {
    pub fn validate(self) -> Result<Self, ChapterGenerationError> {
        if self.target_chars == 0
            || self.min_chars == 0
            || self.max_chars == 0
            || self.save_hard_floor_chars == 0
            || self.save_hard_ceiling_chars == 0
        {
            return Err(ChapterGenerationError::new(
                "CHAPTER_CONTRACT_INVALID",
                "Chapter contract character limits must be positive.",
                true,
            ));
        }
        if self.min_chars > self.target_chars
            || self.target_chars > self.max_chars
            || self.save_hard_floor_chars > self.min_chars
            || self.max_chars > self.save_hard_ceiling_chars
        {
            return Err(ChapterGenerationError::new(
                "CHAPTER_CONTRACT_INVALID",
                "Chapter contract bounds are inconsistent.",
                true,
            ));
        }
        Ok(self)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChapterContextBudget {
    #[serde(default = "default_total_context_chars")]
    pub total_chars: usize,
    #[serde(default = "default_instruction_chars")]
    pub instruction_chars: usize,
    #[serde(default = "default_outline_chars")]
    pub outline_chars: usize,
    #[serde(default = "default_previous_chapters_chars")]
    pub previous_chapters_chars: usize,
    #[serde(default = "default_next_chapter_chars")]
    pub next_chapter_chars: usize,
    #[serde(default = "default_target_existing_chars")]
    pub target_existing_chars: usize,
    #[serde(default = "default_lorebook_chars")]
    pub lorebook_chars: usize,
    #[serde(default = "default_user_profile_chars")]
    pub user_profile_chars: usize,
    #[serde(default = "default_rag_chars")]
    pub rag_chars: usize,
    #[serde(default = "default_previous_chapter_count")]
    pub previous_chapter_count: usize,
    #[serde(default = "default_next_chapter_count")]
    pub next_chapter_count: usize,
    #[serde(default = "default_lorebook_entry_count")]
    pub lorebook_entry_count: usize,
    #[serde(default = "default_user_profile_entry_count")]
    pub user_profile_entry_count: usize,
    #[serde(default = "default_rag_chunk_count")]
    pub rag_chunk_count: usize,
}

impl Default for ChapterContextBudget {
    fn default() -> Self {
        Self {
            total_chars: DEFAULT_TOTAL_CONTEXT_CHARS,
            instruction_chars: DEFAULT_INSTRUCTION_CHARS,
            outline_chars: DEFAULT_OUTLINE_CHARS,
            previous_chapters_chars: DEFAULT_PREVIOUS_CHAPTERS_CHARS,
            next_chapter_chars: DEFAULT_NEXT_CHAPTER_CHARS,
            target_existing_chars: DEFAULT_TARGET_EXISTING_CHARS,
            lorebook_chars: DEFAULT_LOREBOOK_CHARS,
            user_profile_chars: DEFAULT_USER_PROFILE_CHARS,
            rag_chars: DEFAULT_RAG_CHARS,
            previous_chapter_count: DEFAULT_PREVIOUS_CHAPTER_COUNT,
            next_chapter_count: DEFAULT_NEXT_CHAPTER_COUNT,
            lorebook_entry_count: DEFAULT_LOREBOOK_ENTRY_COUNT,
            user_profile_entry_count: DEFAULT_USER_PROFILE_ENTRY_COUNT,
            rag_chunk_count: DEFAULT_RAG_CHUNK_COUNT,
        }
    }
}

fn default_total_context_chars() -> usize {
    DEFAULT_TOTAL_CONTEXT_CHARS
}

fn default_instruction_chars() -> usize {
    DEFAULT_INSTRUCTION_CHARS
}

fn default_outline_chars() -> usize {
    DEFAULT_OUTLINE_CHARS
}

fn default_previous_chapters_chars() -> usize {
    DEFAULT_PREVIOUS_CHAPTERS_CHARS
}

fn default_next_chapter_chars() -> usize {
    DEFAULT_NEXT_CHAPTER_CHARS
}

fn default_target_existing_chars() -> usize {
    DEFAULT_TARGET_EXISTING_CHARS
}

fn default_lorebook_chars() -> usize {
    DEFAULT_LOREBOOK_CHARS
}

fn default_user_profile_chars() -> usize {
    DEFAULT_USER_PROFILE_CHARS
}

fn default_rag_chars() -> usize {
    DEFAULT_RAG_CHARS
}

fn default_previous_chapter_count() -> usize {
    DEFAULT_PREVIOUS_CHAPTER_COUNT
}

fn default_next_chapter_count() -> usize {
    DEFAULT_NEXT_CHAPTER_COUNT
}

fn default_lorebook_entry_count() -> usize {
    DEFAULT_LOREBOOK_ENTRY_COUNT
}

fn default_user_profile_entry_count() -> usize {
    DEFAULT_USER_PROFILE_ENTRY_COUNT
}

fn default_rag_chunk_count() -> usize {
    DEFAULT_RAG_CHUNK_COUNT
}

fn default_target_chars() -> usize {
    DEFAULT_TARGET_CHARS
}

fn default_min_chars() -> usize {
    DEFAULT_MIN_CHARS
}

fn default_max_chars() -> usize {
    DEFAULT_MAX_CHARS
}

fn default_save_hard_floor_chars() -> usize {
    DEFAULT_SAVE_HARD_FLOOR_CHARS
}

fn default_save_hard_ceiling_chars() -> usize {
    DEFAULT_SAVE_HARD_CEILING_CHARS
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChapterTarget {
    pub title: String,
    pub filename: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub number: Option<usize>,
    pub summary: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChapterContextSource {
    pub source_type: String,
    pub id: String,
    pub label: String,
    pub original_chars: usize,
    pub included_chars: usize,
    pub truncated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChapterContextBudgetReport {
    pub max_chars: usize,
    pub included_chars: usize,
    pub source_count: usize,
    pub truncated_source_count: usize,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ChapterIntentArtifact {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chapter_number: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chapter_title: Option<String>,
    pub goal: String,
    pub must_keep: Vec<String>,
    pub must_avoid: Vec<String>,
    pub style_emphasis: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ChapterSelectedEvidenceArtifact {
    pub source: String,
    pub reason: String,
    pub excerpt: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ChapterRuleStackArtifact {
    pub hard: Vec<String>,
    pub soft: Vec<String>,
    pub diagnostic: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ChapterTraceArtifact {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chapter_number: Option<usize>,
    pub planner_inputs: Vec<String>,
    pub selected_evidence_count: usize,
    pub active_override_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ChapterResultDelta {
    pub summary: String,
    pub state_changes: Vec<String>,
    pub character_progress: Vec<String>,
    pub new_conflicts: Vec<String>,
    pub new_clues: Vec<String>,
    pub promise_updates: Vec<String>,
    pub canon_updates: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ReaderTakeaway {
    pub emotional_beat: String,
    pub expectation: String,
    pub unresolved_lack: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ChapterSettlementEvidence {
    pub excerpt: String,
    pub signal: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ChapterResultExtractionCandidate {
    pub field: String,
    pub value: String,
    pub confidence: f32,
    pub evidence: Vec<ChapterSettlementEvidence>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ChapterPromiseExtractionCandidate {
    pub action: String,
    pub kind: String,
    pub title: String,
    pub description: String,
    pub confidence: f32,
    pub expected_payoff: String,
    pub evidence: Vec<ChapterSettlementEvidence>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ChapterBookStateExtractionCandidate {
    pub bucket: String,
    pub value: String,
    pub confidence: f32,
    pub evidence: Vec<ChapterSettlementEvidence>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ChapterSettlementExtraction {
    pub summary_candidates: Vec<ChapterResultExtractionCandidate>,
    pub chapter_result_candidates: Vec<ChapterResultExtractionCandidate>,
    pub promise_candidates: Vec<ChapterPromiseExtractionCandidate>,
    pub book_state_candidates: Vec<ChapterBookStateExtractionCandidate>,
    pub character_state_deltas: Vec<CharacterStateDeltaEntry>,
    pub relationship_deltas: Vec<RelationshipDeltaEntry>,
    #[serde(default)]
    pub knowledge_deltas: Vec<KnowledgeDeltaEntry>,
    #[serde(default)]
    pub identity_deltas: Vec<IdentityDeltaEntry>,
    #[serde(default)]
    pub scene_deltas: Vec<SceneResultProjection>,
    pub warnings: Vec<String>,
    #[serde(default)]
    pub emotional_debt_cues: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ChapterPromiseDeltaAction {
    #[default]
    Introduced,
    Advanced,
    Resolved,
    Deferred,
    Abandoned,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ChapterPromiseDeltaEntry {
    #[serde(default)]
    pub action: ChapterPromiseDeltaAction,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub promise_id: Option<i64>,
    pub kind: String,
    pub title: String,
    pub description: String,
    pub chapter: String,
    pub source_ref: String,
    pub expected_payoff: String,
    pub priority: i32,
    pub related_entities: Vec<String>,
    #[serde(default)]
    pub core: bool,
    #[serde(default)]
    pub promoted: bool,
    #[serde(default)]
    pub blocked_reason: String,
    #[serde(default)]
    pub evidence: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ChapterBookStateDeltaBucket {
    LongTermConstraint,
    MegaPromise,
    #[default]
    IrreversibleChange,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ChapterBookStateDeltaEntry {
    #[serde(default)]
    pub bucket: ChapterBookStateDeltaBucket,
    pub value: String,
    pub source_ref: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ChapterArcDeltaEntry {
    pub scope: String,
    pub value: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CharacterStateDeltaEntry {
    pub character_name: String,
    pub chapter_title: String,
    pub action: String,
    #[serde(default)]
    pub core_commitments: Vec<String>,
    #[serde(default)]
    pub goal_state: serde_json::Value,
    pub source_ref: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RelationshipDeltaEntry {
    pub character_a_name: String,
    pub character_b_name: String,
    pub action: String,
    pub relation_type: String,
    pub visibility: String,
    pub chapter_title: String,
    pub source_ref: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeDeltaEntry {
    pub topic: String,
    pub truth_state: String,
    pub holder_type: String,
    pub holder_id: i64,
    pub knowledge_mode: String,
    pub chapter_title: String,
    pub source_ref: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SceneResultProjection {
    pub scene_id: i64,
    pub outcome: String,
    pub consequence: String,
    pub source_ref: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct IdentityDeltaEntry {
    pub character_name: String,
    pub public_identity: String,
    pub private_identity: String,
    pub revealed_to: Vec<String>,
    pub chapter_title: String,
    pub source_ref: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ChapterSettlementDelta {
    pub chapter_title: String,
    pub chapter_revision: String,
    pub summary: String,
    #[serde(default)]
    pub extraction: ChapterSettlementExtraction,
    #[serde(default)]
    pub chapter_result: ChapterResultDelta,
    #[serde(default)]
    pub promise_updates: Vec<ChapterPromiseDeltaEntry>,
    #[serde(default)]
    pub arc_updates: Vec<ChapterArcDeltaEntry>,
    #[serde(default)]
    pub book_state_updates: Vec<ChapterBookStateDeltaEntry>,
    pub chapter_fact_delta: Vec<String>,
    pub promise_delta: Vec<String>,
    pub arc_delta: Vec<String>,
    pub book_state_delta: Vec<String>,
    #[serde(default)]
    pub character_state_deltas: Vec<CharacterStateDeltaEntry>,
    #[serde(default)]
    pub relationship_deltas: Vec<RelationshipDeltaEntry>,
    #[serde(default)]
    pub knowledge_deltas: Vec<KnowledgeDeltaEntry>,
    #[serde(default)]
    pub identity_deltas: Vec<IdentityDeltaEntry>,
    #[serde(default)]
    pub scene_deltas: Vec<SceneResultProjection>,
    pub continuity_issues: Vec<String>,
    pub repairable: bool,
    #[serde(default)]
    pub reader_takeaway: Option<ReaderTakeaway>,
    #[serde(default)]
    pub emotional_debt_cues: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SettlementReplay {
    pub input_content_hash: String,
    pub memory_snapshot_id: String,
    pub output_delta_hash: String,
    pub created_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettlementReplayResult {
    pub replayed: bool,
    pub matches_original: bool,
    pub mismatches: Vec<String>,
    pub original_hash: String,
    pub replayed_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ChapterSettlementApplyResult {
    pub applied: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chapter_result_snapshot_id: Option<i64>,
    pub promise_created: usize,
    pub promise_advanced: usize,
    pub promise_resolved: usize,
    pub promise_deferred: usize,
    pub promise_abandoned: usize,
    pub book_state_updated: bool,
    pub character_state_applied: usize,
    pub relationship_applied: usize,
    pub knowledge_applied: usize,
    pub identity_applied: usize,
    pub scene_applied: usize,
    pub fact_applied: usize,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LengthPhaseTelemetry {
    pub continuation_count: usize,
    pub compress_count: usize,
    pub hard_compress_count: usize,
    pub continuation_latency_ms: u64,
    pub compress_latency_ms: u64,
    pub hard_compress_latency_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ChapterLengthTelemetry {
    pub target_chars: usize,
    pub min_chars: usize,
    pub max_chars: usize,
    pub save_hard_floor_chars: usize,
    pub save_hard_ceiling_chars: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub draft_chars: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub final_chars: Option<usize>,
    pub continuation_applied: bool,
    pub compress_applied: bool,
    pub hard_compress_applied: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
    #[serde(default)]
    pub phase_telemetry: LengthPhaseTelemetry,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ScenePlanEntry {
    pub name: String,
    pub objective: String,
    pub participants: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuiltChapterContext {
    pub request_id: String,
    pub target: ChapterTarget,
    pub base_revision: String,
    pub chapter_contract: ChapterContract,
    pub prompt_context: String,
    pub sources: Vec<ChapterContextSource>,
    pub budget: ChapterContextBudgetReport,
    pub warnings: Vec<String>,
    pub receipt: WriterTaskReceipt,
    pub intent_artifact: ChapterIntentArtifact,
    pub selected_evidence: Vec<ChapterSelectedEvidenceArtifact>,
    pub rule_stack: ChapterRuleStackArtifact,
    pub trace_artifact: ChapterTraceArtifact,
    #[serde(default)]
    pub scene_plan: Vec<ScenePlanEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub craft_plan: Option<SceneCraftPlan>,
    pub compiled_input: Option<CompiledInput>,
    pub stable_prefix_chars: usize,
    pub dynamic_tail_chars: usize,
    pub focus_pack_rebuild_count: usize,
    #[serde(default)]
    pub previous_fulltext_upgrade_count: usize,
    #[serde(default)]
    pub previous_fulltext_upgrade_reason: String,
    #[serde(default)]
    pub impact_scoped: bool,
    #[serde(default)]
    pub impact_filtered_count: usize,
    #[serde(default)]
    pub impact_truncated: bool,
    #[serde(default)]
    pub generation_strategy: GenerationStrategy,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_quality: Option<ContextQualityReport>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub craft_rule_stats: Option<std::collections::HashMap<String, crate::writer_agent::memory::CraftRuleStats>>,
}

#[derive(Debug, Clone)]
pub struct BuildChapterContextInput {
    pub request_id: String,
    pub target_chapter_title: Option<String>,
    pub target_chapter_number: Option<usize>,
    pub user_instruction: String,
    pub budget: ChapterContextBudget,
    pub chapter_contract: ChapterContract,
    pub chapter_summary_override: Option<String>,
    pub user_profile_entries: Vec<String>,
    pub compiled_input: Option<CompiledInput>,
    pub open_promise_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChapterGenerationError {
    pub code: String,
    pub message: String,
    pub recoverable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evidence: Option<Box<WriterFailureEvidenceBundle>>,
}

impl ChapterGenerationError {
    pub fn new(code: &str, message: impl Into<String>, recoverable: bool) -> Self {
        Self {
            code: code.to_string(),
            message: message.into(),
            recoverable,
            details: None,
            evidence: None,
        }
    }

    pub fn with_details(
        code: &str,
        message: impl Into<String>,
        recoverable: bool,
        details: impl Into<String>,
    ) -> Self {
        Self {
            code: code.to_string(),
            message: message.into(),
            recoverable,
            details: Some(details.into()),
            evidence: None,
        }
    }

    pub fn with_evidence(mut self, evidence: Box<WriterFailureEvidenceBundle>) -> Self {
        self.evidence = Some(evidence);
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateChapterDraftOutput {
    #[serde(skip_serializing)]
    pub content: String,
    pub finish_reason: String,
    pub model: String,
    pub provider: String,
    pub output_chars: usize,
    pub chapter_contract: ChapterContract,
    pub base_revision: String,
    pub provider_budget: WriterProviderBudgetReport,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChapterDraftRepairOutput {
    #[serde(skip_serializing)]
    pub content: String,
    pub output_chars: usize,
    pub provider_budget: WriterProviderBudgetReport,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveConflict {
    pub reason: String,
    pub base_revision: String,
    pub current_revision: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub open_chapter_title: Option<String>,
    pub dirty: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub draft_title: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveGeneratedChapterOutput {
    pub chapter_title: String,
    pub new_revision: String,
    pub saved_mode: String,
    pub output_chars: usize,
}

#[derive(Debug, Clone)]
pub struct SaveGeneratedChapterInput {
    pub request_id: String,
    pub target: ChapterTarget,
    pub generated_content: String,
    pub chapter_contract: ChapterContract,
    pub base_revision: String,
    pub save_mode: SaveMode,
    pub frontend_state: Option<FrontendChapterStateSnapshot>,
    pub receipt: WriterTaskReceipt,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutlineUpdateOutput {
    pub outline_revision: String,
    pub changed: bool,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChapterGenerationEvent {
    pub request_id: String,
    pub phase: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    pub status: String,
    pub message: String,
    pub progress: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_chapter_title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sources: Option<Vec<ChapterContextSource>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub budget: Option<ChapterContextBudgetReport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub receipt: Option<WriterTaskReceipt>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub intent_artifact: Option<ChapterIntentArtifact>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected_evidence: Option<Vec<ChapterSelectedEvidenceArtifact>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rule_stack: Option<ChapterRuleStackArtifact>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trace_artifact: Option<ChapterTraceArtifact>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scene_plan: Option<Vec<ScenePlanEntry>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub settlement_delta: Option<ChapterSettlementDelta>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub settlement_apply: Option<ChapterSettlementApplyResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub length_telemetry: Option<ChapterLengthTelemetry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artifact_refs: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub saved: Option<SaveGeneratedChapterOutput>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chapter_contract: Option<ChapterContract>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_chars: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conflict: Option<SaveConflict>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ChapterGenerationError>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generation_strategy: Option<GenerationStrategy>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quality_report: Option<ChapterQualityReport>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone)]
pub enum PipelineTerminal {
    Completed {
        saved: SaveGeneratedChapterOutput,
        generated_content: String,
        settlement_delta: Box<ChapterSettlementDelta>,
        quality_report: Option<ChapterQualityReport>,
    },
    Conflict(SaveConflict),
    Failed(ChapterGenerationError),
}

#[derive(Debug, Clone)]
pub enum SaveDecision {
    WriteTarget,
    WriteDraft {
        draft_title: String,
        conflict: SaveConflict,
    },
    Conflict(SaveConflict),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChapterContractPhase {
    ModelOutput,
    Save,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChapterContractOutcome {
    Valid,
    UnderMinChars,
    OverMaxChars,
    UnderSaveFloor,
    OverSaveCeiling,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum GenerationStrategy {
    InteractiveFastDraft,
    #[default]
    InteractiveSafeDraft,
    BackgroundLongChapter,
    RepairHeavyMode,
}

pub fn char_count(text: &str) -> usize {
    text.chars().count()
}

pub fn truncate_text_report(text: &str, max_chars: usize) -> (String, usize, bool) {
    let original_chars = char_count(text);
    if original_chars <= max_chars {
        return (text.to_string(), original_chars, false);
    }

    if max_chars == 0 {
        return (String::new(), 0, true);
    }

    let end_byte = text
        .char_indices()
        .nth(max_chars)
        .map(|(idx, _)| idx)
        .unwrap_or(text.len());
    let candidate = &text[..end_byte];
    let min_boundary_chars = max_chars.saturating_div(3).max(1);
    let mut chosen_boundary: Option<usize> = None;
    let mut seen_chars = 0usize;

    for (idx, ch) in candidate.char_indices() {
        seen_chars += 1;
        if seen_chars >= min_boundary_chars
            && matches!(ch, '\n' | '。' | '！' | '？' | '；' | ';' | '.' | '!' | '?')
        {
            chosen_boundary = Some(idx + ch.len_utf8());
        }
    }

    let truncated = if let Some(boundary) = chosen_boundary {
        candidate[..boundary].trim_end().to_string()
    } else {
        candidate.trim_end().to_string()
    };
    let included_chars = char_count(&truncated);
    (truncated, included_chars, true)
}

#[allow(clippy::result_large_err)]
pub fn resolve_target_from_outline(
    outline: &[storage::OutlineNode],
    target_chapter_title: Option<&str>,
    target_chapter_number: Option<usize>,
    summary_override: Option<&str>,
) -> Result<ChapterTarget, ChapterGenerationError> {
    if let Some(title) = target_chapter_title
        .map(str::trim)
        .filter(|t| !t.is_empty())
    {
        let matches: Vec<(usize, &storage::OutlineNode)> = outline
            .iter()
            .enumerate()
            .filter(|(_, node)| node.chapter_title == title)
            .collect();

        if matches.len() > 1 {
            return Err(ChapterGenerationError::new(
                "TARGET_CHAPTER_AMBIGUOUS",
                format!("More than one outline node is titled '{}'.", title),
                true,
            ));
        }

        if let Some((idx, node)) = matches.first() {
            return Ok(ChapterTarget {
                title: node.chapter_title.clone(),
                filename: storage::chapter_filename(&node.chapter_title),
                number: Some(idx + 1),
                summary: summary_override
                    .map(str::to_string)
                    .unwrap_or_else(|| node.summary.clone()),
                status: node.status.clone(),
            });
        }

        if let Some(summary) = summary_override {
            return Ok(ChapterTarget {
                title: title.to_string(),
                filename: storage::chapter_filename(title),
                number: None,
                summary: summary.to_string(),
                status: "empty".to_string(),
            });
        }

        return Err(ChapterGenerationError::new(
            "TARGET_CHAPTER_NOT_FOUND",
            format!("No outline node found for '{}'.", title),
            true,
        ));
    }

    if let Some(number) = target_chapter_number {
        if number == 0 {
            return Err(ChapterGenerationError::new(
                "TARGET_CHAPTER_NOT_FOUND",
                "Chapter numbers start at 1.",
                true,
            ));
        }
        if let Some(node) = outline.get(number - 1) {
            return Ok(ChapterTarget {
                title: node.chapter_title.clone(),
                filename: storage::chapter_filename(&node.chapter_title),
                number: Some(number),
                summary: summary_override
                    .map(str::to_string)
                    .unwrap_or_else(|| node.summary.clone()),
                status: node.status.clone(),
            });
        }
        return Err(ChapterGenerationError::new(
            "TARGET_CHAPTER_NOT_FOUND",
            format!("Outline has no chapter {}.", number),
            true,
        ));
    }

    Err(ChapterGenerationError::new(
        "TARGET_CHAPTER_NOT_FOUND",
        "No target chapter title or number was provided.",
        true,
    ))
}

pub fn decide_save_action(
    target_title: &str,
    request_id: &str,
    save_mode: SaveMode,
    base_revision: &str,
    current_revision: &str,
    frontend_state: Option<&FrontendChapterStateSnapshot>,
) -> SaveDecision {
    let mut conflict_reason: Option<String> = None;
    let mut open_title = None;
    let mut dirty = false;

    if let Some(frontend) = frontend_state {
        open_title = frontend.open_chapter_title.clone();
        dirty = frontend.dirty;
        if frontend
            .open_chapter_title
            .as_deref()
            .map(|title| title == target_title)
            .unwrap_or(false)
            && frontend.dirty
        {
            conflict_reason = Some("frontend_dirty_open_chapter".to_string());
        }
    }

    if conflict_reason.is_none()
        && save_mode == SaveMode::CreateIfMissing
        && current_revision != "missing"
    {
        conflict_reason = Some("target_already_exists".to_string());
    }

    if conflict_reason.is_none() && current_revision != base_revision {
        conflict_reason = Some("revision_mismatch".to_string());
    }

    if let Some(reason) = conflict_reason {
        let draft_title = if save_mode == SaveMode::SaveAsDraft {
            Some(make_draft_title(target_title, request_id))
        } else {
            None
        };
        let conflict = SaveConflict {
            reason,
            base_revision: base_revision.to_string(),
            current_revision: current_revision.to_string(),
            open_chapter_title: open_title,
            dirty,
            draft_title: draft_title.clone(),
        };

        if let Some(draft_title) = draft_title {
            SaveDecision::WriteDraft {
                draft_title,
                conflict,
            }
        } else {
            SaveDecision::Conflict(conflict)
        }
    } else {
        SaveDecision::WriteTarget
    }
}

pub fn validate_generated_content(
    content: &str,
    contract: &ChapterContract,
    phase: ChapterContractPhase,
) -> Result<(), ChapterGenerationError> {
    validate_generated_content_basics(content)?;

    let output_chars = char_count(content);
    let (min_chars, max_chars, under_code, over_code) = match phase {
        ChapterContractPhase::ModelOutput => (
            contract.min_chars,
            contract.max_chars,
            "MODEL_OUTPUT_UNDER_MIN_CHARS",
            "MODEL_OUTPUT_OVER_MAX_CHARS",
        ),
        ChapterContractPhase::Save => (
            contract.save_hard_floor_chars,
            contract.save_hard_ceiling_chars,
            "CONTENT_UNDER_SAVE_FLOOR",
            "CONTENT_OVER_SAVE_CEILING",
        ),
    };

    if output_chars < min_chars {
        return Err(ChapterGenerationError::new(
            under_code,
            format!(
                "Generated chapter content has {} chars, below the required minimum of {}.",
                output_chars, min_chars
            ),
            true,
        ));
    }

    if output_chars > max_chars {
        return Err(ChapterGenerationError::new(
            over_code,
            format!(
                "Generated chapter content has {} chars, above the allowed maximum of {}.",
                output_chars, max_chars
            ),
            true,
        ));
    }

    Ok(())
}

pub fn validate_generated_content_basics(content: &str) -> Result<(), ChapterGenerationError> {
    if content.trim().is_empty() {
        return Err(ChapterGenerationError::new(
            "MODEL_OUTPUT_EMPTY",
            "The model returned empty chapter content.",
            true,
        ));
    }

    let output_chars = char_count(content);
    if output_chars > DEFAULT_OUTPUT_HARD_CAP_CHARS {
        return Err(ChapterGenerationError::new(
            "MODEL_OUTPUT_TOO_LARGE",
            format!(
                "The model returned {} characters, above the hard cap of {}.",
                output_chars, DEFAULT_OUTPUT_HARD_CAP_CHARS
            ),
            true,
        ));
    }

    Ok(())
}

pub fn chapter_contract_outcome(
    content: &str,
    contract: &ChapterContract,
    phase: ChapterContractPhase,
) -> ChapterContractOutcome {
    let output_chars = char_count(content);
    match phase {
        ChapterContractPhase::ModelOutput => {
            if output_chars < contract.min_chars {
                ChapterContractOutcome::UnderMinChars
            } else if output_chars > contract.max_chars {
                ChapterContractOutcome::OverMaxChars
            } else {
                ChapterContractOutcome::Valid
            }
        }
        ChapterContractPhase::Save => {
            if output_chars < contract.save_hard_floor_chars {
                ChapterContractOutcome::UnderSaveFloor
            } else if output_chars > contract.save_hard_ceiling_chars {
                ChapterContractOutcome::OverSaveCeiling
            } else {
                ChapterContractOutcome::Valid
            }
        }
    }
}
