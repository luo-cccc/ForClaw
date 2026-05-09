//! Headless backend facade for CLI/MCP hosts.
//!
//! This is the supported runtime facade for Forge Agent. It exposes the durable
//! kernel and storage core without a desktop UI runtime.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashSet};

use agent_harness_core::context_quality::ContextQualityReport;
use agent_harness_core::provider::openai_compat::OpenAiCompatProvider;
use agent_harness_core::provider::LlmMessage;
use agent_harness_core::tool_executor::{
    ToolExecutionAuditEvent, ToolExecutionAuditSink, ToolHandler,
};
use agent_harness_core::AgentLoopEvent;
use rusqlite::{Connection, OpenFlags};

use crate::agent_status::AgentKernelStatus;
use crate::storage::{self, ChapterInfo, LoreEntry, OutlineNode, ProjectManifest};
use crate::writer_agent::feedback::ProposalFeedback;
use crate::writer_agent::kernel::{
    ModelStartedEventContext, WriterAgentApprovalMode, WriterAgentFrontendState,
    WriterAgentRunRequest, WriterAgentRunResult, WriterAgentStreamMode, WriterAgentTask,
};
use crate::writer_agent::memory::{
    BookStateSummary, ManualAgentTurnSummary, VolumeSnapshotSummary, VolumeSummary, WriterMemory,
};
use crate::writer_agent::observation::WriterObservation;
use crate::writer_agent::operation::{
    OperationApproval, OperationError, OperationResult, WriterOperation,
};
use crate::writer_agent::provider_budget::{
    apply_provider_budget_approval, evaluate_provider_budget, WriterProviderBudgetApproval,
    WriterProviderBudgetReport, WriterProviderBudgetRequest, WriterProviderBudgetTask,
};
use crate::writer_agent::supervised_sprint::{
    advance_sprint, budget_ceiling_reached, cancel_sprint, check_sprint_quality_gate,
    checkpoint_sprint, create_sprint_plan_with_limits, pause_sprint, record_budget_usage,
    resume_sprint, set_sprint_quality_gate, sprint_progress, update_current_chapter_state,
    SprintCheckpoint, SprintProgress, SupervisedSprintPlan,
};
use crate::writer_agent::WriterAgentKernel;

const ACTIVE_PROJECT_FILENAME: &str = "active_project.json";
const DEFAULT_PROJECT_NAME: &str = "Local Project";
const KNOWLEDGE_INDEX_FILENAME: &str = "knowledge_index.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HeadlessConfig {
    pub data_dir: PathBuf,
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub project_name: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HeadlessProjectPaths {
    pub data_dir: String,
    pub project_data_dir: String,
    pub chapters_dir: String,
    pub writer_memory_db: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HeadlessStatus {
    pub project: ProjectManifest,
    pub paths: HeadlessProjectPaths,
    pub kernel: crate::writer_agent::kernel::WriterAgentStatus,
    pub active_sprint: Option<SupervisedSprintPlan>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HeadlessGraphEntity {
    pub id: String,
    pub name: String,
    pub category: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HeadlessGraphRelationship {
    pub source: String,
    pub target: String,
    pub label: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HeadlessGraphChapter {
    pub title: String,
    pub summary: String,
    pub status: String,
    pub word_count: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HeadlessProjectGraphData {
    pub entities: Vec<HeadlessGraphEntity>,
    pub relationships: Vec<HeadlessGraphRelationship>,
    pub chapters: Vec<HeadlessGraphChapter>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AskAgentRequest {
    pub message: String,
    #[serde(default)]
    pub context: String,
    #[serde(default)]
    pub paragraph: String,
    #[serde(default)]
    pub selected_text: String,
    #[serde(default)]
    pub chapter_title: Option<String>,
    #[serde(default)]
    pub chapter_revision: Option<String>,
    #[serde(default)]
    pub cursor_position: Option<usize>,
    #[serde(default)]
    pub dirty: Option<bool>,
    #[serde(default)]
    pub inline_operation: bool,
    #[serde(default)]
    pub provider_budget_approval: Option<WriterProviderBudgetApproval>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalyzeChapterRequest {
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewItem {
    pub quote: String,
    #[serde(rename = "type")]
    pub review_type: String,
    pub issue: String,
    pub suggestion: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewReport {
    pub reviews: Vec<ReviewItem>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParallelDraftRequest {
    pub prefix: String,
    pub suffix: String,
    pub paragraph: String,
    pub selected_text: String,
    #[serde(default)]
    pub chapter_title: Option<String>,
    #[serde(default)]
    pub mission_context: String,
    #[serde(default)]
    pub promise_context: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParallelDraft {
    pub id: String,
    pub label: String,
    pub text: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AskProjectBrainRequest {
    pub query: String,
    #[serde(default)]
    pub provider_budget_approval: Option<WriterProviderBudgetApproval>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AskProjectBrainResponse {
    pub answer: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum MetacognitiveRecoveryAction {
    PlanningReview,
    ContinuityDiagnostic,
}

impl MetacognitiveRecoveryAction {
    fn task(&self) -> WriterAgentTask {
        match self {
            Self::PlanningReview => WriterAgentTask::PlanningReview,
            Self::ContinuityDiagnostic => WriterAgentTask::ContinuityDiagnostic,
        }
    }

    fn default_instruction(&self) -> &'static str {
        match self {
            Self::PlanningReview => {
                "Metacognitive gate requested recovery. Run a read-only Planning Review: rebuild the current context picture, identify missing evidence, list risks, propose candidate next actions, and ask any author-confirmation questions. Do not draft manuscript prose or mutate project memory."
            }
            Self::ContinuityDiagnostic => {
                "Metacognitive gate requested recovery. Run a read-only Continuity Diagnostic: inspect canon, chapter mission, promise, save, and context-pressure risks; cite evidence; produce a diagnostic_report only. Do not draft manuscript prose or mutate project memory."
            }
        }
    }

    fn label(&self) -> &'static str {
        match self {
            Self::PlanningReview => "Planning Review",
            Self::ContinuityDiagnostic => "Continuity Diagnostic",
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetacognitiveRecoveryRequest {
    pub action: MetacognitiveRecoveryAction,
    #[serde(default)]
    pub instruction: Option<String>,
    #[serde(default)]
    pub context: String,
    #[serde(default)]
    pub paragraph: String,
    #[serde(default)]
    pub selected_text: String,
    #[serde(default)]
    pub chapter_title: Option<String>,
    #[serde(default)]
    pub chapter_revision: Option<String>,
    #[serde(default)]
    pub cursor_position: Option<usize>,
    #[serde(default)]
    pub dirty: Option<bool>,
    #[serde(default)]
    pub provider_budget_approval: Option<WriterProviderBudgetApproval>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MetacognitiveRecoveryResponse {
    pub action: MetacognitiveRecoveryActionResult,
    pub answer: String,
    pub task_packet: agent_harness_core::TaskPacket,
    pub task_receipt: Option<crate::writer_agent::task_receipt::WriterTaskReceipt>,
    pub context_pack_summary: crate::writer_agent::kernel::WriterAgentContextPackSummary,
    pub trace_refs: Vec<String>,
    pub source_refs: Vec<String>,
    pub events: Vec<AgentLoopEvent>,
    pub provider_budget: WriterProviderBudgetReport,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchGenerateChapterRequest {
    pub chapter_title: String,
    pub summary: String,
    #[serde(default)]
    pub frontend_state: Option<crate::chapter_generation::FrontendChapterStateSnapshot>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChapterGenerationStart {
    pub request_id: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HeadlessChapterGenerationOutput {
    pub terminal: String,
    pub events: Vec<crate::chapter_generation::ChapterGenerationEvent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub saved: Option<crate::chapter_generation::SaveGeneratedChapterOutput>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generated_content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub settlement_delta: Option<crate::chapter_generation::ChapterSettlementDelta>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conflict: Option<crate::chapter_generation::SaveConflict>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<crate::chapter_generation::ChapterGenerationError>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepairChapterStateResult {
    pub chapter_title: String,
    pub revision: String,
    pub settlement_delta: crate::chapter_generation::ChapterSettlementDelta,
    pub settlement_apply: crate::chapter_generation::ChapterSettlementApplyResult,
    pub artifact_refs: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub settlement_replay_matches: Option<bool>,
    #[serde(default)]
    pub already_repaired: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MetacognitiveRecoveryActionResult {
    PlanningReview,
    ContinuityDiagnostic,
}

impl From<&MetacognitiveRecoveryAction> for MetacognitiveRecoveryActionResult {
    fn from(value: &MetacognitiveRecoveryAction) -> Self {
        match value {
            MetacognitiveRecoveryAction::PlanningReview => Self::PlanningReview,
            MetacognitiveRecoveryAction::ContinuityDiagnostic => Self::ContinuityDiagnostic,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalResearchIngestRequest {
    pub provider: String,
    #[serde(default)]
    pub url_or_path: String,
    pub title: String,
    pub content: String,
    pub author_approved: bool,
    pub approval_reason: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticExport {
    pub path: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AskAgentResponse {
    pub request_id: String,
    pub mode: String,
    pub answer: String,
    pub proposals: Vec<crate::writer_agent::proposal::AgentProposal>,
    pub operations: Vec<WriterOperation>,
    pub run: Option<WriterAgentRunResult>,
    pub events: Vec<AgentLoopEvent>,
    pub provider_budget: Option<WriterProviderBudgetReport>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartSprintRequest {
    pub chapter_titles: Vec<String>,
    #[serde(default)]
    pub require_approval_per_chapter: bool,
    #[serde(default)]
    pub max_chapters_per_session: Option<usize>,
    #[serde(default)]
    pub budget_ceiling_micros: Option<u64>,
}

pub struct HeadlessBackend {
    config: HeadlessConfig,
    project: ProjectManifest,
    kernel: Mutex<WriterAgentKernel>,
    current_sprint: Mutex<Option<SupervisedSprintPlan>>,
}

pub struct HeadlessToolBridge {
    config: HeadlessConfig,
    project: ProjectManifest,
}

#[derive(Clone)]
pub struct HeadlessChapterGenerationProject {
    project_id: String,
    project_data_dir: PathBuf,
    memory_path: PathBuf,
    brain_path: PathBuf,
    chapters_dir: PathBuf,
    lorebook_path: PathBuf,
    outline_path: PathBuf,
}

impl HeadlessChapterGenerationProject {
    fn new(config: &HeadlessConfig, project: &ProjectManifest) -> Result<Self, String> {
        let project_data_dir = project_data_dir(&config.data_dir, &project.id)?;
        Ok(Self {
            project_id: project.id.clone(),
            memory_path: project_data_dir.join(storage::WRITER_MEMORY_DB_FILENAME),
            brain_path: project_data_dir.join("project_brain.json"),
            chapters_dir: project_data_dir.join("chapters"),
            lorebook_path: project_data_dir.join("lorebook.json"),
            outline_path: project_data_dir.join("outline.json"),
            project_data_dir,
        })
    }
}

impl crate::chapter_generation::ChapterGenerationProject for HeadlessChapterGenerationProject {
    fn project_id(&self) -> &str {
        &self.project_id
    }

    fn project_data_dir(&self) -> &Path {
        &self.project_data_dir
    }

    fn memory_path(&self) -> &Path {
        &self.memory_path
    }

    fn brain_path(&self) -> &Path {
        &self.brain_path
    }

    fn load_outline(&self) -> Result<Vec<OutlineNode>, String> {
        read_json_array(&self.outline_path)
    }

    fn save_outline(&self, nodes: &[OutlineNode]) -> Result<(), String> {
        write_json_pretty(&self.outline_path, &nodes)
    }

    fn load_lorebook(&self) -> Result<Vec<LoreEntry>, String> {
        read_json_array(&self.lorebook_path)
    }

    fn load_chapter(&self, title: &str) -> Result<String, String> {
        let path = self.chapters_dir.join(storage::chapter_filename(title));
        if !path.exists() {
            return Err(format!("Chapter '{}' not found", title));
        }
        std::fs::read_to_string(path).map_err(|error| error.to_string())
    }

    fn chapter_revision(&self, title: &str) -> Result<String, String> {
        let path = self.chapters_dir.join(storage::chapter_filename(title));
        if !path.exists() {
            return Ok("missing".to_string());
        }
        let content = std::fs::read_to_string(path).map_err(|error| error.to_string())?;
        Ok(storage::content_revision(&content))
    }

    fn save_chapter_content_and_revision(
        &self,
        title: &str,
        content: &str,
    ) -> Result<String, String> {
        let path = self.chapters_dir.join(storage::chapter_filename(title));
        storage::atomic_write(&path, content)?;
        Ok(storage::content_revision(content))
    }
    fn open_memory_db(&self) -> Option<rusqlite::Connection> {
        rusqlite::Connection::open(&self.memory_path).ok()
    }
}

impl HeadlessToolBridge {
    fn new(config: &HeadlessConfig, project: &ProjectManifest) -> Self {
        Self {
            config: config.clone(),
            project: project.clone(),
        }
    }

    fn load_chapter(&self, title: String) -> Result<String, String> {
        let path = chapter_path(&self.config.data_dir, &self.project.id, &title)?;
        std::fs::read_to_string(&path).map_err(|error| format!("load_current_chapter: {}", error))
    }

    fn load_lorebook(&self) -> Result<Vec<LoreEntry>, String> {
        read_json_array(&lorebook_path(&self.config.data_dir, &self.project.id)?)
    }

    fn load_outline(&self) -> Result<Vec<OutlineNode>, String> {
        read_json_array(&outline_path(&self.config.data_dir, &self.project.id)?)
    }
}

#[async_trait::async_trait]
impl ToolHandler for HeadlessToolBridge {
    async fn execute(
        &self,
        tool_name: &str,
        args: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        match tool_name {
            "load_current_chapter" => {
                let chapter = string_arg(&args, &["chapter", "chapter_title"]);
                let content = self.load_chapter(chapter.to_string())?;
                Ok(serde_json::json!({ "content": content, "chapter": chapter }))
            }
            "search_lorebook" => {
                let keyword = string_arg(&args, &["keyword", "keywords", "query"]);
                let entries = self.load_lorebook()?;
                let keyword_lower = keyword.to_lowercase();
                let matches = entries
                    .iter()
                    .filter(|entry| {
                        let entry_keyword = entry.keyword.to_lowercase();
                        entry_keyword.contains(&keyword_lower)
                            || keyword_lower.contains(&entry_keyword)
                    })
                    .map(|entry| {
                        serde_json::json!({
                            "keyword": entry.keyword,
                            "content": entry.content,
                        })
                    })
                    .collect::<Vec<_>>();
                Ok(serde_json::json!({ "matches": matches }))
            }
            "load_outline_node" => {
                let id = string_arg(&args, &["chapter", "id", "chapter_title"]);
                let nodes = self.load_outline()?;
                let node = nodes
                    .iter()
                    .find(|node| node.chapter_title == id)
                    .map(|node| {
                        serde_json::json!({
                            "chapter_title": node.chapter_title,
                            "summary": node.summary,
                            "status": node.status,
                        })
                    })
                    .unwrap_or_else(|| serde_json::json!({ "error": "not found" }));
                Ok(node)
            }
            "query_project_brain" => {
                let query = string_arg(&args, &["query", "semantic_query"]);
                let answer = self.query_project_brain(query).await?;
                Ok(serde_json::json!({ "answer": answer }))
            }
            "generate_bounded_continuation" => {
                let prompt = string_arg(&args, &["prompt", "context"]);
                let api_key =
                    crate::resolve_api_key().ok_or_else(|| "No API key configured".to_string())?;
                let settings = crate::llm_runtime::settings(api_key);
                let result = crate::llm_runtime::chat_text_profile(
                    &settings,
                    vec![serde_json::json!({ "role": "user", "content": prompt })],
                    crate::llm_runtime::LlmRequestProfile::ToolContinuation,
                    120,
                )
                .await
                .map_err(|error| format!("generate_bounded_continuation: {}", error))?;
                Ok(serde_json::json!({ "text": result }))
            }
            "generate_chapter_draft" => {
                Err("generate_chapter_draft requires explicit approval.".to_string())
            }
            "read_user_drift_profile"
            | "load_domain_profile"
            | "pack_agent_context"
            | "plan_chapter_task"
            | "classify_writing_intent"
            | "record_run_trace" => Ok(serde_json::json!({
                "status": "ok",
                "tool": tool_name,
            })),
            _ => Err(format!("Unknown tool: {}", tool_name)),
        }
    }
}

impl HeadlessToolBridge {
    async fn query_project_brain(&self, query: &str) -> Result<String, String> {
        let api_key =
            crate::resolve_api_key().ok_or_else(|| "No API key configured".to_string())?;
        let settings = crate::llm_runtime::settings(api_key);
        answer_project_brain_query(
            &self.config.data_dir,
            &self.project.id,
            &settings,
            query,
            None,
        )
        .await
    }
}

impl HeadlessBackend {
    pub fn open(config: HeadlessConfig) -> Result<Self, String> {
        std::fs::create_dir_all(&config.data_dir).map_err(|error| {
            format!(
                "Failed to create data dir '{}': {}",
                config.data_dir.display(),
                error
            )
        })?;
        let project = load_or_create_project_manifest(&config)?;
        let project_data_dir = project_data_dir(&config.data_dir, &project.id)?;
        std::fs::create_dir_all(&project_data_dir).map_err(|error| {
            format!(
                "Failed to create project data dir '{}': {}",
                project_data_dir.display(),
                error
            )
        })?;
        std::fs::create_dir_all(chapters_dir(&config.data_dir, &project.id)?)
            .map_err(|error| format!("Failed to create chapters dir: {}", error))?;

        let memory_path = writer_memory_path(&config.data_dir, &project.id)?;
        let memory = WriterMemory::open(&memory_path).map_err(|error| {
            format!(
                "Failed to open writer memory DB at '{}': {}",
                memory_path.display(),
                error
            )
        })?;
        seed_story_model_if_empty(&config.data_dir, &project, &memory);
        let current_sprint = memory
            .get_latest_active_supervised_sprint(&project.id)
            .map_err(|error| format!("Failed to restore supervised sprint: {}", error))?;
        let kernel = WriterAgentKernel::new(&project.id, memory);

        Ok(Self {
            config,
            project,
            kernel: Mutex::new(kernel),
            current_sprint: Mutex::new(current_sprint),
        })
    }

    pub fn default_config() -> Result<HeadlessConfig, String> {
        let cwd = std::env::current_dir()
            .map_err(|error| format!("Failed to resolve current directory: {}", error))?;
        Ok(HeadlessConfig {
            data_dir: std::env::var("FORGE_AGENT_DATA_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|_| cwd.join(".forge-agent-data")),
            project_id: std::env::var("FORGE_AGENT_PROJECT_ID").ok(),
            project_name: std::env::var("FORGE_AGENT_PROJECT_NAME").ok(),
        })
    }

    pub fn project(&self) -> &ProjectManifest {
        &self.project
    }

    pub fn paths(&self) -> Result<HeadlessProjectPaths, String> {
        let project_data_dir = project_data_dir(&self.config.data_dir, &self.project.id)?;
        let chapters_dir = chapters_dir(&self.config.data_dir, &self.project.id)?;
        let writer_memory_db = writer_memory_path(&self.config.data_dir, &self.project.id)?;
        Ok(HeadlessProjectPaths {
            data_dir: self.config.data_dir.to_string_lossy().to_string(),
            project_data_dir: project_data_dir.to_string_lossy().to_string(),
            chapters_dir: chapters_dir.to_string_lossy().to_string(),
            writer_memory_db: writer_memory_db.to_string_lossy().to_string(),
        })
    }

    pub fn status(&self) -> Result<HeadlessStatus, String> {
        let kernel = self.lock_kernel()?;
        let sprint = self.lock_sprint()?.clone();
        Ok(HeadlessStatus {
            project: self.project.clone(),
            paths: self.paths()?,
            kernel: kernel.status(),
            active_sprint: sprint,
        })
    }

    pub fn agent_tools(&self) -> Vec<crate::agent_runtime::AgentToolDescriptor> {
        crate::agent_runtime::registered_tools()
    }

    pub fn effective_agent_tool_inventory(&self) -> agent_harness_core::EffectiveToolInventory {
        crate::agent_runtime::effective_tool_inventory()
    }

    pub fn agent_kernel_status(&self) -> AgentKernelStatus {
        let registry = agent_harness_core::default_writing_tool_registry();
        let tools = registry.list();
        let inventory = crate::agent_runtime::effective_tool_inventory();
        let domain = agent_harness_core::writing_domain_profile();

        AgentKernelStatus {
            tool_generation: registry.generation(),
            tool_count: tools.len(),
            effective_tool_count: inventory.allowed.len(),
            blocked_tool_count: inventory.blocked.len(),
            model_callable_tool_count: inventory.openai_callable_allowed_count(),
            approval_required_tool_count: tools
                .iter()
                .filter(|tool| tool.requires_approval)
                .count(),
            write_tool_count: tools
                .iter()
                .filter(|tool| {
                    tool.side_effect_level == agent_harness_core::ToolSideEffectLevel::Write
                })
                .count(),
            domain_id: domain.id,
            capability_count: domain.capabilities.len(),
            quality_gate_count: domain.quality_gates.len(),
            trace_enabled: true,
        }
    }

    pub fn agent_domain_profile(&self) -> agent_harness_core::AgentDomainProfile {
        agent_harness_core::writing_domain_profile()
    }

    pub fn list_chapters(&self) -> Result<Vec<ChapterInfo>, String> {
        read_project_dir(&self.config.data_dir, &self.project.id)
    }

    pub fn create_chapter(&self, title: String) -> Result<ChapterInfo, String> {
        let filename = storage::chapter_filename(&title);
        let path = chapter_path(&self.config.data_dir, &self.project.id, &title)?;
        if !path.exists() {
            storage::atomic_write(&path, "")?;
        }
        Ok(ChapterInfo { title, filename })
    }

    pub fn load_chapter(&self, title: String) -> Result<String, String> {
        let path = chapter_path(&self.config.data_dir, &self.project.id, &title)?;
        if !path.exists() {
            return Err(format!("Chapter '{}' not found", title));
        }
        std::fs::read_to_string(&path).map_err(|error| error.to_string())
    }

    pub fn save_chapter(&self, title: String, content: String) -> Result<String, String> {
        let path = chapter_path(&self.config.data_dir, &self.project.id, &title)?;
        storage::atomic_write(&path, &content)?;
        let revision = storage::content_revision(&content);

        let observation = chapter_save_observation(&self.project.id, &title, &revision, &content);
        let mut kernel = self.lock_kernel()?;
        kernel.observe(observation)?;
        Ok(revision)
    }

    pub fn chapter_revision(&self, title: String) -> Result<String, String> {
        let path = chapter_path(&self.config.data_dir, &self.project.id, &title)?;
        if !path.exists() {
            return Ok("missing".to_string());
        }
        let content = std::fs::read_to_string(path).map_err(|error| error.to_string())?;
        Ok(storage::content_revision(&content))
    }

    pub fn record_manual_craft_edit_feedback(
        &self,
        mut request: crate::chapter_generation::ManualCraftEditFeedbackRequest,
    ) -> Result<crate::chapter_generation::ManualCraftEditFeedbackResult, String> {
        if request.anchor_keywords.is_empty() {
            request.anchor_keywords =
                manual_quality_anchor_keywords(&self.load_lorebook().unwrap_or_default(), &request);
        }
        if request.author_voice.is_none() {
            let memory_path = writer_memory_path(&self.config.data_dir, &self.project.id)?;
            if let Ok(memory) = WriterMemory::open(&memory_path) {
                request.author_voice = Some(
                    crate::writer_agent::author_voice::build_author_voice_snapshot(
                        &memory,
                        std::slice::from_ref(&request.chapter_title),
                        now_ms(),
                    ),
                );
            }
        }
        let memory_path = writer_memory_path(&self.config.data_dir, &self.project.id)?;
        let conn = rusqlite::Connection::open(&memory_path)
            .map_err(|error| format!("open writer memory: {}", error))?;
        crate::chapter_generation::record_manual_craft_edit_feedback(&conn, request)
    }

    pub fn rename_chapter_file(&self, old_name: String, new_name: String) -> Result<(), String> {
        let dir = chapters_dir(&self.config.data_dir, &self.project.id)?;
        let old_path = safe_chapter_file_path(&dir, &old_name)?;
        let new_path = safe_chapter_file_path(&dir, &new_name)?;
        if old_path.exists() && !new_path.exists() {
            std::fs::rename(&old_path, &new_path).map_err(|error| error.to_string())?;
        }
        Ok(())
    }

    pub fn load_lorebook(&self) -> Result<Vec<LoreEntry>, String> {
        read_json_array(&lorebook_path(&self.config.data_dir, &self.project.id)?)
    }

    pub fn save_lore_entry(
        &self,
        keyword: String,
        content: String,
    ) -> Result<Vec<LoreEntry>, String> {
        let mut entries = self.load_lorebook()?;
        if let Some(entry) = entries.iter_mut().find(|entry| entry.keyword == keyword) {
            entry.content = content;
        } else {
            entries.push(LoreEntry {
                id: (entries.len() + 1).to_string(),
                keyword,
                content,
            });
        }
        write_json_pretty(
            &lorebook_path(&self.config.data_dir, &self.project.id)?,
            &entries,
        )?;
        self.reseed_story_model();
        Ok(entries)
    }

    pub fn delete_lore_entry(&self, id: String) -> Result<Vec<LoreEntry>, String> {
        let mut entries = self.load_lorebook()?;
        entries.retain(|entry| entry.id != id);
        write_json_pretty(
            &lorebook_path(&self.config.data_dir, &self.project.id)?,
            &entries,
        )?;
        self.reseed_story_model();
        Ok(entries)
    }

    pub fn load_outline(&self) -> Result<Vec<OutlineNode>, String> {
        read_json_array(&outline_path(&self.config.data_dir, &self.project.id)?)
    }

    pub fn save_outline_node(
        &self,
        chapter_title: String,
        summary: String,
        status: Option<String>,
    ) -> Result<Vec<OutlineNode>, String> {
        let mut nodes = self.load_outline()?;
        if let Some(node) = nodes
            .iter_mut()
            .find(|node| node.chapter_title == chapter_title)
        {
            node.summary = summary;
            if let Some(status) = status {
                node.status = status;
            }
        } else {
            nodes.push(OutlineNode {
                chapter_title,
                summary,
                status: status.unwrap_or_else(|| "draft".to_string()),
            });
        }
        write_json_pretty(
            &outline_path(&self.config.data_dir, &self.project.id)?,
            &nodes,
        )?;
        self.reseed_story_model();
        Ok(nodes)
    }

    pub fn delete_outline_node(&self, chapter_title: String) -> Result<Vec<OutlineNode>, String> {
        let mut nodes = self.load_outline()?;
        nodes.retain(|node| node.chapter_title != chapter_title);
        write_json_pretty(
            &outline_path(&self.config.data_dir, &self.project.id)?,
            &nodes,
        )?;
        Ok(nodes)
    }

    pub fn update_outline_status(
        &self,
        chapter_title: String,
        status: String,
    ) -> Result<Vec<OutlineNode>, String> {
        let mut nodes = self.load_outline()?;
        if let Some(node) = nodes
            .iter_mut()
            .find(|node| node.chapter_title == chapter_title)
        {
            node.status = status;
        }
        write_json_pretty(
            &outline_path(&self.config.data_dir, &self.project.id)?,
            &nodes,
        )?;
        Ok(nodes)
    }

    pub fn reorder_outline_nodes(
        &self,
        ordered_titles: Vec<String>,
    ) -> Result<Vec<OutlineNode>, String> {
        let nodes = self.load_outline()?;
        let mut reordered = Vec::with_capacity(nodes.len());
        for title in &ordered_titles {
            if let Some(node) = nodes.iter().find(|node| &node.chapter_title == title) {
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
        write_json_pretty(
            &outline_path(&self.config.data_dir, &self.project.id)?,
            &reordered,
        )?;
        Ok(reordered)
    }

    pub fn list_volumes(&self) -> Result<Vec<VolumeSummary>, String> {
        let kernel = self.lock_kernel()?;
        kernel
            .memory
            .list_volumes(&self.project.id)
            .map_err(|error| error.to_string())
    }

    pub fn save_volume(&self, mut volume: VolumeSummary) -> Result<VolumeSummary, String> {
        volume.project_id = self.project.id.clone();
        let kernel = self.lock_kernel()?;
        kernel
            .memory
            .upsert_volume(&volume)
            .map_err(|error| error.to_string())?;
        Ok(volume)
    }

    pub fn delete_volume(&self, volume_id: String) -> Result<bool, String> {
        let kernel = self.lock_kernel()?;
        kernel
            .memory
            .delete_volume(&self.project.id, &volume_id)
            .map_err(|error| error.to_string())
    }

    pub fn get_volume_snapshot(
        &self,
        volume_id: String,
    ) -> Result<Option<VolumeSnapshotSummary>, String> {
        let kernel = self.lock_kernel()?;
        kernel
            .memory
            .get_latest_volume_snapshot(&self.project.id, &volume_id)
            .map_err(|error| error.to_string())
    }

    pub fn save_volume_snapshot(&self, mut snapshot: VolumeSnapshotSummary) -> Result<i64, String> {
        snapshot.project_id = self.project.id.clone();
        let kernel = self.lock_kernel()?;
        kernel
            .memory
            .upsert_volume_snapshot(&snapshot)
            .map_err(|error| error.to_string())
    }

    pub fn get_book_state(&self) -> Result<Option<BookStateSummary>, String> {
        let kernel = self.lock_kernel()?;
        kernel
            .memory
            .get_book_state(&self.project.id)
            .map_err(|error| error.to_string())
    }

    pub fn save_book_state(
        &self,
        mut book_state: BookStateSummary,
    ) -> Result<BookStateSummary, String> {
        book_state.project_id = self.project.id.clone();
        let kernel = self.lock_kernel()?;
        kernel
            .memory
            .upsert_book_state(&book_state)
            .map_err(|error| error.to_string())?;
        Ok(book_state)
    }

    pub async fn analyze_chapter(
        &self,
        request: AnalyzeChapterRequest,
    ) -> Result<Vec<ReviewItem>, String> {
        let api_key = crate::require_api_key()?;
        let settings = crate::llm_runtime::settings(api_key);
        let budget_report = evaluate_provider_budget(WriterProviderBudgetRequest::new(
            WriterProviderBudgetTask::ManualRequest,
            &settings.model,
            2_000,
            1_024,
        ));
        tracing::info!(
            task = "headless_analyze_chapter",
            decision = ?budget_report.decision,
            tokens = budget_report.estimated_total_tokens,
            "Provider budget preflight"
        );

        let system_prompt = r#"You are a professional novel editor. Analyze the chapter and output a JSON object with a "reviews" array.

Each review must have:
- "quote": exact text from the chapter (copy verbatim, at least 10 characters)
- "type": one of "logic" | "ooc" | "pacing" | "prose"
- "issue": what the problem is
- "suggestion": how to fix it (in Chinese, specific rewrite suggestion)

Output ONLY the JSON object, no explanation outside. Example:
{"reviews":[{"quote":"他走出了房间","type":"prose","issue":"缺乏画面感","suggestion":"他推开吱呀作响的木门，幽暗的走廊里只有自己的脚步声在回荡。"}]}"#;

        let truncated = agent_harness_core::truncate_context(&request.content, 8_000);
        let body = crate::llm_runtime::chat_json(
            &settings,
            vec![
                serde_json::json!({ "role": "system", "content": system_prompt }),
                serde_json::json!({ "role": "user", "content": format!("Analyze this chapter:\n\n{}", truncated) }),
            ],
            60,
        )
        .await?;
        let report: ReviewReport = serde_json::from_value(body)
            .map_err(|error| format!("Failed to parse review JSON: {}", error))?;
        Ok(report.reviews)
    }

    pub async fn analyze_pacing(&self, summaries: String) -> Result<String, String> {
        let api_key = crate::require_api_key()?;
        let settings = crate::llm_runtime::settings(api_key);
        let budget_report = evaluate_provider_budget(WriterProviderBudgetRequest::new(
            WriterProviderBudgetTask::ManualRequest,
            &settings.model,
            3_000,
            1_024,
        ));
        tracing::info!(
            task = "headless_analyze_pacing",
            decision = ?budget_report.decision,
            tokens = budget_report.estimated_total_tokens,
            "Provider budget preflight"
        );

        let text = crate::llm_runtime::chat_text_profile(
            &settings,
            vec![
                serde_json::json!({"role": "system", "content": "You are a structural editor. Analyze the chapter sequence for pacing issues, slow sections, abrupt transitions, and unresolved arcs. Be specific and concise."}),
                serde_json::json!({"role": "user", "content": format!("Chapter summaries:\n{}", summaries)}),
            ],
            crate::llm_runtime::LlmRequestProfile::Analysis,
            60,
        )
        .await?;

        Ok(if text.is_empty() {
            "No analysis generated".to_string()
        } else {
            text
        })
    }

    pub async fn generate_parallel_drafts(
        &self,
        request: ParallelDraftRequest,
    ) -> Result<Vec<ParallelDraft>, String> {
        let api_key = crate::require_api_key()?;
        let settings = crate::llm_runtime::settings(api_key);
        let budget_report = evaluate_provider_budget(WriterProviderBudgetRequest::new(
            WriterProviderBudgetTask::GhostPreview,
            &settings.model,
            2_000,
            2_000,
        ));
        tracing::info!(
            task = "headless_generate_parallel_drafts",
            decision = ?budget_report.decision,
            tokens = budget_report.estimated_total_tokens,
            "Provider budget preflight"
        );

        let chapter = request
            .chapter_title
            .as_deref()
            .filter(|title| !title.trim().is_empty())
            .unwrap_or("当前章节");
        let focus = if request.selected_text.trim().is_empty() {
            request.paragraph.trim()
        } else {
            request.selected_text.trim()
        };
        let mission_block = if request.mission_context.trim().is_empty() {
            String::new()
        } else {
            format!("\n## 本章任务约束\n{}\n", request.mission_context)
        };
        let promise_block = if request.promise_context.trim().is_empty() {
            String::new()
        } else {
            format!("\n## 未兑现伏笔参考\n{}\n", request.promise_context)
        };
        let prompt = format!(
            "你是中文小说共创写手。请顺着用户已有文本，生成三个不同方向的平行草稿。\n\
             输出格式必须严格为：\n\
             A: ...\nB: ...\nC: ...\n\
             每个版本 2-5 句，可以分段；每个版本末尾用括号标注关联的创作依据。不要解释，不要 Markdown。\n\
             A 偏顺势推进，B 偏冲突加压，C 偏情绪转折。\n\
             ## 章节\n{}{}{}\n## 光标前文\n{}\n## 光标后文\n{}\n## 当前焦点\n{}",
            chapter,
            mission_block,
            promise_block,
            agent_harness_core::truncate_context(&request.prefix, 3_000),
            agent_harness_core::truncate_context(&request.suffix, 1_000),
            focus,
        );

        let text = crate::llm_runtime::chat_text_profile(
            &settings,
            vec![serde_json::json!({ "role": "user", "content": prompt })],
            crate::llm_runtime::LlmRequestProfile::ParallelDraft,
            45,
        )
        .await?;
        let drafts = parse_parallel_drafts(&text);
        if drafts.is_empty() {
            let fallback = trim_parallel_draft(&text);
            if fallback.is_empty() {
                return Ok(Vec::new());
            }
            return Ok(vec![ParallelDraft {
                id: "a".to_string(),
                label: "A 顺势推进".to_string(),
                text: fallback,
            }]);
        }
        Ok(drafts)
    }

    pub async fn ask_project_brain(
        &self,
        request: AskProjectBrainRequest,
    ) -> Result<AskProjectBrainResponse, String> {
        let api_key = crate::require_api_key()?;
        let settings = crate::llm_runtime::settings(api_key);
        let answer = answer_project_brain_query(
            &self.config.data_dir,
            &self.project.id,
            &settings,
            &request.query,
            request.provider_budget_approval.as_ref(),
        )
        .await?;
        Ok(AskProjectBrainResponse { answer })
    }

    pub async fn run_metacognitive_recovery(
        &self,
        request: MetacognitiveRecoveryRequest,
    ) -> Result<MetacognitiveRecoveryResponse, String> {
        let api_key = crate::require_api_key()?;
        let settings = crate::llm_runtime::settings(api_key);
        let model = settings.model.clone();
        let message = request
            .instruction
            .clone()
            .filter(|instruction| !instruction.trim().is_empty())
            .unwrap_or_else(|| request.action.default_instruction().to_string());
        let ask_request = AskAgentRequest {
            message: message.clone(),
            context: request.context.clone(),
            paragraph: request.paragraph.clone(),
            selected_text: request.selected_text.clone(),
            chapter_title: request.chapter_title.clone(),
            chapter_revision: request.chapter_revision.clone(),
            cursor_position: request.cursor_position,
            dirty: request.dirty,
            inline_operation: false,
            provider_budget_approval: request.provider_budget_approval.clone(),
        };
        let observation = manual_writer_observation(&ask_request, &self.project.id);
        let request_id = format!("meta-recovery-{}", now_ms());
        let recovery_task = request.action.task();
        let mut prepared_run = {
            let mut kernel = self.lock_kernel()?;
            refresh_kernel_canon_from_headless(&self.config.data_dir, &self.project, &mut kernel);
            let run_request = WriterAgentRunRequest {
                task: recovery_task,
                observation: observation.clone(),
                user_instruction: message.clone(),
                frontend_state: WriterAgentFrontendState {
                    truncated_context: agent_harness_core::truncate_context(
                        &request.context,
                        2_000,
                    )
                    .to_string(),
                    paragraph: request.paragraph.clone(),
                    selected_text: request.selected_text.clone(),
                    memory_context: String::new(),
                    has_lore: !self.load_lorebook()?.is_empty(),
                    has_outline: !self.load_outline()?.is_empty(),
                },
                approval_mode: WriterAgentApprovalMode::ReadOnly,
                stream_mode: WriterAgentStreamMode::Text,
                manual_history: Vec::new(),
            };
            let provider = Arc::new(OpenAiCompatProvider::new(
                &settings.api_base,
                &settings.api_key,
                &settings.model,
            ));
            kernel.prepare_task_run(
                run_request,
                provider,
                HeadlessToolBridge::new(&self.config, &self.project),
                &model,
            )?
        };

        let preflight_estimated_input_tokens = prepared_run.first_round_estimated_input_tokens();
        let mut budget_report = prepared_run.provider_budget_from_estimate(
            WriterProviderBudgetTask::MetacognitiveRecovery,
            model.clone(),
            preflight_estimated_input_tokens,
            4_096,
        );
        budget_report = apply_provider_budget_approval(
            budget_report,
            request.provider_budget_approval.as_ref(),
        );
        let budget_task_id = format!("metacognitive-recovery-{}", request_id);
        let budget_source_refs =
            recovery_budget_source_refs(&request_id, &observation, &budget_report, &request.action);
        self.record_provider_budget_report(
            budget_task_id.clone(),
            &budget_report,
            budget_source_refs.clone(),
        );
        if budget_report.approval_required {
            self.record_provider_budget_failure(budget_task_id, &budget_report, budget_source_refs);
            return Err("METACOGNITIVE_RECOVERY_PROVIDER_BUDGET_APPROVAL_REQUIRED".to_string());
        }

        install_headless_recovery_provider_budget_guard(
            &mut prepared_run,
            self.project.id.clone(),
            self.config.data_dir.clone(),
            request_id.clone(),
            observation.clone(),
            request.action.clone(),
            request.provider_budget_approval.clone(),
            preflight_estimated_input_tokens,
        );
        prepared_run.agent.executor.set_audit_sink(
            self.tool_audit_sink(budget_task_id, vec!["metacognitive_recovery".to_string()]),
        );
        let events = Arc::new(Mutex::new(Vec::<AgentLoopEvent>::new()));
        let callback_events = events.clone();
        prepared_run.set_event_callback(Arc::new(move |event| {
            if let Ok(mut events) = callback_events.lock() {
                events.push(event);
            }
        }));
        let run_request = prepared_run.request().clone();
        let result = prepared_run.run().await?;
        {
            let mut kernel = self.lock_kernel()?;
            kernel.record_run_completion(&run_request, &result)?;
        }
        let collected_events = events
            .lock()
            .map_err(|_| "Agent event log lock poisoned".to_string())?
            .clone();
        Ok(MetacognitiveRecoveryResponse {
            action: (&request.action).into(),
            answer: result.answer,
            task_packet: result.task_packet,
            task_receipt: result.task_receipt,
            context_pack_summary: result.context_pack_summary,
            trace_refs: result.trace_refs,
            source_refs: result.source_refs,
            events: collected_events,
            provider_budget: budget_report,
        })
    }

    pub async fn batch_generate_chapter(
        &self,
        request: BatchGenerateChapterRequest,
    ) -> Result<HeadlessChapterGenerationOutput, String> {
        let request_id = crate::chapter_generation::make_request_id("batch");
        let payload = crate::chapter_generation::GenerateChapterAutonomousPayload {
            request_id: Some(request_id),
            target_chapter_title: Some(request.chapter_title.clone()),
            target_chapter_number: None,
            user_instruction: format!("帮我写《{}》这一章的完整初稿。", request.chapter_title),
            budget: None,
            frontend_state: request.frontend_state,
            save_mode: crate::chapter_generation::SaveMode::ReplaceIfClean,
            chapter_summary_override: Some(request.summary),
            chapter_contract: None,
            provider_budget_approval: None,
        };
        self.generate_chapter_autonomous(payload).await
    }

    pub async fn generate_chapter_autonomous(
        &self,
        payload: crate::chapter_generation::GenerateChapterAutonomousPayload,
    ) -> Result<HeadlessChapterGenerationOutput, String> {
        self.ensure_sprint_allows_generation(payload.target_chapter_title.as_deref())?;
        let api_key = crate::require_api_key()?;
        let settings = crate::llm_runtime::settings(api_key);
        let request_id = payload
            .request_id
            .clone()
            .unwrap_or_else(|| crate::chapter_generation::make_request_id("chapter"));
        let payload = crate::chapter_generation::GenerateChapterAutonomousPayload {
            request_id: Some(request_id.clone()),
            ..payload
        };
        let project = HeadlessChapterGenerationProject::new(&self.config, &self.project)?;
        let memory_path = project.memory_path.clone();
        let user_profile_entries = self.user_profile_entries();
        let user_instruction = payload.user_instruction.clone();
        let events = Arc::new(Mutex::new(Vec::<
            crate::chapter_generation::ChapterGenerationEvent,
        >::new()));
        let emit_events = events.clone();
        let trace_project_id = self.project.id.clone();
        let trace_session_id = self
            .lock_kernel()
            .map(|kernel| kernel.session_id.clone())
            .unwrap_or_else(|_| format!("headless-{}", now_ms()));
        let trace_memory_path = memory_path.clone();

        let terminal = crate::chapter_generation::run_chapter_generation_pipeline(
            crate::chapter_generation::ChapterGenerationConfig {
                project,
                settings,
                payload,
                user_profile_entries,
                project_id: self.project.id.clone(),
                memory_path: trace_memory_path.clone(),
            },
            move |event| {
                if let Ok(mut events) = emit_events.lock() {
                    events.push(event);
                }
            },
            {
                let trace_project_id = trace_project_id.clone();
                let trace_session_id = trace_session_id.clone();
                let trace_memory_path = trace_memory_path.clone();
                let backend = self;
                move |context| {
                    let created_at_ms = now_ms();
                    backend.record_sprint_context_built(context);
                    if let Ok(memory) = WriterMemory::open(&trace_memory_path) {
                        let mut kernel = WriterAgentKernel::new(&trace_project_id, memory);
                        kernel.session_id = trace_session_id.clone();
                        kernel.record_chapter_context_pack_built_run_event(context, created_at_ms);
                        let packet =
                            crate::chapter_generation::build_chapter_generation_task_packet(
                                &trace_project_id,
                                &trace_session_id,
                                context,
                                &user_instruction,
                                created_at_ms,
                            );
                        if let Err(error) = kernel.record_task_packet(
                            context.request_id.clone(),
                            "ChapterGeneration",
                            packet,
                        ) {
                            tracing::warn!(
                                "Headless chapter-generation task packet rejected: {}",
                                error
                            );
                        }
                    }
                }
            },
            {
                let trace_project_id = trace_project_id.clone();
                let trace_memory_path = trace_memory_path.clone();
                let backend = self;
                move |context, report| {
                    record_chapter_provider_budget_report_headless(
                        &trace_project_id,
                        &trace_memory_path,
                        context,
                        report,
                    );
                    backend.record_sprint_provider_budget(&context.target.title, report);
                }
            },
            {
                let backend = self;
                move |context, report| {
                    backend
                        .ensure_provider_budget_allowed(&context.target.title, report)
                        .map_err(headless_sprint_blocked_error)
                }
            },
            {
                let trace_project_id = trace_project_id.clone();
                let trace_memory_path = trace_memory_path.clone();
                move |context, report| {
                    record_chapter_model_started_headless(
                        &trace_project_id,
                        &trace_memory_path,
                        context,
                        report,
                    );
                }
            },
        )
        .await;

        let output = self.finish_headless_chapter_generation(
            terminal,
            &request_id,
            events
                .lock()
                .map_err(|_| "Chapter generation event log lock poisoned".to_string())?
                .clone(),
        )?;
        Ok(output)
    }

    pub fn repair_chapter_state(
        &self,
        chapter_title: String,
    ) -> Result<RepairChapterStateResult, String> {
        repair_chapter_state_headless(self, chapter_title)
    }

    pub fn get_project_brain_knowledge_graph(
        &self,
    ) -> Result<crate::brain_service::ProjectBrainKnowledgeIndex, String> {
        load_project_brain_knowledge_index(
            &self.config.data_dir,
            &self.project.id,
            &self.load_outline()?,
            &self.load_lorebook()?,
        )
    }

    pub fn compare_project_brain_source_revisions(
        &self,
        source_ref: String,
    ) -> Result<crate::brain_service::ProjectBrainSourceCompare, String> {
        compare_project_brain_source_revisions_headless(
            &self.config.data_dir,
            &self.project.id,
            &source_ref,
        )
    }

    pub fn restore_project_brain_source_revision(
        &self,
        source_ref: String,
        revision: String,
    ) -> Result<crate::brain_service::ProjectBrainSourceRevisionRestore, String> {
        let restored = restore_project_brain_source_revision_headless(
            &self.config.data_dir,
            &self.project.id,
            &source_ref,
            &revision,
        )?;
        let _ = self.get_project_brain_knowledge_graph();
        Ok(restored)
    }

    pub fn cross_reference_brain_nodes(
        &self,
        source_node_id: String,
        target_node_id: String,
    ) -> Result<crate::brain_service::ProjectBrainCrossReferenceResult, String> {
        cross_reference_project_brain_nodes_headless(
            &self.get_project_brain_knowledge_graph()?,
            &source_node_id,
            &target_node_id,
        )
    }

    pub fn ingest_external_research(
        &self,
        request: ExternalResearchIngestRequest,
    ) -> Result<crate::brain_service::ExternalResearchIngestResult, String> {
        let result = ingest_external_research_source_headless(
            &self.config.data_dir,
            &self.project.id,
            request,
        )?;
        let _ = self.get_project_brain_knowledge_graph();
        Ok(result)
    }

    pub fn export_writer_agent_trajectory(
        &self,
        limit: Option<usize>,
        format: Option<String>,
    ) -> Result<DiagnosticExport, String> {
        let kernel = self.lock_kernel()?;
        let export = kernel.export_trajectory(limit.unwrap_or(200).min(1_000));
        let dir = self.config.data_dir.join("logs").join("trajectory");
        std::fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
        let trace_viewer_format = matches!(
            format.as_deref(),
            Some("trace_viewer" | "claude_code" | "hf_agent_trace_viewer")
        );
        let file_name = format!(
            "writer-agent-{}-{}{}.jsonl",
            safe_filename_component(&export.project_id),
            now_ms(),
            if trace_viewer_format {
                "-trace-viewer"
            } else {
                ""
            }
        );
        let path = dir.join(file_name);
        let jsonl = if trace_viewer_format {
            export.trace_viewer_jsonl
        } else {
            export.jsonl
        };
        std::fs::write(&path, jsonl).map_err(|error| error.to_string())?;
        Ok(DiagnosticExport {
            path: path.to_string_lossy().to_string(),
        })
    }

    pub fn export_diagnostic_logs(&self) -> Result<DiagnosticExport, String> {
        use std::io::Write;

        let log_dir = self.config.data_dir.join("logs");
        std::fs::create_dir_all(&log_dir).map_err(|error| error.to_string())?;
        let out_path = log_dir.join("diagnostic-export.zip");
        let file = std::fs::File::create(&out_path).map_err(|error| error.to_string())?;
        let mut zip = zip::ZipWriter::new(file);
        let opts = zip::write::SimpleFileOptions::default();

        if let Ok(entries) = std::fs::read_dir(&log_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_some_and(|extension| extension == "log") {
                    let name = path.file_name().unwrap_or_default().to_string_lossy();
                    let content = std::fs::read_to_string(&path).unwrap_or_default();
                    zip.start_file(&*name, opts)
                        .map_err(|error| error.to_string())?;
                    zip.write_all(content.as_bytes())
                        .map_err(|error| error.to_string())?;
                }
            }
        }

        let storage_snapshot = match self.project_storage_diagnostics() {
            Ok(snapshot) => {
                serde_json::to_string_pretty(&snapshot).map_err(|error| error.to_string())?
            }
            Err(error) => serde_json::json!({
                "healthy": false,
                "error": error,
            })
            .to_string(),
        };
        zip.start_file("project-storage-diagnostics.json", opts)
            .map_err(|error| error.to_string())?;
        zip.write_all(storage_snapshot.as_bytes())
            .map_err(|error| error.to_string())?;
        zip.finish().map_err(|error| error.to_string())?;
        Ok(DiagnosticExport {
            path: out_path.to_string_lossy().to_string(),
        })
    }

    pub async fn ask_agent(&self, request: AskAgentRequest) -> Result<AskAgentResponse, String> {
        if request.inline_operation {
            return self.run_inline_writer_operation(request).await;
        }

        let api_key = crate::require_api_key()?;
        let settings = crate::llm_runtime::settings(api_key);
        let model = settings.model.clone();
        let truncated_context =
            agent_harness_core::truncate_context(&request.context, 2_000).to_string();
        let manual_observation = manual_writer_observation(&request, &self.project.id);
        let request_id = format!("ask-{}", now_ms());
        let persisted_history = {
            let kernel = self.lock_kernel()?;
            kernel
                .memory
                .list_manual_agent_turns(&self.project.id, 32)
                .map_err(|error| error.to_string())?
        };
        let manual_history = manual_agent_history_messages(&persisted_history, 8, 12_000);
        let (mut prepared_run, emitted_proposals) = {
            let mut kernel = self.lock_kernel()?;
            refresh_kernel_canon_from_headless(&self.config.data_dir, &self.project, &mut kernel);
            let request_packet = WriterAgentRunRequest {
                task: WriterAgentTask::ManualRequest,
                observation: manual_observation.clone(),
                user_instruction: request.message.clone(),
                frontend_state: WriterAgentFrontendState {
                    truncated_context,
                    paragraph: request.paragraph.clone(),
                    selected_text: request.selected_text.clone(),
                    memory_context: String::new(),
                    has_lore: !self.load_lorebook()?.is_empty(),
                    has_outline: !self.load_outline()?.is_empty(),
                },
                approval_mode: WriterAgentApprovalMode::SurfaceProposals,
                stream_mode: WriterAgentStreamMode::Text,
                manual_history,
            };
            let provider = Arc::new(OpenAiCompatProvider::new(
                &settings.api_base,
                &settings.api_key,
                &settings.model,
            ));
            let prepared = kernel.prepare_task_run(
                request_packet,
                provider,
                HeadlessToolBridge::new(&self.config, &self.project),
                &model,
            )?;
            let proposals = prepared.proposals().to_vec();
            (prepared, proposals)
        };

        let mut budget_report = prepared_run
            .first_round_provider_budget(WriterProviderBudgetTask::ManualRequest, model.clone());
        budget_report = apply_provider_budget_approval(
            budget_report,
            request.provider_budget_approval.as_ref(),
        );
        self.record_provider_budget_report(
            format!("manual-request-{}", request_id),
            &budget_report,
            vec!["manual_agent_loop".to_string()],
        );
        if budget_report.approval_required {
            self.record_provider_budget_failure(
                format!("manual-request-{}", request_id),
                &budget_report,
                vec!["manual_agent_loop".to_string()],
            );
            return Ok(AskAgentResponse {
                request_id,
                mode: "chat".to_string(),
                answer: String::new(),
                proposals: emitted_proposals,
                operations: Vec::new(),
                run: None,
                events: Vec::new(),
                provider_budget: Some(budget_report),
            });
        }

        let first_round_estimated_input_tokens = prepared_run.first_round_estimated_input_tokens();
        install_headless_provider_budget_guard(
            &mut prepared_run,
            self.project.id.clone(),
            self.config.data_dir.clone(),
            request_id.clone(),
            manual_observation.clone(),
            request.provider_budget_approval.clone(),
            first_round_estimated_input_tokens,
        );
        prepared_run
            .agent
            .executor
            .set_audit_sink(self.tool_audit_sink(
                format!("manual-request-{}", request_id),
                vec!["manual_agent_loop".to_string()],
            ));
        let events = Arc::new(Mutex::new(Vec::<AgentLoopEvent>::new()));
        let callback_events = events.clone();
        prepared_run.set_event_callback(Arc::new(move |event| {
            if let Ok(mut events) = callback_events.lock() {
                events.push(event);
            }
        }));

        let run_request = prepared_run.request().clone();
        let mut run_result = prepared_run.run().await?;
        {
            let mut kernel = self.lock_kernel()?;
            kernel.record_run_completion(&run_request, &run_result)?;
        }
        let answer = run_result.answer.clone();
        let operations = run_result.operations.clone();
        let proposals = {
            let mut proposals = emitted_proposals;
            proposals.extend(run_result.proposals.clone());
            proposals
        };
        let collected_events = events
            .lock()
            .map_err(|_| "Agent event log lock poisoned".to_string())?
            .clone();
        run_result.proposals = proposals.clone();
        Ok(AskAgentResponse {
            request_id,
            mode: "chat".to_string(),
            answer,
            proposals,
            operations,
            run: Some(run_result),
            events: collected_events,
            provider_budget: Some(budget_report),
        })
    }

    async fn run_inline_writer_operation(
        &self,
        request: AskAgentRequest,
    ) -> Result<AskAgentResponse, String> {
        let api_key = crate::require_api_key()?;
        let settings = crate::llm_runtime::settings(api_key);
        let model = settings.model.clone();
        let request_id = format!("inline-{}", now_ms());
        let observation = manual_writer_observation(&request, &self.project.id);
        let (context_pack, local_proposals) = {
            let mut kernel = self.lock_kernel()?;
            refresh_kernel_canon_from_headless(&self.config.data_dir, &self.project, &mut kernel);
            let local_proposals = kernel.observe(observation.clone())?;
            let context_pack = kernel.context_pack_for_default(
                crate::writer_agent::context::AgentTask::InlineRewrite,
                &observation,
            );
            (context_pack, local_proposals)
        };
        let messages =
            writer_agent_inline_operation_messages(&request.message, &observation, &context_pack);
        let draft = crate::llm_runtime::chat_text_profile(
            &settings,
            messages,
            crate::llm_runtime::LlmRequestProfile::ManualRewrite,
            30,
        )
        .await?;
        let (proposal, operation) = {
            let mut kernel = self.lock_kernel()?;
            let proposal = kernel.create_inline_operation_proposal(
                observation,
                &request.message,
                draft.clone(),
                &model,
            )?;
            let operation = proposal.operations.first().cloned().ok_or_else(|| {
                "inline operation proposal did not include an operation".to_string()
            })?;
            (proposal, operation)
        };
        let mut proposals = local_proposals;
        proposals.push(proposal);
        Ok(AskAgentResponse {
            request_id,
            mode: "inline_operation".to_string(),
            answer: draft,
            proposals,
            operations: vec![operation],
            run: None,
            events: Vec::new(),
            provider_budget: None,
        })
    }

    pub fn observe(
        &self,
        mut observation: WriterObservation,
    ) -> Result<Vec<crate::writer_agent::proposal::AgentProposal>, String> {
        observation.project_id = self.project.id.clone();
        let mut kernel = self.lock_kernel()?;
        kernel.observe(observation)
    }

    pub fn ledger_snapshot(
        &self,
    ) -> Result<crate::writer_agent::kernel::WriterAgentLedgerSnapshot, String> {
        Ok(self.lock_kernel()?.ledger_snapshot())
    }

    pub fn pending_proposals(
        &self,
    ) -> Result<Vec<crate::writer_agent::proposal::AgentProposal>, String> {
        Ok(self.lock_kernel()?.pending_proposals())
    }

    pub fn story_review_queue(
        &self,
    ) -> Result<Vec<crate::writer_agent::kernel::StoryReviewQueueEntry>, String> {
        Ok(self.lock_kernel()?.story_review_queue())
    }

    pub fn story_debt_snapshot(
        &self,
    ) -> Result<crate::writer_agent::kernel::StoryDebtSnapshot, String> {
        Ok(self.lock_kernel()?.story_debt_snapshot())
    }

    pub fn today_five_summary(
        &self,
    ) -> Result<crate::writer_agent::kernel::TodayFiveSummary, String> {
        Ok(self.lock_kernel()?.today_five_summary())
    }

    pub fn reader_compensation_review_chain(
        &self,
    ) -> Result<crate::writer_agent::kernel::ReaderCompensationReviewChain, String> {
        Ok(self.lock_kernel()?.reader_compensation_review_chain())
    }

    pub fn trace_snapshot(
        &self,
        limit: usize,
    ) -> Result<crate::writer_agent::kernel::WriterAgentTraceSnapshot, String> {
        Ok(self.lock_kernel()?.trace_snapshot(limit))
    }

    pub fn inspector_timeline(
        &self,
        limit: usize,
    ) -> Result<crate::writer_agent::kernel::WriterInspectorTimeline, String> {
        Ok(self.lock_kernel()?.inspector_timeline(limit))
    }

    pub fn companion_timeline_summary(
        &self,
    ) -> Result<crate::writer_agent::kernel::WriterInspectorTimeline, String> {
        Ok(self.lock_kernel()?.companion_timeline_summary())
    }

    pub fn apply_feedback(&self, feedback: ProposalFeedback) -> Result<(), String> {
        self.lock_kernel()?.apply_feedback(feedback)
    }

    pub fn record_implicit_ghost_rejection(
        &self,
        proposal_id: String,
        created_at: u64,
    ) -> Result<bool, String> {
        self.lock_kernel()?
            .record_implicit_ghost_rejection(&proposal_id, created_at)
    }

    pub fn approve_writer_operation(
        &self,
        operation: WriterOperation,
        current_revision: String,
        approval: Option<OperationApproval>,
    ) -> Result<OperationResult, String> {
        if let WriterOperation::OutlineUpdate { node_id, patch } = operation.clone() {
            {
                let mut kernel = self.lock_kernel()?;
                let preflight = kernel.approve_editor_operation_with_approval(
                    operation.clone(),
                    &current_revision,
                    approval.as_ref(),
                )?;
                if preflight
                    .error
                    .as_ref()
                    .is_none_or(|error| error.code != "invalid")
                {
                    return Ok(preflight);
                }
            }
            return self.approve_outline_update_operation(operation, node_id, patch, approval);
        }

        self.lock_kernel()?.approve_editor_operation_with_approval(
            operation,
            &current_revision,
            approval.as_ref(),
        )
    }

    fn approve_outline_update_operation(
        &self,
        operation: WriterOperation,
        node_id: String,
        patch: serde_json::Value,
        approval: Option<OperationApproval>,
    ) -> Result<OperationResult, String> {
        if !approval
            .as_ref()
            .is_some_and(OperationApproval::is_valid_for_write)
        {
            return Ok(OperationResult {
                success: false,
                operation,
                error: Some(OperationError::approval_required(
                    "outline.update requires an explicit surfaced approval context",
                )),
                revision_after: None,
            });
        }

        let result = match self.patch_outline_node(node_id, patch) {
            Ok(_) => OperationResult {
                success: true,
                operation: operation.clone(),
                error: None,
                revision_after: None,
            },
            Err(error) => OperationResult {
                success: false,
                operation: operation.clone(),
                error: Some(OperationError::invalid(&error)),
                revision_after: None,
            },
        };

        let mut kernel = self.lock_kernel()?;
        if !result.success {
            let save_result = result
                .error
                .as_ref()
                .map(|error| format!("{}:{}", error.code, error.message))
                .unwrap_or_else(|| "outline_storage:failed".to_string());
            kernel.record_operation_durable_save(
                approval
                    .as_ref()
                    .and_then(|context| context.proposal_id.clone()),
                operation,
                save_result,
            )?;
            return Ok(result);
        }

        if let Some(context) = approval
            .as_ref()
            .filter(|context| context.is_valid_for_write())
        {
            kernel.record_operation_durable_save(
                context.proposal_id.clone(),
                operation,
                "outline_storage:ok".to_string(),
            )?;
        }
        Ok(result)
    }

    fn patch_outline_node(
        &self,
        chapter_title: String,
        patch: serde_json::Value,
    ) -> Result<Vec<OutlineNode>, String> {
        let mut nodes = self.load_outline()?;
        apply_outline_patch(&mut nodes, &chapter_title, &patch)?;
        write_json_pretty(
            &outline_path(&self.config.data_dir, &self.project.id)?,
            &nodes,
        )?;
        self.reseed_story_model();
        Ok(nodes)
    }

    pub fn record_writer_operation_durable_save(
        &self,
        proposal_id: Option<String>,
        operation: WriterOperation,
        save_result: String,
        saved_content: Option<String>,
        chapter_title: Option<String>,
        chapter_revision: Option<String>,
    ) -> Result<(), String> {
        let saved_text = saved_content.map(|content| html_to_plain_text(&content));
        self.lock_kernel()?
            .record_operation_durable_save_with_post_write(
                proposal_id,
                operation,
                save_result,
                saved_text,
                chapter_title,
                chapter_revision,
            )
    }

    pub fn ambient_entity_hints(
        &self,
        paragraph: String,
        chapter: String,
    ) -> Result<Vec<serde_json::Value>, String> {
        let kernel = self.lock_kernel()?;
        let names = kernel
            .ledger_snapshot()
            .canon_entities
            .iter()
            .map(|entity| entity.name.clone())
            .collect::<Vec<_>>();
        let mut hints = Vec::new();
        for name in names {
            if name.len() < 2 || !paragraph.contains(&name) {
                continue;
            }
            if hints
                .iter()
                .any(|hint: &serde_json::Value| hint["keyword"].as_str() == Some(&name))
            {
                continue;
            }
            let facts = kernel
                .memory
                .get_canon_facts_for_entity(&name)
                .unwrap_or_default();
            let content = if facts.is_empty() {
                "Canon entity".to_string()
            } else {
                facts
                    .iter()
                    .take(3)
                    .map(|(key, value)| format!("{}: {}", key, value))
                    .collect::<Vec<_>>()
                    .join(" · ")
            };
            hints.push(serde_json::json!({
                "keyword": name,
                "content": content,
                "chapter": chapter,
            }));
        }
        Ok(hints)
    }

    pub fn start_sprint(
        &self,
        request: StartSprintRequest,
    ) -> Result<SupervisedSprintPlan, String> {
        if request.chapter_titles.is_empty() {
            return Err("supervised sprint requires at least one chapter".to_string());
        }
        let sprint_id = format!("sprint-{}", now_ms());
        let plan = create_sprint_plan_with_limits(
            &sprint_id,
            &request.chapter_titles,
            request.require_approval_per_chapter,
            request
                .max_chapters_per_session
                .unwrap_or(request.chapter_titles.len())
                .max(1),
            request.budget_ceiling_micros,
        );
        self.persist_sprint_plan(&plan)?;
        *self.lock_sprint()? = Some(plan.clone());
        Ok(plan)
    }

    pub fn sprint_plan(&self) -> Result<Option<SupervisedSprintPlan>, String> {
        Ok(self.lock_sprint()?.clone())
    }

    pub fn sprint_progress(&self) -> Result<Option<SprintProgress>, String> {
        Ok(self.lock_sprint()?.as_ref().map(sprint_progress))
    }

    pub fn pause_sprint(&self) -> Result<Option<SprintProgress>, String> {
        self.mutate_sprint(|plan| {
            pause_sprint(plan);
            Ok(Some(sprint_progress(plan)))
        })
    }

    pub fn resume_sprint(&self) -> Result<Option<SprintProgress>, String> {
        self.mutate_sprint(|plan| {
            resume_sprint(plan);
            Ok(Some(sprint_progress(plan)))
        })
    }

    pub fn cancel_sprint(&self) -> Result<Option<SprintProgress>, String> {
        self.mutate_sprint(|plan| {
            cancel_sprint(plan);
            Ok(Some(sprint_progress(plan)))
        })
    }

    pub fn checkpoint_sprint(&self, source: String) -> Result<Option<SprintCheckpoint>, String> {
        let output = self.mutate_sprint(|plan| Ok(Some(checkpoint_sprint(plan, &source))))?;
        if let Some(checkpoint) = output.as_ref() {
            let kernel = self.lock_kernel()?;
            kernel
                .memory
                .insert_supervised_sprint_checkpoint(&self.project.id, checkpoint)
                .map_err(|error| error.to_string())?;
        }
        Ok(output)
    }

    pub fn record_sprint_budget_usage(
        &self,
        spent_micros: u64,
    ) -> Result<Option<SprintProgress>, String> {
        self.mutate_sprint(|plan| {
            record_budget_usage(plan, spent_micros);
            Ok(Some(sprint_progress(plan)))
        })
    }

    pub fn set_sprint_quality_gate(
        &self,
        minimum_quality_score: Option<f32>,
        stop_on_fatal_issue: Option<bool>,
    ) -> Result<Option<SprintProgress>, String> {
        self.mutate_sprint(|plan| {
            set_sprint_quality_gate(plan, minimum_quality_score, stop_on_fatal_issue);
            Ok(Some(sprint_progress(plan)))
        })
    }

    pub fn project_graph_data(&self) -> Result<HeadlessProjectGraphData, String> {
        let lore_entries = self.load_lorebook()?;
        let outline = self.load_outline()?;
        let mut entities = lore_entries
            .into_iter()
            .map(|entry| HeadlessGraphEntity {
                id: format!("lore-{}", entry.id),
                name: entry.keyword,
                category: "character".to_string(),
                description: entry.content,
            })
            .collect::<Vec<_>>();

        {
            let kernel = self.lock_kernel()?;
            for entity in kernel.ledger_snapshot().canon_entities {
                if entities.iter().any(|existing| existing.name == entity.name) {
                    continue;
                }
                entities.push(HeadlessGraphEntity {
                    id: format!("canon-{}", entity.name),
                    name: entity.name,
                    category: entity.kind,
                    description: entity.summary,
                });
            }
        }

        let mut chapters = outline
            .into_iter()
            .map(|node| {
                let path =
                    chapter_path(&self.config.data_dir, &self.project.id, &node.chapter_title);
                let word_count = path
                    .ok()
                    .and_then(|path| std::fs::read_to_string(path).ok())
                    .map(|content| content.split_whitespace().count())
                    .unwrap_or(0);
                HeadlessGraphChapter {
                    title: node.chapter_title,
                    summary: node.summary,
                    status: node.status,
                    word_count,
                }
            })
            .collect::<Vec<_>>();

        if chapters.is_empty() {
            for chapter in self.list_chapters()? {
                let content = self.load_chapter(chapter.title.clone()).unwrap_or_default();
                chapters.push(HeadlessGraphChapter {
                    title: chapter.title,
                    summary: String::new(),
                    status: "empty".to_string(),
                    word_count: content.split_whitespace().count(),
                });
            }
        }

        let entity_names = entities
            .iter()
            .map(|entity| entity.name.clone())
            .collect::<Vec<_>>();
        let mut relationships = Vec::new();
        for chapter in &chapters {
            let content = self
                .load_chapter(chapter.title.clone())
                .unwrap_or_default()
                .to_lowercase();
            let found = entity_names
                .iter()
                .filter(|name| content.contains(&name.to_lowercase()))
                .collect::<Vec<_>>();
            for i in 0..found.len() {
                for j in i + 1..found.len() {
                    let exists =
                        relationships
                            .iter()
                            .any(|relationship: &HeadlessGraphRelationship| {
                                (relationship.source == *found[i]
                                    && relationship.target == *found[j])
                                    || (relationship.source == *found[j]
                                        && relationship.target == *found[i])
                            });
                    if !exists {
                        relationships.push(HeadlessGraphRelationship {
                            source: found[i].clone(),
                            target: found[j].clone(),
                            label: format!("Co-occur in {}", chapter.title),
                        });
                    }
                }
            }
        }

        Ok(HeadlessProjectGraphData {
            entities,
            relationships,
            chapters,
        })
    }

    pub fn project_storage_diagnostics(
        &self,
    ) -> Result<storage::ProjectStorageDiagnostics, String> {
        let project_data_dir = project_data_dir(&self.config.data_dir, &self.project.id)?;
        let chapters_dir = chapters_dir(&self.config.data_dir, &self.project.id)?;
        let files = vec![
            diagnose_json_array_file::<LoreEntry>(
                "lorebook",
                &lorebook_path(&self.config.data_dir, &self.project.id)?,
            ),
            diagnose_json_array_file::<OutlineNode>(
                "outline",
                &outline_path(&self.config.data_dir, &self.project.id)?,
            ),
            diagnose_json_array_file::<agent_harness_core::vector_db::Chunk>(
                "project_brain",
                &project_data_dir.join("project_brain.json"),
            ),
            diagnose_chapters_directory(&chapters_dir),
        ];
        let databases = vec![diagnose_sqlite_database(
            "writer_memory",
            &project_data_dir.join(storage::WRITER_MEMORY_DB_FILENAME),
            &[
                "story_contracts",
                "chapter_missions",
                "chapter_result_snapshots",
                "canon_entities",
                "canon_facts",
                "canon_rules",
                "plot_promises",
                "style_preferences",
                "creative_decisions",
                "proposal_feedback",
                "memory_audit_events",
                "manual_agent_turns",
                "writer_observation_trace",
                "writer_proposal_trace",
                "writer_feedback_trace",
            ],
        )];
        let healthy = files.iter().all(|file| file.status != "error")
            && databases.iter().all(|database| database.status == "ok");
        Ok(storage::ProjectStorageDiagnostics {
            project_id: self.project.id.clone(),
            project_name: self.project.name.clone(),
            app_data_dir: self.config.data_dir.to_string_lossy().to_string(),
            project_data_dir: project_data_dir.to_string_lossy().to_string(),
            checked_at: now_ms(),
            healthy,
            files,
            databases,
        })
    }

    pub fn list_file_backups(
        &self,
        target: storage::BackupTarget,
    ) -> Result<Vec<storage::FileBackupInfo>, String> {
        let target_path = self.backup_target_path(&target)?;
        let backup_dir = backup_dir_for(&target_path)?;
        if !backup_dir.exists() {
            return Ok(Vec::new());
        }
        let mut backups = std::fs::read_dir(&backup_dir)
            .map_err(|error| {
                format!(
                    "Failed to read backup dir '{}': {}",
                    backup_dir.display(),
                    error
                )
            })?
            .flatten()
            .filter(|entry| entry.path().is_file())
            .filter_map(|entry| backup_info(entry).ok())
            .collect::<Vec<_>>();
        backups.sort_by_key(|backup| std::cmp::Reverse(backup.modified_at));
        Ok(backups)
    }

    pub fn restore_file_backup(
        &self,
        target: storage::BackupTarget,
        backup_id: String,
    ) -> Result<(), String> {
        let target_path = self.backup_target_path(&target)?;
        let backup_dir = backup_dir_for(&target_path)?;
        let backup_path = safe_backup_file_path(&backup_dir, &backup_id)?;
        if !backup_path.exists() {
            return Err(format!("Backup '{}' not found", backup_id));
        }
        let content = std::fs::read_to_string(&backup_path).map_err(|error| {
            format!(
                "Failed to read backup '{}': {}",
                backup_path.display(),
                error
            )
        })?;
        validate_backup_content(&target, &content, &backup_path)?;
        storage::atomic_write(&target_path, &content)
    }

    pub fn set_api_key(&self, provider: String, key: String) -> Result<(), String> {
        crate::api_key::store_api_key(&provider, &key)
    }

    pub fn check_api_key(&self, provider: String) -> bool {
        crate::api_key::has_api_key_for_provider(&provider)
    }

    fn record_provider_budget_report(
        &self,
        task_id: String,
        report: &WriterProviderBudgetReport,
        source_refs: Vec<String>,
    ) {
        if let Ok(mut kernel) = self.lock_kernel() {
            kernel.record_provider_budget_report(task_id, report, source_refs, now_ms());
        }
    }

    fn record_provider_budget_failure(
        &self,
        task_id: String,
        report: &WriterProviderBudgetReport,
        source_refs: Vec<String>,
    ) {
        if let Ok(mut kernel) = self.lock_kernel() {
            let bundle = crate::writer_agent::task_receipt::WriterFailureEvidenceBundle::new(
                crate::writer_agent::task_receipt::WriterFailureCategory::ProviderFailed,
                "HEADLESS_PROVIDER_BUDGET_APPROVAL_REQUIRED",
                "Headless Writer Agent provider budget requires explicit approval before calling the model.",
                true,
                Some(task_id),
                source_refs,
                serde_json::json!({ "providerBudget": report }),
                vec![
                    "Approve the provider budget and retry, or narrow the request context."
                        .to_string(),
                ],
                now_ms(),
            );
            kernel.record_failure_evidence_bundle(&bundle);
        }
    }

    fn tool_audit_sink(&self, task_id: String, source_refs: Vec<String>) -> ToolExecutionAuditSink {
        let config = self.config.clone();
        let project = self.project.clone();
        Arc::new(move |event| {
            let Ok(memory_path) = writer_memory_path(&config.data_dir, &project.id) else {
                return;
            };
            let Ok(memory) = WriterMemory::open(&memory_path) else {
                return;
            };
            let mut kernel = WriterAgentKernel::new(&project.id, memory);
            match event {
                ToolExecutionAuditEvent::Start { tool_name, input } => {
                    let mut refs = source_refs.clone();
                    refs.push(format!("tool:{}", tool_name));
                    kernel.record_tool_called_run_event(
                        task_id.clone(),
                        tool_name,
                        "start",
                        Some(&input),
                        None,
                        refs,
                        now_ms(),
                    );
                }
                ToolExecutionAuditEvent::End { execution } => {
                    let mut refs = source_refs.clone();
                    refs.push(format!("tool:{}", execution.tool_name));
                    kernel.record_tool_called_run_event(
                        task_id.clone(),
                        execution.tool_name.clone(),
                        "end",
                        Some(&execution.input),
                        Some(&execution),
                        refs,
                        now_ms(),
                    );
                }
            }
        })
    }

    fn backup_target_path(&self, target: &storage::BackupTarget) -> Result<PathBuf, String> {
        match target {
            storage::BackupTarget::Lorebook => {
                lorebook_path(&self.config.data_dir, &self.project.id)
            }
            storage::BackupTarget::Outline => outline_path(&self.config.data_dir, &self.project.id),
            storage::BackupTarget::ProjectBrain => {
                Ok(project_data_dir(&self.config.data_dir, &self.project.id)?
                    .join("project_brain.json"))
            }
            storage::BackupTarget::Chapter { title } => {
                chapter_path(&self.config.data_dir, &self.project.id, title)
            }
        }
    }

    pub fn dispatch(
        &self,
        action: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        match action {
            "project_manifest" => to_value(self.project()),
            "project_paths" => to_value(self.paths()?),
            "status" => to_value(self.status()?),
            "agent_tools" => to_value(self.agent_tools()),
            "effective_agent_tool_inventory" => to_value(self.effective_agent_tool_inventory()),
            "agent_kernel_status" => to_value(self.agent_kernel_status()),
            "agent_domain_profile" => to_value(self.agent_domain_profile()),
            "list_chapters" => to_value(self.list_chapters()?),
            "create_chapter" => {
                let title = required_string(&params, "title")?;
                to_value(self.create_chapter(title)?)
            }
            "load_chapter" => {
                let title = required_string(&params, "title")?;
                to_value(self.load_chapter(title)?)
            }
            "save_chapter" => {
                let title = required_string(&params, "title")?;
                let content = required_string(&params, "content")?;
                to_value(self.save_chapter(title, content)?)
            }
            "chapter_revision" => {
                let title = required_string(&params, "title")?;
                to_value(self.chapter_revision(title)?)
            }
            "rename_chapter_file" => {
                let old_name = required_string_any(&params, &["oldName", "old_name"])?;
                let new_name = required_string_any(&params, &["newName", "new_name"])?;
                self.rename_chapter_file(old_name, new_name)?;
                to_value(serde_json::json!({ "ok": true }))
            }
            "load_lorebook" => to_value(self.load_lorebook()?),
            "save_lore_entry" => {
                let keyword = required_string(&params, "keyword")?;
                let content = required_string(&params, "content")?;
                to_value(self.save_lore_entry(keyword, content)?)
            }
            "delete_lore_entry" => {
                let id = required_string(&params, "id")?;
                to_value(self.delete_lore_entry(id)?)
            }
            "load_outline" => to_value(self.load_outline()?),
            "save_outline_node" => {
                let chapter_title = required_string(&params, "chapterTitle")
                    .or_else(|_| required_string(&params, "chapter_title"))?;
                let summary = required_string(&params, "summary")?;
                let status = params
                    .get("status")
                    .and_then(|value| value.as_str())
                    .map(ToOwned::to_owned);
                to_value(self.save_outline_node(chapter_title, summary, status)?)
            }
            "delete_outline_node" => {
                let chapter_title =
                    required_string_any(&params, &["chapterTitle", "chapter_title"])?;
                to_value(self.delete_outline_node(chapter_title)?)
            }
            "update_outline_status" => {
                let chapter_title =
                    required_string_any(&params, &["chapterTitle", "chapter_title"])?;
                let status = required_string(&params, "status")?;
                to_value(self.update_outline_status(chapter_title, status)?)
            }
            "reorder_outline_nodes" => {
                let ordered_titles =
                    string_array_any(&params, &["orderedTitles", "ordered_titles"])?;
                to_value(self.reorder_outline_nodes(ordered_titles)?)
            }
            "list_volumes" => to_value(self.list_volumes()?),
            "save_volume" => {
                let volume = serde_json::from_value(params)
                    .map_err(|error| format!("Invalid volume: {}", error))?;
                to_value(self.save_volume(volume)?)
            }
            "delete_volume" => {
                let volume_id = required_string_any(&params, &["volumeId", "volume_id"])?;
                to_value(self.delete_volume(volume_id)?)
            }
            "get_volume_snapshot" => {
                let volume_id = required_string_any(&params, &["volumeId", "volume_id"])?;
                to_value(self.get_volume_snapshot(volume_id)?)
            }
            "save_volume_snapshot" => {
                let snapshot = serde_json::from_value(params)
                    .map_err(|error| format!("Invalid volume snapshot: {}", error))?;
                to_value(self.save_volume_snapshot(snapshot)?)
            }
            "get_book_state" => to_value(self.get_book_state()?),
            "save_book_state" => {
                let book_state = serde_json::from_value(params)
                    .map_err(|error| format!("Invalid book state: {}", error))?;
                to_value(self.save_book_state(book_state)?)
            }
            "repair_chapter_state" => {
                let chapter_title =
                    required_string_any(&params, &["chapterTitle", "chapter_title"])?;
                to_value(self.repair_chapter_state(chapter_title)?)
            }
            "get_project_brain_knowledge_graph" => {
                to_value(self.get_project_brain_knowledge_graph()?)
            }
            "compare_project_brain_source_revisions" => {
                let source_ref = required_string_any(&params, &["sourceRef", "source_ref"])?;
                to_value(self.compare_project_brain_source_revisions(source_ref)?)
            }
            "restore_project_brain_source_revision" => {
                let source_ref = required_string_any(&params, &["sourceRef", "source_ref"])?;
                let revision = required_string(&params, "revision")?;
                to_value(self.restore_project_brain_source_revision(source_ref, revision)?)
            }
            "cross_reference_brain_nodes" => {
                let source_node_id =
                    required_string_any(&params, &["sourceNodeId", "source_node_id"])?;
                let target_node_id =
                    required_string_any(&params, &["targetNodeId", "target_node_id"])?;
                to_value(self.cross_reference_brain_nodes(source_node_id, target_node_id)?)
            }
            "ingest_external_research" => {
                let request = serde_json::from_value(params)
                    .map_err(|error| format!("Invalid external research request: {}", error))?;
                to_value(self.ingest_external_research(request)?)
            }
            "observe" => {
                let observation = serde_json::from_value(params)
                    .map_err(|error| format!("Invalid observation: {}", error))?;
                to_value(self.observe(observation)?)
            }
            "ledger" => to_value(self.ledger_snapshot()?),
            "pending_proposals" => to_value(self.pending_proposals()?),
            "story_review_queue" => to_value(self.story_review_queue()?),
            "story_debt" => to_value(self.story_debt_snapshot()?),
            "today_five" => to_value(self.today_five_summary()?),
            "reader_compensation_review_chain" => {
                to_value(self.reader_compensation_review_chain()?)
            }
            "trace" => {
                let limit = optional_usize(&params, "limit").unwrap_or(50);
                to_value(self.trace_snapshot(limit)?)
            }
            "inspector_timeline" => {
                let limit = optional_usize(&params, "limit").unwrap_or(50);
                to_value(self.inspector_timeline(limit)?)
            }
            "companion_timeline_summary" => to_value(self.companion_timeline_summary()?),
            "apply_feedback" => {
                let feedback = serde_json::from_value(params)
                    .map_err(|error| format!("Invalid feedback: {}", error))?;
                self.apply_feedback(feedback)?;
                to_value(serde_json::json!({ "ok": true }))
            }
            "record_implicit_ghost_rejection" => {
                let proposal_id = required_string_any(&params, &["proposalId", "proposal_id"])?;
                let created_at = required_u64_any(&params, &["createdAt", "created_at"])?;
                to_value(self.record_implicit_ghost_rejection(proposal_id, created_at)?)
            }
            "approve_writer_operation" => {
                let operation = params
                    .get("operation")
                    .cloned()
                    .ok_or_else(|| "operation is required".to_string())
                    .and_then(|value| {
                        serde_json::from_value(value)
                            .map_err(|error| format!("Invalid operation: {}", error))
                    })?;
                let current_revision =
                    required_string_any(&params, &["currentRevision", "current_revision"])?;
                let approval = params
                    .get("approval")
                    .cloned()
                    .filter(|value| !value.is_null())
                    .map(serde_json::from_value)
                    .transpose()
                    .map_err(|error| format!("Invalid approval: {}", error))?;
                to_value(self.approve_writer_operation(operation, current_revision, approval)?)
            }
            "record_writer_operation_durable_save" => {
                let proposal_id = optional_string_any(&params, &["proposalId", "proposal_id"]);
                let operation = params
                    .get("operation")
                    .cloned()
                    .ok_or_else(|| "operation is required".to_string())
                    .and_then(|value| {
                        serde_json::from_value(value)
                            .map_err(|error| format!("Invalid operation: {}", error))
                    })?;
                let save_result = required_string_any(&params, &["saveResult", "save_result"])?;
                let saved_content =
                    optional_string_any(&params, &["savedContent", "saved_content"]);
                let chapter_title =
                    optional_string_any(&params, &["chapterTitle", "chapter_title"]);
                let chapter_revision =
                    optional_string_any(&params, &["chapterRevision", "chapter_revision"]);
                self.record_writer_operation_durable_save(
                    proposal_id,
                    operation,
                    save_result,
                    saved_content,
                    chapter_title,
                    chapter_revision,
                )?;
                to_value(serde_json::json!({ "ok": true }))
            }
            "ambient_entity_hints" => {
                let paragraph = required_string(&params, "paragraph")?;
                let chapter = required_string(&params, "chapter")?;
                to_value(self.ambient_entity_hints(paragraph, chapter)?)
            }
            "craft_library" => {
                let rules = crate::chapter_generation::craft_library_for_stats();
                to_value(
                    serde_json::to_value(rules)
                        .map_err(|e| format!("Failed to serialize craft library: {}", e))?,
                )
            }
            "craft_memory_stats" => {
                let memory_path = writer_memory_path(&self.config.data_dir, &self.project.id)?;
                let conn = rusqlite::Connection::open_with_flags(
                    &memory_path,
                    rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
                )
                .map_err(|e| format!("craft_memory_stats: {}", e))?;
                let rule_id = params.get("ruleId").and_then(|v| v.as_str());
                let library = crate::chapter_generation::craft_library_for_stats();
                let stats: Vec<serde_json::Value> = library
                    .iter()
                    .filter(|r| rule_id.is_none_or(|id| r.id == id))
                    .filter_map(|r| {
                        crate::writer_agent::memory::get_craft_rule_stats(&conn, &r.id).map(|s| {
                            let examples =
                                crate::writer_agent::memory::list_craft_examples(&conn, &r.id, 3)
                                    .unwrap_or_default();
                            let bad_patterns =
                                crate::writer_agent::memory::list_craft_bad_patterns(
                                    &conn, &r.id, 3,
                                )
                                .unwrap_or_default();
                            serde_json::json!({
                                "ruleId": s.rule_id,
                                "acceptedCount": s.accepted_count,
                                "rejectedCount": s.rejected_count,
                                "acceptanceRate": s.acceptance_rate(),
                                "examples": examples,
                                "badPatterns": bad_patterns,
                            })
                        })
                    })
                    .collect();
                to_value(serde_json::json!({
                    "rules": stats,
                    "totalRules": library.len(),
                }))
            }
            "record_manual_craft_edit_feedback" => {
                let request: crate::chapter_generation::ManualCraftEditFeedbackRequest =
                    serde_json::from_value(params).map_err(|error| {
                        format!("Invalid manual craft edit feedback request: {}", error)
                    })?;
                to_value(self.record_manual_craft_edit_feedback(request)?)
            }
            "chapter_quality_report" => {
                let chapter_text = required_string(&params, "chapterText")?;
                let chapter_title = required_string(&params, "chapterTitle")?;
                let min_chars = params
                    .get("targetMinChars")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(3000) as usize;
                let max_chars = params
                    .get("targetMaxChars")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(4000) as usize;
                let plan = crate::chapter_generation::SceneCraftPlan::default();
                let report = crate::chapter_generation::evaluate_chapter_quality(
                    &chapter_text,
                    &chapter_title,
                    &plan,
                    &[],
                    min_chars,
                    max_chars,
                );
                to_value(
                    serde_json::to_value(&report)
                        .map_err(|e| format!("Failed to serialize quality report: {}", e))?,
                )
            }
            "context_quality_report" => {
                let chapter_title = required_string(&params, "chapterTitle")?;
                let runtime_dir = project_data_dir(&self.config.data_dir, &self.project.id)
                    .map(|p| p.join("chapter_runtime"))
                    .unwrap_or_default();

                let mut report_files: Vec<_> = std::fs::read_dir(&runtime_dir)
                    .ok()
                    .into_iter()
                    .flatten()
                    .filter_map(|entry| entry.ok())
                    .filter(|entry| {
                        let name = entry.file_name().to_string_lossy().to_string();
                        name.contains(chapter_title.as_str())
                            && name.ends_with(".context_quality.json")
                    })
                    .map(|entry| {
                        let modified = entry.metadata().ok().and_then(|m| m.modified().ok());
                        (entry.path(), modified)
                    })
                    .collect();
                report_files.sort_by_key(|(_, modified)| std::cmp::Reverse(*modified));

                if let Some((path, _)) = report_files.first() {
                    let content = std::fs::read_to_string(path)
                        .map_err(|e| format!("Failed to read context quality report: {}", e))?;
                    let report: ContextQualityReport = serde_json::from_str(&content)
                        .map_err(|e| format!("Failed to parse context quality report: {}", e))?;
                    to_value(serde_json::to_value(&report).map_err(|e| {
                        format!("Failed to serialize context quality report: {}", e)
                    })?)
                } else {
                    to_value(serde_json::json!({
                        "chapterTitle": chapter_title,
                        "note": "No context quality report found. Generate a chapter first."
                    }))
                }
            }
            "budget_calibration" => {
                let budget = crate::chapter_generation::ChapterContextBudget::default();
                to_value(
                    serde_json::to_value(&budget)
                        .map_err(|e| format!("Failed to serialize budget: {}", e))?,
                )
            }
            "execution_plan" => to_value(serde_json::json!({
                "status": "idle",
                "strategy": "interactive_safe_draft",
                "note": "execution_plan reflects latest pipeline invocation; no active run"
            })),
            "start_sprint" => {
                let request = serde_json::from_value(params)
                    .map_err(|error| format!("Invalid sprint request: {}", error))?;
                to_value(self.start_sprint(request)?)
            }
            "sprint_plan" => to_value(self.sprint_plan()?),
            "sprint_progress" => to_value(self.sprint_progress()?),
            "pause_sprint" => to_value(self.pause_sprint()?),
            "resume_sprint" => to_value(self.resume_sprint()?),
            "cancel_sprint" => to_value(self.cancel_sprint()?),
            "checkpoint_sprint" => {
                let source = params
                    .get("source")
                    .and_then(|value| value.as_str())
                    .unwrap_or("mcp")
                    .to_string();
                to_value(self.checkpoint_sprint(source)?)
            }
            "record_sprint_budget_usage" => {
                let spent_micros = params
                    .get("spentMicros")
                    .or_else(|| params.get("spent_micros"))
                    .and_then(|value| value.as_u64())
                    .ok_or_else(|| "spentMicros is required".to_string())?;
                to_value(self.record_sprint_budget_usage(spent_micros)?)
            }
            "set_sprint_quality_gate" => {
                let minimum_quality_score = params
                    .get("minimumQualityScore")
                    .or_else(|| params.get("minimum_quality_score"))
                    .and_then(|v| v.as_f64())
                    .map(|f| f as f32);
                let stop_on_fatal = params
                    .get("stopOnFatalIssue")
                    .or_else(|| params.get("stop_on_fatal_issue"))
                    .and_then(|v| v.as_bool());
                to_value(self.set_sprint_quality_gate(minimum_quality_score, stop_on_fatal)?)
            }
            "project_graph_data" => to_value(self.project_graph_data()?),
            "project_storage_diagnostics" => to_value(self.project_storage_diagnostics()?),
            "export_writer_agent_trajectory" => {
                let limit = optional_usize(&params, "limit");
                let format = optional_string_any(&params, &["format"]);
                to_value(self.export_writer_agent_trajectory(limit, format)?)
            }
            "export_diagnostic_logs" => to_value(self.export_diagnostic_logs()?),
            "list_file_backups" => {
                let target_value = params
                    .get("target")
                    .cloned()
                    .unwrap_or_else(|| params.clone());
                let target = serde_json::from_value(target_value)
                    .map_err(|error| format!("Invalid backup target: {}", error))?;
                to_value(self.list_file_backups(target)?)
            }
            "restore_file_backup" => {
                let target_value = params
                    .get("target")
                    .cloned()
                    .unwrap_or_else(|| params.clone());
                let target = serde_json::from_value(target_value)
                    .map_err(|error| format!("Invalid backup target: {}", error))?;
                let backup_id = required_string_any(&params, &["backupId", "backup_id"])?;
                self.restore_file_backup(target, backup_id)?;
                to_value(serde_json::json!({ "ok": true }))
            }
            "set_api_key" => {
                let provider = required_string(&params, "provider")?;
                let key = required_string(&params, "key")?;
                self.set_api_key(provider, key)?;
                to_value(serde_json::json!({ "ok": true }))
            }
            "check_api_key" => {
                let provider = required_string(&params, "provider")?;
                to_value(self.check_api_key(provider))
            }
            other => Err(format!("Unknown backend action '{}'", other)),
        }
    }

    fn lock_kernel(&self) -> Result<MutexGuard<'_, WriterAgentKernel>, String> {
        self.kernel
            .lock()
            .map_err(|_| "Writer kernel lock poisoned".to_string())
    }

    fn lock_sprint(&self) -> Result<MutexGuard<'_, Option<SupervisedSprintPlan>>, String> {
        self.current_sprint
            .lock()
            .map_err(|_| "Supervised sprint lock poisoned".to_string())
    }

    fn persist_sprint_plan(&self, plan: &SupervisedSprintPlan) -> Result<(), String> {
        let kernel = self.lock_kernel()?;
        kernel
            .memory
            .upsert_supervised_sprint(&self.project.id, plan)
            .map_err(|error| error.to_string())
    }

    fn mutate_sprint<T>(
        &self,
        mutate: impl FnOnce(&mut SupervisedSprintPlan) -> Result<Option<T>, String>,
    ) -> Result<Option<T>, String> {
        let (result, plan) = {
            let mut sprint = self.lock_sprint()?;
            let Some(plan) = sprint.as_mut() else {
                return Ok(None);
            };
            let result = mutate(plan)?;
            (result, plan.clone())
        };
        self.persist_sprint_plan(&plan)?;
        Ok(result)
    }

    fn ensure_sprint_allows_generation(&self, target_title: Option<&str>) -> Result<(), String> {
        let Some(target_title) = target_title else {
            return Ok(());
        };
        let sprint = self.lock_sprint()?;
        let Some(plan) = sprint
            .as_ref()
            .filter(|plan| sprint_matches_target(plan, target_title))
        else {
            return Ok(());
        };
        if let Some(message) = sprint_block_message(plan, target_title) {
            return Err(message);
        }
        Ok(())
    }

    fn ensure_provider_budget_allowed(
        &self,
        target_title: &str,
        report: &WriterProviderBudgetReport,
    ) -> Result<(), String> {
        let sprint = self.lock_sprint()?;
        let Some(plan) = sprint
            .as_ref()
            .filter(|plan| sprint_matches_target(plan, target_title))
        else {
            return Ok(());
        };
        if let Some(message) = sprint_block_message(plan, target_title) {
            return Err(message);
        }
        if let Some(ceiling) = plan.budget_ceiling_micros {
            let projected = plan
                .spent_budget_micros
                .saturating_add(report.estimated_cost_micros);
            if projected > ceiling {
                return Err(format!(
                    "supervised sprint {} would exceed budget ceiling before {} (spent {} + estimated {} > ceiling {})",
                    plan.sprint_id,
                    target_title,
                    plan.spent_budget_micros,
                    report.estimated_cost_micros,
                    ceiling
                ));
            }
        }
        Ok(())
    }

    fn record_sprint_provider_budget(
        &self,
        target_title: &str,
        report: &WriterProviderBudgetReport,
    ) {
        let Ok(Some(())) = self.mutate_sprint(|plan| {
            if !sprint_matches_target(plan, target_title) {
                return Ok(Some(()));
            }
            record_budget_usage(plan, report.estimated_cost_micros);
            if budget_ceiling_reached(plan) {
                plan.status = "paused".to_string();
                update_current_chapter_state(
                    plan,
                    None,
                    None,
                    None,
                    Some("budget ceiling reached after provider estimate"),
                );
            }
            Ok(Some(()))
        }) else {
            return;
        };
    }

    fn record_sprint_context_built(
        &self,
        context: &crate::chapter_generation::BuiltChapterContext,
    ) {
        let readiness = if context.warnings.is_empty() {
            "ready"
        } else {
            "warning"
        };
        let _ = self.mutate_sprint(|plan| {
            if !sprint_matches_target(plan, &context.target.title) {
                return Ok(Some(()));
            }
            if plan.status == "planned" {
                plan.status = "running".to_string();
            }
            update_current_chapter_state(
                plan,
                Some("drafting"),
                Some(&context.receipt.task_id),
                Some(readiness),
                None,
            );
            Ok(Some(()))
        });
    }

    fn record_sprint_generation_completed(
        &self,
        saved: &crate::chapter_generation::SaveGeneratedChapterOutput,
        quality_report: Option<&crate::chapter_generation::ChapterQualityReport>,
    ) {
        let _ = self.mutate_sprint(|plan| {
            if !sprint_matches_target(plan, &saved.chapter_title) {
                return Ok(Some(()));
            }
            update_current_chapter_state(plan, Some("settled"), None, Some("ready"), None);
            // Sprint quality gate: blocks advance when quality falls below threshold.
            if let Err(e) = check_sprint_quality_gate(plan, quality_report) {
                tracing::warn!("Sprint quality gate blocked {}: {}", saved.chapter_title, e);
                return Ok(Some(()));
            }
            if !plan.require_approval_per_chapter && !budget_ceiling_reached(plan) {
                let _ = advance_sprint(plan);
            }
            Ok(Some(()))
        });
    }

    fn record_sprint_generation_failed(&self, target_title: Option<&str>, error_message: &str) {
        let Some(target_title) = target_title else {
            return;
        };
        let _ = self.mutate_sprint(|plan| {
            if !sprint_matches_target(plan, target_title) {
                return Ok(Some(()));
            }
            update_current_chapter_state(
                plan,
                Some("blocked"),
                None,
                Some("blocked"),
                Some(error_message),
            );
            Ok(Some(()))
        });
    }

    fn finish_headless_chapter_generation(
        &self,
        terminal: crate::chapter_generation::PipelineTerminal,
        request_id: &str,
        events: Vec<crate::chapter_generation::ChapterGenerationEvent>,
    ) -> Result<HeadlessChapterGenerationOutput, String> {
        match terminal {
            crate::chapter_generation::PipelineTerminal::Completed {
                saved,
                generated_content,
                settlement_delta,
                quality_report,
            } => {
                self.record_sprint_generation_completed(&saved, quality_report.as_ref());
                self.observe_generated_chapter_result(
                    &saved,
                    &generated_content,
                    &settlement_delta,
                );
                self.embed_chapter_best_effort(&saved.chapter_title, &generated_content);
                Ok(HeadlessChapterGenerationOutput {
                    terminal: "completed".to_string(),
                    events,
                    saved: Some(saved),
                    generated_content: Some(generated_content),
                    settlement_delta: Some(*settlement_delta),
                    conflict: None,
                    error: None,
                })
            }
            crate::chapter_generation::PipelineTerminal::Conflict(conflict) => {
                let message = format!("save conflict: {}", conflict.reason);
                self.record_sprint_generation_failed(
                    conflict.open_chapter_title.as_deref(),
                    &message,
                );
                let bundle = crate::writer_agent::task_receipt::WriterFailureEvidenceBundle::new(
                    crate::writer_agent::task_receipt::WriterFailureCategory::SaveFailed,
                    "SAVE_CONFLICT",
                    format!("Save blocked by {}.", conflict.reason),
                    true,
                    Some(request_id.to_string()),
                    vec![
                        format!("base_revision:{}", conflict.base_revision),
                        format!("current_revision:{}", conflict.current_revision),
                        format!("save_conflict:{}", conflict.reason),
                    ],
                    serde_json::json!({ "conflict": conflict }),
                    vec![
                        "Resolve editor/storage revision mismatch or save as a draft copy."
                            .to_string(),
                    ],
                    now_ms(),
                );
                if let Ok(mut kernel) = self.lock_kernel() {
                    kernel.record_failure_evidence_bundle(&bundle);
                }
                Ok(HeadlessChapterGenerationOutput {
                    terminal: "conflict".to_string(),
                    events,
                    saved: None,
                    generated_content: None,
                    settlement_delta: None,
                    conflict: Some(conflict),
                    error: None,
                })
            }
            crate::chapter_generation::PipelineTerminal::Failed(error) => {
                self.record_sprint_generation_failed(None, &error.message);
                let bundle = crate::chapter_generation::failure_bundle_from_chapter_error(
                    request_id,
                    &error,
                    now_ms(),
                );
                if let Ok(mut kernel) = self.lock_kernel() {
                    kernel.record_failure_evidence_bundle(&bundle);
                }
                Ok(HeadlessChapterGenerationOutput {
                    terminal: "failed".to_string(),
                    events,
                    saved: None,
                    generated_content: None,
                    settlement_delta: None,
                    conflict: None,
                    error: Some(error),
                })
            }
        }
    }

    fn observe_generated_chapter_result(
        &self,
        saved: &crate::chapter_generation::SaveGeneratedChapterOutput,
        generated_content: &str,
        settlement_delta: &crate::chapter_generation::ChapterSettlementDelta,
    ) {
        let project_id = self.project.id.clone();
        let precomputed_result = self.lock_kernel().ok().map(|kernel| {
            let created_at = kernel
                .memory
                .latest_chapter_result(&project_id, &settlement_delta.chapter_title)
                .ok()
                .flatten()
                .filter(|result| result.chapter_revision == settlement_delta.chapter_revision)
                .map(|result| result.created_at)
                .unwrap_or_else(now_ms);
            crate::writer_agent::memory::ChapterResultSummary {
                id: 0,
                project_id: project_id.clone(),
                chapter_title: settlement_delta.chapter_title.clone(),
                chapter_revision: settlement_delta.chapter_revision.clone(),
                summary: settlement_delta.chapter_result.summary.clone(),
                state_changes: settlement_delta.chapter_result.state_changes.clone(),
                character_progress: settlement_delta.chapter_result.character_progress.clone(),
                new_conflicts: settlement_delta.chapter_result.new_conflicts.clone(),
                new_clues: settlement_delta.chapter_result.new_clues.clone(),
                promise_updates: settlement_delta.chapter_result.promise_updates.clone(),
                canon_updates: settlement_delta.chapter_result.canon_updates.clone(),
                source_ref: format!(
                    "chapter_settlement:{}:{}",
                    settlement_delta.chapter_title, settlement_delta.chapter_revision
                ),
                created_at,
            }
        });
        let observation = chapter_save_observation(
            &self.project.id,
            &saved.chapter_title,
            &saved.new_revision,
            generated_content,
        );
        if let Ok(mut kernel) = self.lock_kernel() {
            refresh_kernel_canon_from_headless(&self.config.data_dir, &self.project, &mut kernel);
            let result = if let Some(result) = precomputed_result {
                kernel.observe_save_result(observation, result)
            } else {
                kernel.observe(observation)
            };
            if let Err(error) = result {
                tracing::warn!(
                    "Headless generated-chapter result feedback failed for '{}': {}",
                    saved.chapter_title,
                    error
                );
            }
        }
    }

    fn embed_chapter_best_effort(&self, chapter_title: &str, content: &str) {
        let Some(api_key) = crate::resolve_api_key() else {
            return;
        };
        let settings = crate::llm_runtime::settings(api_key);
        let project_id = self.project.id.clone();
        let data_dir = self.config.data_dir.clone();
        let chapter_title = chapter_title.to_string();
        let content = content.to_string();
        tokio::spawn(async move {
            if let Err(error) =
                embed_chapter_headless(&data_dir, &project_id, &settings, &chapter_title, &content)
                    .await
            {
                tracing::warn!(
                    "Headless Project Brain update failed for '{}': {}",
                    chapter_title,
                    error
                );
            }
        });
    }

    fn user_profile_entries(&self) -> Vec<String> {
        let Ok(kernel) = self.lock_kernel() else {
            return Vec::new();
        };
        kernel
            .memory
            .list_style_preferences(32)
            .unwrap_or_default()
            .into_iter()
            .map(|profile| {
                format!(
                    "- {}: {} (accepted {}, rejected {})",
                    profile.key, profile.value, profile.accepted_count, profile.rejected_count
                )
            })
            .collect()
    }

    fn reseed_story_model(&self) {
        if let Ok(kernel) = self.lock_kernel() {
            seed_story_model_if_empty(&self.config.data_dir, &self.project, &kernel.memory);
        }
    }
}

fn load_or_create_project_manifest(config: &HeadlessConfig) -> Result<ProjectManifest, String> {
    let path = config.data_dir.join(ACTIVE_PROJECT_FILENAME);
    if path.exists() {
        let raw = std::fs::read_to_string(&path).map_err(|error| error.to_string())?;
        let manifest: ProjectManifest = serde_json::from_str(&raw)
            .map_err(|error| format!("Failed to parse project manifest: {}", error))?;
        validate_project_id(&manifest.id)?;
        return Ok(manifest);
    }

    let id = match config
        .project_id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty())
    {
        Some(id) => {
            validate_project_id(id)?;
            id.to_string()
        }
        None => generated_project_id(&config.data_dir),
    };
    let manifest = ProjectManifest {
        id,
        name: config
            .project_name
            .as_deref()
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .unwrap_or(DEFAULT_PROJECT_NAME)
            .to_string(),
    };
    write_json_pretty(&path, &manifest)?;
    Ok(manifest)
}

fn validate_project_id(id: &str) -> Result<(), String> {
    if !id.trim().is_empty()
        && id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
    {
        Ok(())
    } else {
        Err(format!("Invalid project id '{}'", id))
    }
}

fn generated_project_id(data_dir: &Path) -> String {
    let revision = storage::content_revision(&data_dir.to_string_lossy());
    let hash = revision.split('-').next().unwrap_or("0000000000000000");
    format!("local-{}", hash)
}

fn project_data_dir(data_dir: &Path, project_id: &str) -> Result<PathBuf, String> {
    validate_project_id(project_id)?;
    Ok(data_dir.join("projects").join(project_id))
}

fn chapters_dir(data_dir: &Path, project_id: &str) -> Result<PathBuf, String> {
    Ok(project_data_dir(data_dir, project_id)?.join("chapters"))
}

fn writer_memory_path(data_dir: &Path, project_id: &str) -> Result<PathBuf, String> {
    Ok(project_data_dir(data_dir, project_id)?.join(storage::WRITER_MEMORY_DB_FILENAME))
}

fn lorebook_path(data_dir: &Path, project_id: &str) -> Result<PathBuf, String> {
    Ok(project_data_dir(data_dir, project_id)?.join("lorebook.json"))
}

fn outline_path(data_dir: &Path, project_id: &str) -> Result<PathBuf, String> {
    Ok(project_data_dir(data_dir, project_id)?.join("outline.json"))
}

fn brain_path(data_dir: &Path, project_id: &str) -> Result<PathBuf, String> {
    Ok(project_data_dir(data_dir, project_id)?.join("project_brain.json"))
}

fn knowledge_index_path(data_dir: &Path, project_id: &str) -> Result<PathBuf, String> {
    Ok(project_data_dir(data_dir, project_id)?.join(KNOWLEDGE_INDEX_FILENAME))
}

fn chapter_path(data_dir: &Path, project_id: &str, title: &str) -> Result<PathBuf, String> {
    Ok(chapters_dir(data_dir, project_id)?.join(storage::chapter_filename(title)))
}

fn read_project_dir(data_dir: &Path, project_id: &str) -> Result<Vec<ChapterInfo>, String> {
    let dir = chapters_dir(data_dir, project_id)?;
    std::fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    let mut chapters = Vec::new();
    for entry in std::fs::read_dir(&dir).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let path = entry.path();
        if path.extension().is_some_and(|extension| extension == "md") {
            let stem = path
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            chapters.push(ChapterInfo {
                filename: path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string(),
                title: stem.replace('-', " "),
            });
        }
    }
    chapters.sort_by(|left, right| left.title.cmp(&right.title));
    Ok(chapters)
}

fn refresh_kernel_canon_from_headless(
    data_dir: &Path,
    project: &ProjectManifest,
    kernel: &mut WriterAgentKernel,
) {
    if let Ok(entries) =
        read_json_array::<LoreEntry>(&lorebook_path(data_dir, &project.id).unwrap_or_default())
    {
        for entry in entries {
            let keyword = entry.keyword.trim();
            if keyword.is_empty() {
                continue;
            }
            let summary = entry.content.chars().take(240).collect::<String>();
            let _ = kernel.memory.upsert_character(
                keyword,
                &Vec::<String>::new(),
                "supporting",
                &summary,
            );
        }
    }
}

fn manual_writer_observation(request: &AskAgentRequest, project_id: &str) -> WriterObservation {
    let cursor_position = request
        .cursor_position
        .unwrap_or_else(|| request.context.chars().count());
    let chapter_title = request
        .chapter_title
        .clone()
        .filter(|title| !title.trim().is_empty())
        .or_else(|| Some("manual".to_string()));
    let chapter_revision = request
        .chapter_revision
        .clone()
        .filter(|revision| !revision.trim().is_empty())
        .or_else(|| Some(storage::content_revision(&request.context)));
    let paragraph = if request.paragraph.trim().is_empty() {
        if request.selected_text.trim().is_empty() {
            request.message.clone()
        } else {
            request.selected_text.clone()
        }
    } else {
        request.paragraph.clone()
    };
    let (prefix, suffix) =
        split_context_for_cursor(&request.context, cursor_position, 3_000, 1_000);
    WriterObservation {
        id: format!("manual-{}", now_ms()),
        created_at: now_ms(),
        source: crate::writer_agent::observation::ObservationSource::ManualRequest,
        reason: crate::writer_agent::observation::ObservationReason::Explicit,
        project_id: project_id.to_string(),
        chapter_title,
        chapter_revision,
        cursor: Some(crate::writer_agent::observation::TextRange {
            from: cursor_position,
            to: cursor_position,
        }),
        selection: selected_text_range(&request.context, &request.selected_text),
        prefix,
        suffix,
        paragraph,
        full_text_digest: Some(storage::content_revision(&request.context)),
        editor_dirty: request.dirty.unwrap_or(false),
    }
}

fn split_context_for_cursor(
    context: &str,
    cursor_position: usize,
    prefix_chars: usize,
    suffix_chars: usize,
) -> (String, String) {
    let cursor_position = cursor_position.min(context.chars().count());
    let prefix = context.chars().take(cursor_position).collect::<String>();
    let suffix = context
        .chars()
        .skip(cursor_position)
        .take(suffix_chars)
        .collect::<String>();
    (text_tail(&prefix, prefix_chars), suffix)
}

fn selected_text_range(
    context: &str,
    selected_text: &str,
) -> Option<crate::writer_agent::observation::TextSelection> {
    let selected = selected_text.trim();
    if selected.is_empty() {
        return None;
    }
    let (from, to) = find_char_range(context, selected).unwrap_or((0, selected.chars().count()));
    Some(crate::writer_agent::observation::TextSelection {
        from,
        to,
        text: selected.to_string(),
    })
}

fn find_char_range(text: &str, needle: &str) -> Option<(usize, usize)> {
    let start_byte = text.find(needle)?;
    let start = text[..start_byte.min(text.len())].chars().count();
    let end = start + needle.chars().count();
    Some((start, end))
}

fn manual_agent_history_messages(
    turns: &[ManualAgentTurnSummary],
    max_turns: usize,
    max_chars: usize,
) -> Vec<LlmMessage> {
    let mut selected = Vec::new();
    let mut consumed = 0usize;
    for turn in turns.iter().rev() {
        if selected.len() >= max_turns {
            break;
        }
        let user = text_tail(&turn.user, 1_200);
        let assistant = text_tail(&turn.assistant, 2_400);
        let cost = user.chars().count() + assistant.chars().count();
        if !selected.is_empty() && consumed + cost > max_chars {
            break;
        }
        consumed += cost;
        selected.push((turn, user, assistant));
    }
    selected.reverse();

    let mut messages = Vec::with_capacity(selected.len() * 2);
    for (turn, user, assistant) in selected {
        messages.push(LlmMessage {
            role: "user".to_string(),
            content: Some(format!(
                "[Earlier manual request, project={}, at={}]\n{}",
                turn.project_id, turn.created_at, user
            )),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        });
        let source_note = if turn.source_refs.is_empty() {
            String::new()
        } else {
            format!(
                "\n\n[Context sources used: {}]",
                turn.source_refs
                    .iter()
                    .take(12)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };
        messages.push(LlmMessage {
            role: "assistant".to_string(),
            content: Some(format!("{}{}", assistant, source_note)),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        });
    }
    messages
}

fn writer_agent_inline_operation_messages(
    message: &str,
    observation: &WriterObservation,
    pack: &crate::writer_agent::context::WritingContextPack,
) -> Vec<serde_json::Value> {
    let context = crate::writer_agent::kernel::render_context_pack_for_prompt(pack);
    let selected = observation.selected_text();
    vec![
        serde_json::json!({
            "role": "system",
            "content": "你是 Forge 的 Cursor 式中文小说写作 Agent。你只为当前光标生成可执行的正文改写或插入文本，不聊天，不解释，不输出 Markdown，不输出 XML action 标签。必须尊重 ContextPack、设定、伏笔和光标后文。输出必须是可直接进入小说正文的中文文本。"
        }),
        serde_json::json!({
            "role": "user",
            "content": format!(
                "作者指令: {}\n章节: {}\n光标文本位置: {}\n选中文本:\n{}\n\n光标前文:\n{}\n\n光标后文:\n{}\n\nContextPack:\n{}\n\n请只输出要应用到正文中的文本:",
                message,
                observation.chapter_title.as_deref().unwrap_or("current chapter"),
                observation.cursor.as_ref().map(|cursor| cursor.to).unwrap_or(0),
                selected,
                observation.prefix,
                observation.suffix,
                context
            )
        }),
    ]
}

fn trim_parallel_draft(text: &str) -> String {
    text.trim_matches(|ch: char| ch == '`' || ch.is_whitespace())
        .chars()
        .take(1_200)
        .collect::<String>()
}

fn parse_parallel_drafts(raw: &str) -> Vec<ParallelDraft> {
    let labels = ["A 顺势推进", "B 冲突加压", "C 情绪转折"];
    let ids = ["a", "b", "c"];
    let mut drafts = Vec::new();
    let mut current_idx: Option<usize> = None;
    let mut current_text = String::new();

    let flush = |drafts: &mut Vec<ParallelDraft>,
                 current_idx: &mut Option<usize>,
                 current_text: &mut String| {
        let Some(idx) = current_idx.take() else {
            current_text.clear();
            return;
        };
        let text = trim_parallel_draft(current_text);
        current_text.clear();
        if text.is_empty() {
            return;
        }
        drafts.push(ParallelDraft {
            id: ids[idx].to_string(),
            label: labels[idx].to_string(),
            text,
        });
    };

    for line in raw.lines() {
        let trimmed = line.trim_start();
        let marker = trimmed
            .split_once(':')
            .or_else(|| trimmed.split_once('：'))
            .and_then(|(head, body)| {
                let idx = match head.trim().chars().next().map(|ch| ch.to_ascii_uppercase()) {
                    Some('A') => 0,
                    Some('B') => 1,
                    Some('C') => 2,
                    _ => return None,
                };
                Some((idx, body.trim_start()))
            });

        if let Some((idx, body)) = marker {
            flush(&mut drafts, &mut current_idx, &mut current_text);
            current_idx = Some(idx);
            current_text.push_str(body);
        } else if current_idx.is_some() {
            if !current_text.is_empty() {
                current_text.push('\n');
            }
            current_text.push_str(line);
        }
    }
    flush(&mut drafts, &mut current_idx, &mut current_text);
    drafts.truncate(3);
    drafts
}

fn install_headless_provider_budget_guard(
    prepared_run: &mut crate::writer_agent::kernel::WriterAgentPreparedRun<
        OpenAiCompatProvider,
        HeadlessToolBridge,
    >,
    project_id: String,
    data_dir: PathBuf,
    request_id: String,
    observation: WriterObservation,
    approval: Option<WriterProviderBudgetApproval>,
    preflight_estimated_input_tokens: u64,
) {
    prepared_run.set_provider_call_guard(Arc::new(move |context| {
        let mut report = crate::writer_agent::kernel::WriterAgentPreparedRun::<
            OpenAiCompatProvider,
            HeadlessToolBridge,
        >::provider_budget_from_call_context(
            WriterProviderBudgetTask::ManualRequest,
            &context,
        );
        if context.round == 1
            && report.estimated_input_tokens <= preflight_estimated_input_tokens
            && report.approval_required
        {
            report = apply_provider_budget_approval(report, approval.as_ref());
        }

        let task_id = format!("manual-request-{}-round-{}", request_id, context.round);
        let mut source_refs = vec![
            format!("manual_request:{}", request_id),
            format!("model:{}", report.model),
            format!("estimated_tokens:{}", report.estimated_total_tokens),
            format!("estimated_cost_micros:{}", report.estimated_cost_micros),
        ];
        if let Some(chapter) = observation.chapter_title.as_deref() {
            source_refs.push(format!("chapter:{}", chapter));
        }
        if let Some(revision) = observation.chapter_revision.as_deref() {
            source_refs.push(format!("revision:{}", revision));
        }

        if let Ok(memory_path) = writer_memory_path(&data_dir, &project_id) {
            if let Ok(memory) = WriterMemory::open(&memory_path) {
                let mut kernel = WriterAgentKernel::new(&project_id, memory);
                kernel.record_provider_budget_report(
                    task_id.clone(),
                    &report,
                    source_refs.clone(),
                    now_ms(),
                );
                if report.approval_required {
                    let bundle =
                        crate::writer_agent::task_receipt::WriterFailureEvidenceBundle::new(
                            crate::writer_agent::task_receipt::WriterFailureCategory::ProviderFailed,
                            "HEADLESS_PROVIDER_BUDGET_APPROVAL_REQUIRED",
                            "Headless Writer Agent provider budget requires explicit approval before entering the agent loop.",
                            true,
                            Some(task_id.clone()),
                            source_refs.clone(),
                            serde_json::json!({ "providerBudget": report }),
                            vec![
                                "Approve the provider budget and retry, or narrow the request context."
                                    .to_string(),
                            ],
                            now_ms(),
                        );
                    kernel.record_failure_evidence_bundle(&bundle);
                } else {
                    kernel.record_model_started_run_event(
                        ModelStartedEventContext {
                            task_id: task_id.clone(),
                            task: report.task,
                            model: report.model.clone(),
                            provider: context.provider.clone(),
                            stream: context.stream,
                        },
                        source_refs.clone(),
                        Some(&report),
                        now_ms(),
                    );
                }
            }
        }

        if report.approval_required {
            return Err("HEADLESS_PROVIDER_BUDGET_APPROVAL_REQUIRED".to_string());
        }
        Ok(())
    }));
}

fn install_headless_recovery_provider_budget_guard(
    prepared_run: &mut crate::writer_agent::kernel::WriterAgentPreparedRun<
        OpenAiCompatProvider,
        HeadlessToolBridge,
    >,
    project_id: String,
    data_dir: PathBuf,
    request_id: String,
    observation: WriterObservation,
    action: MetacognitiveRecoveryAction,
    approval: Option<WriterProviderBudgetApproval>,
    preflight_estimated_input_tokens: u64,
) {
    prepared_run.set_provider_call_guard(Arc::new(move |context| {
        let mut report = crate::writer_agent::kernel::WriterAgentPreparedRun::<
            OpenAiCompatProvider,
            HeadlessToolBridge,
        >::provider_budget_from_call_context(
            WriterProviderBudgetTask::MetacognitiveRecovery,
            &context,
        );
        if context.round == 1
            && report.estimated_input_tokens <= preflight_estimated_input_tokens
            && report.approval_required
        {
            report = apply_provider_budget_approval(report, approval.as_ref());
        }
        let task_id = format!(
            "metacognitive-recovery-{}-round-{}",
            request_id, context.round
        );
        let source_refs = recovery_budget_source_refs(&request_id, &observation, &report, &action);
        if let Ok(memory_path) = writer_memory_path(&data_dir, &project_id) {
            if let Ok(memory) = WriterMemory::open(&memory_path) {
                let mut kernel = WriterAgentKernel::new(&project_id, memory);
                kernel.record_provider_budget_report(
                    task_id.clone(),
                    &report,
                    source_refs.clone(),
                    now_ms(),
                );
                if report.approval_required {
                    let bundle =
                        crate::writer_agent::task_receipt::WriterFailureEvidenceBundle::new(
                            crate::writer_agent::task_receipt::WriterFailureCategory::ProviderFailed,
                            "METACOGNITIVE_RECOVERY_PROVIDER_BUDGET_APPROVAL_REQUIRED",
                            "Metacognitive recovery provider budget requires explicit approval before entering the read-only agent loop.",
                            true,
                            Some(task_id.clone()),
                            source_refs.clone(),
                            serde_json::json!({
                                "providerBudget": report,
                                "recoveryAction": action.label(),
                            }),
                            vec![
                                "Approve the recovery provider budget and retry, or narrow the current context."
                                    .to_string(),
                            ],
                            now_ms(),
                        );
                    kernel.record_failure_evidence_bundle(&bundle);
                } else {
                    kernel.record_model_started_run_event(
                        ModelStartedEventContext {
                            task_id: task_id.clone(),
                            task: report.task,
                            model: report.model.clone(),
                            provider: context.provider.clone(),
                            stream: context.stream,
                        },
                        source_refs.clone(),
                        Some(&report),
                        now_ms(),
                    );
                }
            }
        }
        if report.approval_required {
            return Err("METACOGNITIVE_RECOVERY_PROVIDER_BUDGET_APPROVAL_REQUIRED".to_string());
        }
        Ok(())
    }));
}

fn recovery_budget_source_refs(
    request_id: &str,
    observation: &WriterObservation,
    report: &WriterProviderBudgetReport,
    action: &MetacognitiveRecoveryAction,
) -> Vec<String> {
    let mut refs = vec![
        format!("metacognitive_recovery:{}", request_id),
        format!("recovery_action:{}", action.label()),
        format!("model:{}", report.model),
        format!("estimated_tokens:{}", report.estimated_total_tokens),
        format!("estimated_cost_micros:{}", report.estimated_cost_micros),
    ];
    if let Some(chapter) = observation.chapter_title.as_deref() {
        refs.push(format!("chapter:{}", chapter));
    }
    if let Some(revision) = observation.chapter_revision.as_deref() {
        refs.push(format!("revision:{}", revision));
    }
    refs
}

fn chapter_model_source_refs(
    context: &crate::chapter_generation::BuiltChapterContext,
    report: &WriterProviderBudgetReport,
) -> Vec<String> {
    let mut source_refs = vec![
        format!("receipt:{}", context.receipt.task_id),
        format!("chapter:{}", context.target.title),
        format!("model:{}", report.model),
        format!("estimated_tokens:{}", report.estimated_total_tokens),
        format!("estimated_cost_micros:{}", report.estimated_cost_micros),
    ];
    source_refs.extend(
        context
            .sources
            .iter()
            .filter(|source| source.included_chars > 0)
            .map(|source| format!("{}:{}", source.source_type, source.id)),
    );
    source_refs
}

fn record_chapter_provider_budget_report_headless(
    project_id: &str,
    memory_path: &Path,
    context: &crate::chapter_generation::BuiltChapterContext,
    report: &WriterProviderBudgetReport,
) {
    let Ok(memory) = WriterMemory::open(memory_path) else {
        return;
    };
    let mut kernel = WriterAgentKernel::new(project_id, memory);
    kernel.record_provider_budget_report(
        context.request_id.clone(),
        report,
        chapter_model_source_refs(context, report),
        now_ms(),
    );
}

fn record_chapter_model_started_headless(
    project_id: &str,
    memory_path: &Path,
    context: &crate::chapter_generation::BuiltChapterContext,
    report: &WriterProviderBudgetReport,
) {
    let Ok(memory) = WriterMemory::open(memory_path) else {
        return;
    };
    let mut kernel = WriterAgentKernel::new(project_id, memory);
    kernel.record_model_started_run_event(
        ModelStartedEventContext {
            task_id: context.request_id.clone(),
            task: report.task,
            model: report.model.clone(),
            provider: "openai-compatible".to_string(),
            stream: false,
        },
        chapter_model_source_refs(context, report),
        Some(report),
        now_ms(),
    );
}

fn sprint_current_chapter_title(plan: &SupervisedSprintPlan) -> Option<&str> {
    plan.chapters
        .get(plan.current_index)
        .map(|chapter| chapter.chapter_title.as_str())
}

fn sprint_matches_target(plan: &SupervisedSprintPlan, target_title: &str) -> bool {
    sprint_current_chapter_title(plan).is_some_and(|title| title == target_title)
}

fn sprint_block_message(plan: &SupervisedSprintPlan, target_title: &str) -> Option<String> {
    if plan.status == "paused" {
        return Some(format!(
            "supervised sprint {} is paused before {}",
            plan.sprint_id, target_title
        ));
    }
    if plan.status == "cancelled" || plan.status == "completed" {
        return Some(format!(
            "supervised sprint {} is {}",
            plan.sprint_id, plan.status
        ));
    }
    if budget_ceiling_reached(plan) {
        return Some(format!(
            "supervised sprint {} reached budget ceiling before {}",
            plan.sprint_id, target_title
        ));
    }
    None
}

fn headless_sprint_blocked_error(
    message: String,
) -> crate::chapter_generation::ChapterGenerationError {
    crate::chapter_generation::ChapterGenerationError::new(
        "SUPERVISED_SPRINT_BLOCKED",
        message,
        true,
    )
}

fn safe_chapter_file_path(dir: &Path, filename: &str) -> Result<PathBuf, String> {
    let path = Path::new(filename);
    if path.components().count() != 1 || path.file_name().is_none() {
        return Err(format!("Invalid chapter filename: {}", filename));
    }
    if path
        .extension()
        .map(|extension| extension != "md")
        .unwrap_or(true)
    {
        return Err(format!("Chapter filename must end with .md: {}", filename));
    }
    Ok(dir.join(path))
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

fn read_json_array<T>(path: &Path) -> Result<Vec<T>, String>
where
    T: for<'de> Deserialize<'de>,
{
    if !path.exists() {
        return Ok(Vec::new());
    }
    let raw = std::fs::read_to_string(path).map_err(|error| error.to_string())?;
    serde_json::from_str(&raw)
        .map_err(|error| format!("Failed to parse JSON at '{}': {}", path.display(), error))
}

async fn answer_project_brain_query(
    data_dir: &Path,
    project_id: &str,
    settings: &crate::llm_runtime::LlmSettings,
    query: &str,
    provider_budget_approval: Option<&WriterProviderBudgetApproval>,
) -> Result<String, String> {
    let search_text = query.trim();
    if search_text.is_empty() {
        return Err("Project Brain query is required".to_string());
    }
    let query_embedding = crate::llm_runtime::embed(settings, search_text, 30)
        .await
        .map_err(|error| format!("Embed error: {}", error))?;
    let path = brain_path(data_dir, project_id)?;
    let db = agent_harness_core::vector_db::VectorDB::load(&path).map_err(|error| {
        format!(
            "Project Brain index at '{}' is unreadable; restore a backup or rebuild the index: {}",
            path.display(),
            error
        )
    })?;
    let results = db.search_hybrid(search_text, &query_embedding, 5);
    let context = if results.is_empty() {
        "No relevant chunks found in the book.".to_string()
    } else {
        results
            .iter()
            .enumerate()
            .map(|(index, (score, chunk))| {
                format!(
                    "[Chunk {} · {} · score {:.3}]\n{}",
                    index + 1,
                    chunk.chapter,
                    score,
                    chunk.text
                )
            })
            .collect::<Vec<_>>()
            .join("\n\n")
    };
    let messages = vec![
        serde_json::json!({"role": "system", "content": format!(
            "You are an expert on this novel. Answer the user's question using ONLY the provided book excerpts. If the excerpts don't contain relevant information, say so honestly.\n\nBook excerpts:\n{}",
            context
        )}),
        serde_json::json!({"role": "user", "content": query}),
    ];
    let budget_report = apply_provider_budget_approval(
        project_brain_query_provider_budget(settings, &messages),
        provider_budget_approval,
    );
    if budget_report.approval_required {
        return Err("PROJECT_BRAIN_PROVIDER_BUDGET_APPROVAL_REQUIRED".to_string());
    }
    crate::llm_runtime::chat_text_profile(
        settings,
        messages,
        crate::llm_runtime::LlmRequestProfile::ProjectBrainStream,
        60,
    )
    .await
}

fn project_brain_query_provider_budget(
    settings: &crate::llm_runtime::LlmSettings,
    messages: &[serde_json::Value],
) -> WriterProviderBudgetReport {
    let converted = messages
        .iter()
        .map(|message| LlmMessage {
            role: message
                .get("role")
                .and_then(|value| value.as_str())
                .unwrap_or("user")
                .to_string(),
            content: message
                .get("content")
                .and_then(|value| value.as_str())
                .map(str::to_string),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        })
        .collect::<Vec<_>>();
    let estimated_input_tokens =
        agent_harness_core::context_window_guard::estimate_request_tokens(&converted, None);
    evaluate_provider_budget(WriterProviderBudgetRequest::new(
        WriterProviderBudgetTask::ProjectBrainQuery,
        settings.model.clone(),
        estimated_input_tokens,
        u64::from(
            crate::llm_runtime::request_options(
                settings,
                crate::llm_runtime::LlmRequestProfile::ProjectBrainStream,
            )
            .max_tokens,
        ),
    ))
}

fn load_project_brain_knowledge_index(
    data_dir: &Path,
    project_id: &str,
    outline: &[OutlineNode],
    lorebook: &[LoreEntry],
) -> Result<crate::brain_service::ProjectBrainKnowledgeIndex, String> {
    let path = knowledge_index_path(data_dir, project_id)?;
    if path.exists() {
        let data = std::fs::read_to_string(&path).map_err(|error| {
            format!(
                "Failed to read knowledge index '{}': {}",
                path.display(),
                error
            )
        })?;
        return serde_json::from_str(&data).map_err(|error| {
            format!(
                "Failed to parse knowledge index '{}': {}",
                path.display(),
                error
            )
        });
    }
    rebuild_project_brain_knowledge_index(data_dir, project_id, outline, lorebook)
}

fn rebuild_project_brain_knowledge_index(
    data_dir: &Path,
    project_id: &str,
    outline: &[OutlineNode],
    lorebook: &[LoreEntry],
) -> Result<crate::brain_service::ProjectBrainKnowledgeIndex, String> {
    let brain_path = brain_path(data_dir, project_id)?;
    let brain = agent_harness_core::vector_db::VectorDB::load(&brain_path).map_err(|error| {
        format!(
            "Project Brain index at '{}' is unreadable; restore a backup or rebuild the index: {}",
            brain_path.display(),
            error
        )
    })?;
    let index = build_project_brain_knowledge_index(project_id, &brain, outline, lorebook);
    let json = serde_json::to_string_pretty(&index).map_err(|error| error.to_string())?;
    storage::atomic_write(&knowledge_index_path(data_dir, project_id)?, &json)?;
    Ok(index)
}

fn build_project_brain_knowledge_index(
    project_id: &str,
    brain: &agent_harness_core::vector_db::VectorDB,
    outline: &[OutlineNode],
    lorebook: &[LoreEntry],
) -> crate::brain_service::ProjectBrainKnowledgeIndex {
    let mut nodes = Vec::new();
    for entry in lorebook {
        nodes.push(crate::brain_service::ProjectBrainKnowledgeNode {
            id: format!("lore:{}", stable_node_id(&entry.id, &entry.keyword)),
            kind: "lore".to_string(),
            label: entry.keyword.clone(),
            source_ref: format!("lorebook:{}", entry.id),
            source_revision: None,
            source_kind: Some("lorebook".to_string()),
            chunk_index: None,
            archived: false,
            keywords: unique_keywords(vec![entry.keyword.clone()], &entry.content),
            summary: snippet_text(&entry.content, 220),
        });
    }
    for node in outline {
        nodes.push(crate::brain_service::ProjectBrainKnowledgeNode {
            id: format!(
                "outline:{}",
                stable_node_id(&node.chapter_title, &node.summary)
            ),
            kind: "outline".to_string(),
            label: node.chapter_title.clone(),
            source_ref: format!("outline:{}", node.chapter_title),
            source_revision: None,
            source_kind: Some("outline".to_string()),
            chunk_index: None,
            archived: false,
            keywords: unique_keywords(vec![node.chapter_title.clone()], &node.summary),
            summary: snippet_text(&node.summary, 220),
        });
    }
    for chunk in &brain.chunks {
        let label = if chunk.chapter.trim().is_empty() {
            chunk.id.clone()
        } else {
            chunk.chapter.clone()
        };
        let source_ref = chunk
            .source_ref
            .clone()
            .unwrap_or_else(|| format!("project_brain:{}", chunk.id));
        nodes.push(crate::brain_service::ProjectBrainKnowledgeNode {
            id: format!("chunk:{}", stable_node_id(&chunk.id, &chunk.chapter)),
            kind: "chunk".to_string(),
            label,
            source_ref,
            source_revision: chunk.source_revision.clone(),
            source_kind: chunk.source_kind.clone(),
            chunk_index: chunk.chunk_index,
            archived: chunk.archived,
            keywords: unique_keywords(chunk.keywords.clone(), &chunk.text),
            summary: snippet_text(&chunk.text, 220),
        });
    }
    let edges = build_knowledge_edges(&nodes);
    let source_history = build_source_history(&nodes);
    crate::brain_service::ProjectBrainKnowledgeIndex {
        project_id: project_id.to_string(),
        source_count: lorebook.len() + outline.len() + brain.chunks.len(),
        nodes,
        edges,
        source_history,
    }
}

fn build_source_history(
    nodes: &[crate::brain_service::ProjectBrainKnowledgeNode],
) -> Vec<crate::brain_service::ProjectBrainSourceHistory> {
    #[derive(Default)]
    struct SourceAccumulator {
        source_kind: Option<String>,
        revisions: BTreeMap<String, crate::brain_service::ProjectBrainSourceRevision>,
        node_count: usize,
        chunk_count: usize,
        active_revisions: HashSet<String>,
        latest_summary: String,
    }

    let mut by_source = BTreeMap::<String, SourceAccumulator>::new();
    for node in nodes {
        let source_ref = node.source_ref.trim();
        if source_ref.is_empty() {
            continue;
        }
        let entry = by_source.entry(source_ref.to_string()).or_default();
        entry.node_count += 1;
        if node.kind == "chunk" {
            entry.chunk_count += 1;
        }
        if entry.source_kind.is_none() {
            entry.source_kind = node
                .source_kind
                .clone()
                .filter(|kind| !kind.trim().is_empty())
                .or_else(|| Some(node.kind.clone()));
        }
        if !node.summary.trim().is_empty() {
            entry.latest_summary = node.summary.clone();
        }
        if let Some(revision) = node
            .source_revision
            .as_deref()
            .map(str::trim)
            .filter(|revision| !revision.is_empty())
        {
            if !node.archived {
                entry.active_revisions.insert(revision.to_string());
            }
            let revision_entry = entry
                .revisions
                .entry(revision.to_string())
                .or_insert_with(|| crate::brain_service::ProjectBrainSourceRevision {
                    revision: revision.to_string(),
                    node_count: 0,
                    chunk_indexes: Vec::new(),
                    active: false,
                });
            revision_entry.node_count += 1;
            if let Some(chunk_index) = node.chunk_index {
                revision_entry.chunk_indexes.push(chunk_index);
            }
        }
    }

    by_source
        .into_iter()
        .map(|(source_ref, entry)| {
            let mut revisions = entry.revisions.into_values().collect::<Vec<_>>();
            for revision in &mut revisions {
                revision.chunk_indexes.sort_unstable();
                revision.chunk_indexes.dedup();
                revision.active = entry.active_revisions.contains(&revision.revision);
            }
            crate::brain_service::ProjectBrainSourceHistory {
                source_ref,
                source_kind: entry.source_kind.unwrap_or_else(|| "unknown".to_string()),
                revisions,
                node_count: entry.node_count,
                chunk_count: entry.chunk_count,
                latest_summary: snippet_text(&entry.latest_summary, 220),
            }
        })
        .collect()
}

fn build_knowledge_edges(
    nodes: &[crate::brain_service::ProjectBrainKnowledgeNode],
) -> Vec<crate::brain_service::ProjectBrainKnowledgeEdge> {
    let mut keyword_to_nodes =
        BTreeMap::<String, Vec<&crate::brain_service::ProjectBrainKnowledgeNode>>::new();
    for node in nodes {
        for keyword in &node.keywords {
            keyword_to_nodes
                .entry(keyword.to_string())
                .or_default()
                .push(node);
        }
    }

    let mut seen = HashSet::new();
    let mut edges = Vec::new();
    for (keyword, linked_nodes) in keyword_to_nodes {
        if linked_nodes.len() < 2 {
            continue;
        }
        for left in 0..linked_nodes.len() {
            for right in left + 1..linked_nodes.len() {
                let from = &linked_nodes[left].id;
                let to = &linked_nodes[right].id;
                let key = if from <= to {
                    format!("{}|{}|{}", from, to, keyword)
                } else {
                    format!("{}|{}|{}", to, from, keyword)
                };
                if !seen.insert(key) {
                    continue;
                }
                edges.push(crate::brain_service::ProjectBrainKnowledgeEdge {
                    from: from.clone(),
                    to: to.clone(),
                    relation: format!("shared_keyword:{}", keyword),
                    evidence_ref: keyword.clone(),
                });
            }
        }
    }
    edges
}

fn compare_project_brain_source_revisions_headless(
    data_dir: &Path,
    project_id: &str,
    source_ref: &str,
) -> Result<crate::brain_service::ProjectBrainSourceCompare, String> {
    let source_ref = source_ref.trim();
    if source_ref.is_empty() {
        return Err("Project Brain source ref is required for revision compare".to_string());
    }
    let brain = agent_harness_core::vector_db::VectorDB::load(&brain_path(data_dir, project_id)?)
        .map_err(|error| error.to_string())?;
    Ok(compare_project_brain_source_revisions_from_db(
        source_ref, &brain,
    ))
}

fn compare_project_brain_source_revisions_from_db(
    source_ref: &str,
    brain: &agent_harness_core::vector_db::VectorDB,
) -> crate::brain_service::ProjectBrainSourceCompare {
    #[derive(Default)]
    struct RevisionAccumulator {
        active: bool,
        node_count: usize,
        chunk_count: usize,
        chunk_indexes: Vec<usize>,
        keywords: Vec<String>,
        summary_parts: Vec<String>,
    }

    let source_ref = source_ref.trim();
    let mut source_kind = "unknown".to_string();
    let mut by_revision = BTreeMap::<String, RevisionAccumulator>::new();
    for chunk in brain
        .chunks
        .iter()
        .filter(|chunk| chunk.source_ref.as_deref() == Some(source_ref))
    {
        if let Some(kind) = chunk
            .source_kind
            .as_deref()
            .map(str::trim)
            .filter(|kind| !kind.is_empty())
        {
            source_kind = kind.to_string();
        }
        let revision = chunk
            .source_revision
            .as_deref()
            .map(str::trim)
            .filter(|revision| !revision.is_empty())
            .unwrap_or("unknown");
        let entry = by_revision.entry(revision.to_string()).or_default();
        entry.node_count += 1;
        entry.chunk_count += 1;
        if !chunk.archived {
            entry.active = true;
        }
        if let Some(chunk_index) = chunk.chunk_index {
            entry.chunk_indexes.push(chunk_index);
        }
        entry.keywords.extend(chunk.keywords.iter().cloned());
        if !chunk.text.trim().is_empty() {
            entry.summary_parts.push(chunk.text.clone());
        }
    }

    let mut revisions = by_revision
        .into_iter()
        .map(|(revision, mut entry)| {
            entry.chunk_indexes.sort_unstable();
            entry.chunk_indexes.dedup();
            let summary = snippet_text(
                &entry
                    .summary_parts
                    .iter()
                    .map(|part| part.trim())
                    .filter(|part| !part.is_empty())
                    .collect::<Vec<_>>()
                    .join("\n"),
                360,
            );
            crate::brain_service::ProjectBrainSourceCompareRevision {
                revision,
                active: entry.active,
                node_count: entry.node_count,
                chunk_count: entry.chunk_count,
                chunk_indexes: entry.chunk_indexes,
                keywords: normalized_limited_keywords(entry.keywords, 16),
                summary,
            }
        })
        .collect::<Vec<_>>();
    revisions.sort_by(|left, right| {
        right
            .active
            .cmp(&left.active)
            .then_with(|| left.revision.cmp(&right.revision))
    });

    let active_revision = revisions
        .iter()
        .find(|revision| revision.active)
        .map(|revision| revision.revision.clone());
    let active_keywords = revisions
        .iter()
        .find(|revision| revision.active)
        .map(|revision| normalized_keyword_set(&revision.keywords))
        .unwrap_or_default();
    let archived_keywords = revisions
        .iter()
        .filter(|revision| !revision.active)
        .flat_map(|revision| revision.keywords.iter().cloned())
        .collect::<Vec<_>>();
    let archived_keywords = normalized_keyword_set(&archived_keywords);

    let active_summary = revisions
        .iter()
        .find(|revision| revision.active)
        .map(|revision| revision.summary.clone())
        .unwrap_or_default();
    let archived_summary = revisions
        .iter()
        .filter(|revision| !revision.active)
        .map(|revision| revision.summary.as_str())
        .collect::<Vec<_>>()
        .join("\n");

    crate::brain_service::ProjectBrainSourceCompare {
        source_ref: source_ref.to_string(),
        source_kind,
        active_revision,
        revisions,
        added_keywords: active_keywords
            .difference(&archived_keywords)
            .take(12)
            .cloned()
            .collect(),
        removed_keywords: archived_keywords
            .difference(&active_keywords)
            .take(12)
            .cloned()
            .collect(),
        shared_keywords: active_keywords
            .intersection(&archived_keywords)
            .take(12)
            .cloned()
            .collect(),
        added_summary: compare_summary_terms(&active_summary, &archived_summary),
        removed_summary: compare_summary_terms(&archived_summary, &active_summary),
        evidence_refs: vec![format!("source_ref:{}", source_ref)],
    }
}

fn restore_project_brain_source_revision_headless(
    data_dir: &Path,
    project_id: &str,
    source_ref: &str,
    revision: &str,
) -> Result<crate::brain_service::ProjectBrainSourceRevisionRestore, String> {
    let path = brain_path(data_dir, project_id)?;
    let mut brain =
        agent_harness_core::vector_db::VectorDB::load(&path).map_err(|error| error.to_string())?;
    let report = restore_project_brain_source_revision_in_db(source_ref, revision, &mut brain)?;
    storage::atomic_write(
        &path,
        &serde_json::to_string_pretty(&brain.chunks).map_err(|error| error.to_string())?,
    )?;
    Ok(report)
}

fn restore_project_brain_source_revision_in_db(
    source_ref: &str,
    revision: &str,
    brain: &mut agent_harness_core::vector_db::VectorDB,
) -> Result<crate::brain_service::ProjectBrainSourceRevisionRestore, String> {
    let source_ref = source_ref.trim();
    let revision = revision.trim();
    if source_ref.is_empty() {
        return Err("Project Brain source ref is required for revision restore".to_string());
    }
    if revision.is_empty() {
        return Err("Project Brain source revision is required for revision restore".to_string());
    }

    let mut source_kind = "unknown".to_string();
    let mut previous_active_revisions = BTreeSet::new();
    let mut has_requested_revision = false;
    let mut total_source_chunk_count = 0usize;
    let mut active_chunk_count = 0usize;
    let mut archived_chunk_count = 0usize;
    let mut changed_chunk_count = 0usize;

    for chunk in &brain.chunks {
        if chunk.source_ref.as_deref() != Some(source_ref) {
            continue;
        }
        total_source_chunk_count += 1;
        if let Some(kind) = chunk
            .source_kind
            .as_deref()
            .map(str::trim)
            .filter(|kind| !kind.is_empty())
        {
            source_kind = kind.to_string();
        }
        if !chunk.archived {
            if let Some(active_revision) = chunk
                .source_revision
                .as_deref()
                .map(str::trim)
                .filter(|active_revision| !active_revision.is_empty())
            {
                previous_active_revisions.insert(active_revision.to_string());
            }
        }
        if chunk.source_revision.as_deref().map(str::trim) == Some(revision) {
            has_requested_revision = true;
        }
    }

    if total_source_chunk_count == 0 {
        return Err(format!(
            "Project Brain source '{}' has no indexed chunks to restore",
            source_ref
        ));
    }
    if !has_requested_revision {
        return Err(format!(
            "Project Brain source '{}' has no revision '{}'",
            source_ref, revision
        ));
    }

    for chunk in &mut brain.chunks {
        if chunk.source_ref.as_deref() != Some(source_ref) {
            continue;
        }
        let should_archive = chunk.source_revision.as_deref().map(str::trim) != Some(revision);
        if chunk.archived != should_archive {
            changed_chunk_count += 1;
            chunk.archived = should_archive;
        }
        if chunk.archived {
            archived_chunk_count += 1;
        } else {
            active_chunk_count += 1;
        }
    }

    Ok(crate::brain_service::ProjectBrainSourceRevisionRestore {
        source_ref: source_ref.to_string(),
        source_kind,
        restored_revision: revision.to_string(),
        previous_active_revisions: previous_active_revisions.into_iter().collect(),
        changed_chunk_count,
        active_chunk_count,
        archived_chunk_count,
        total_source_chunk_count,
        evidence_refs: vec![
            format!("source_ref:{}", source_ref),
            format!("source_revision:{}", revision),
        ],
    })
}

fn cross_reference_project_brain_nodes_headless(
    index: &crate::brain_service::ProjectBrainKnowledgeIndex,
    source_node_id: &str,
    target_node_id: &str,
) -> Result<crate::brain_service::ProjectBrainCrossReferenceResult, String> {
    let source_node_id = source_node_id.trim();
    let target_node_id = target_node_id.trim();
    if source_node_id.is_empty() || target_node_id.is_empty() {
        return Err("Both source and target node IDs are required for cross-reference".to_string());
    }
    if source_node_id == target_node_id {
        return Err("Cannot cross-reference a node with itself".to_string());
    }
    let source = index
        .nodes
        .iter()
        .find(|node| node.id == source_node_id)
        .ok_or_else(|| {
            format!(
                "Source node '{}' not found in knowledge index",
                source_node_id
            )
        })?;
    let target = index
        .nodes
        .iter()
        .find(|node| node.id == target_node_id)
        .ok_or_else(|| {
            format!(
                "Target node '{}' not found in knowledge index",
                target_node_id
            )
        })?;
    let shared_keywords = source
        .keywords
        .iter()
        .filter(|keyword| target.keywords.contains(keyword))
        .cloned()
        .collect::<Vec<_>>();
    let confidence = if shared_keywords.len() >= 3 {
        0.85
    } else if !shared_keywords.is_empty() {
        0.55
    } else {
        0.25
    };
    let relation = if source.kind == target.kind {
        "extends"
    } else if shared_keywords.len() >= 2 {
        "supports"
    } else if shared_keywords.is_empty() {
        "references"
    } else {
        "relates_to"
    };
    let suggested_action = if confidence >= 0.7 {
        format!(
            "Strong cross-reference: '{}' {} '{}' ({} shared keywords). Consider linking in context.",
            source.label,
            relation,
            target.label,
            shared_keywords.len()
        )
    } else if confidence >= 0.4 {
        format!(
            "Weak cross-reference: '{}' {} '{}'. Review connection before linking.",
            source.label, relation, target.label
        )
    } else {
        format!(
            "Minimal cross-reference: '{}' and '{}' share little evidence. Manual review recommended.",
            source.label, target.label
        )
    };

    Ok(crate::brain_service::ProjectBrainCrossReferenceResult {
        reference: crate::brain_service::ProjectBrainCrossReference {
            source_node_id: source_node_id.to_string(),
            target_node_id: target_node_id.to_string(),
            relation: relation.to_string(),
            confidence,
            evidence_keywords: shared_keywords.clone(),
            created_at_ms: now_ms(),
        },
        source_label: source.label.clone(),
        target_label: target.label.clone(),
        shared_keywords,
        suggested_action,
    })
}

fn ingest_external_research_source_headless(
    data_dir: &Path,
    project_id: &str,
    request: ExternalResearchIngestRequest,
) -> Result<crate::brain_service::ExternalResearchIngestResult, String> {
    let provider = request.provider.trim();
    let url_or_path = request.url_or_path.trim();
    let title = request.title.trim();
    let content = request.content.trim();
    let approval_reason = request.approval_reason.trim();
    if provider.is_empty() || title.is_empty() || content.is_empty() {
        return Err("External research provider, title, and content are all required".to_string());
    }
    if !request.author_approved {
        return Err("External research ingestion writes to Project Brain and requires explicit author approval".to_string());
    }
    if approval_reason.is_empty() {
        return Err("External research ingestion requires an author approval reason".to_string());
    }

    let revision = storage::content_revision(content);
    let source_ref = format!("external:{}:{}", provider, revision);
    let source_kind = "external_research";
    let content_snippet = content.chars().take(480).collect::<String>();
    let source = crate::brain_service::ExternalResearchSource {
        provider: provider.to_string(),
        url_or_path: url_or_path.to_string(),
        title: title.to_string(),
        content_snippet,
        relevance_score: 0.7,
        source_kind: source_kind.to_string(),
        ingestion_mode: "manual_author_approved".to_string(),
        author_approved: request.author_approved,
    };

    let path = brain_path(data_dir, project_id)?;
    let mut brain = agent_harness_core::vector_db::VectorDB::load(&path).map_err(|error| {
        format!(
            "Project Brain index at '{}' is unreadable: {}",
            path.display(),
            error
        )
    })?;
    let chunks = content
        .split("\n\n")
        .filter(|part| !part.trim().is_empty())
        .map(|part| part.trim().to_string())
        .collect::<Vec<_>>();
    let mut node_ids = Vec::new();
    for (index, chunk_text) in chunks.iter().enumerate() {
        let node_id = format!("{}:chunk:{}", source_ref, index);
        let keywords = chunk_text
            .split_whitespace()
            .filter(|word| word.chars().count() >= 2)
            .take(8)
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>();
        brain.chunks.push(agent_harness_core::vector_db::Chunk {
            id: node_id.clone(),
            chapter: title.to_string(),
            text: chunk_text.clone(),
            embedding: Vec::new(),
            keywords,
            topic: None,
            source_ref: Some(source_ref.clone()),
            source_revision: Some(revision.clone()),
            source_kind: Some(source_kind.to_string()),
            chunk_index: Some(index),
            archived: false,
        });
        node_ids.push(node_id);
    }

    storage::atomic_write(
        &path,
        &serde_json::to_string_pretty(&brain.chunks).map_err(|error| error.to_string())?,
    )?;
    let evidence_refs = node_ids
        .iter()
        .map(|id| format!("knowledge_node:{}", id))
        .collect::<Vec<_>>();
    Ok(crate::brain_service::ExternalResearchIngestResult {
        source,
        chunk_count: chunks.len(),
        node_ids,
        evidence_refs,
        created_at_ms: now_ms(),
    })
}

fn manual_quality_anchor_keywords(
    lorebook: &[LoreEntry],
    request: &crate::chapter_generation::ManualCraftEditFeedbackRequest,
) -> Vec<String> {
    let mut anchors = Vec::new();
    let combined = format!("{} {}", request.before_text, request.after_text);
    for entry in lorebook {
        if combined.contains(entry.keyword.as_str()) {
            push_unique_string(&mut anchors, &entry.keyword);
        }
    }
    for phrase in [
        "代价", "旧债", "真相", "秘密", "承诺", "背叛", "选择", "入口", "线索",
    ] {
        if combined.contains(phrase) {
            push_unique_string(&mut anchors, phrase);
        }
    }
    anchors.truncate(16);
    anchors
}

fn push_unique_string(values: &mut Vec<String>, value: &str) {
    let value = value.trim();
    if value.is_empty() || values.iter().any(|existing| existing == value) {
        return;
    }
    values.push(value.to_string());
}

fn repair_chapter_state_headless(
    backend: &HeadlessBackend,
    chapter_title: String,
) -> Result<RepairChapterStateResult, String> {
    let project = HeadlessChapterGenerationProject::new(&backend.config, &backend.project)?;
    let content = crate::chapter_generation::ChapterGenerationProject::load_chapter(
        &project,
        &chapter_title,
    )?;
    let revision = crate::chapter_generation::ChapterGenerationProject::chapter_revision(
        &project,
        &chapter_title,
    )?;
    let runtime_dir = project.project_data_dir.join("chapter_runtime");
    if let Ok(entries) = std::fs::read_dir(&runtime_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if !name.ends_with(".settlement.json") {
                continue;
            }
            let raw = match std::fs::read_to_string(entry.path()) {
                Ok(raw) => raw,
                Err(_) => continue,
            };
            let existing: crate::chapter_generation::ChapterSettlementDelta =
                match serde_json::from_str(&raw) {
                    Ok(delta) => delta,
                    Err(_) => continue,
                };
            if existing.chapter_title == chapter_title && existing.chapter_revision == revision {
                return Ok(RepairChapterStateResult {
                    chapter_title,
                    revision,
                    settlement_delta: existing,
                    settlement_apply: Default::default(),
                    artifact_refs: Vec::new(),
                    settlement_replay_matches: None,
                    already_repaired: true,
                });
            }
        }
    }

    let memory = WriterMemory::open(
        crate::chapter_generation::ChapterGenerationProject::memory_path(&project),
    )
    .map_err(|error| error.to_string())?;
    let context = tokio::runtime::Handle::current().block_on(async {
        crate::chapter_generation::build_chapter_context(
            &project,
            crate::chapter_generation::BuildChapterContextInput {
                request_id: crate::chapter_generation::make_request_id("repair-state"),
                target_chapter_title: Some(chapter_title.clone()),
                target_chapter_number: None,
                user_instruction: format!(
                    "Repair chapter settlement state for '{}' without rewriting the chapter.",
                    chapter_title
                ),
                budget: crate::chapter_generation::ChapterContextBudget::default(),
                chapter_contract: crate::chapter_generation::ChapterContract::default(),
                chapter_summary_override: None,
                user_profile_entries: backend.user_profile_entries(),
                compiled_input: None,
                open_promise_count: memory
                    .get_open_promises()
                    .ok()
                    .map(|promises| promises.len())
                    .unwrap_or(0),
            },
        )
        .await
        .map_err(|error| error.message.clone())
    })?;
    let saved = crate::chapter_generation::SaveGeneratedChapterOutput {
        chapter_title: chapter_title.clone(),
        new_revision: revision.clone(),
        saved_mode: "repaired_state".to_string(),
        output_chars: content.chars().count(),
    };
    let delta = crate::chapter_generation::build_basic_chapter_settlement_delta(
        &backend.project.id,
        &chapter_title,
        &revision,
        &content,
        now_ms(),
        &memory,
        Vec::new(),
    );
    let chronology_before = memory
        .list_recent_chapter_results(&backend.project.id, 20)
        .unwrap_or_default()
        .into_iter()
        .map(|result| result.chapter_title)
        .collect::<Vec<_>>();
    let settlement_apply = crate::writer_agent::settlement_apply::apply_chapter_settlement_delta(
        &memory,
        &backend.project.id,
        &delta,
    )?;
    let replay = crate::chapter_generation::replay_settlement_extraction(&delta, &content, &memory);
    if !replay.matches_original {
        tracing::warn!(
            chapter = chapter_title,
            mismatches = replay.mismatches.join("; "),
            "Headless settlement replay produced different results"
        );
    }

    let telemetry = crate::chapter_generation::ChapterLengthTelemetry {
        target_chars: context.chapter_contract.target_chars,
        min_chars: context.chapter_contract.min_chars,
        max_chars: context.chapter_contract.max_chars,
        save_hard_floor_chars: context.chapter_contract.save_hard_floor_chars,
        save_hard_ceiling_chars: context.chapter_contract.save_hard_ceiling_chars,
        draft_chars: None,
        final_chars: Some(saved.output_chars),
        continuation_applied: false,
        compress_applied: false,
        hard_compress_applied: false,
        phase_telemetry: Default::default(),
        warning: None,
    };
    let artifacts = crate::chapter_generation::persist_chapter_runtime_artifacts(
        &project,
        &context.request_id,
        &context,
        &delta,
        &telemetry,
        &content,
    )?;
    if let Ok(mut kernel) = backend.lock_kernel() {
        kernel
            .memory
            .record_decision(
                &chapter_title,
                &format!("Repair chapter state: {}", chapter_title),
                "repair_chapter_state",
                &[],
                &format!(
                    "Rebuilt settlement/runtime artifacts for '{}' at revision {} without rewriting chapter text.",
                    chapter_title, revision
                ),
                &artifacts.artifact_refs,
            )
            .ok();
        let observation =
            chapter_save_observation(&backend.project.id, &chapter_title, &revision, &content);
        let result = crate::writer_agent::memory::ChapterResultSummary {
            id: 0,
            project_id: backend.project.id.clone(),
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
            created_at: now_ms(),
        };
        let _ = kernel.observe_save_result(observation, result);
    }
    let chronology_after = memory
        .list_recent_chapter_results(&backend.project.id, 20)
        .unwrap_or_default()
        .into_iter()
        .map(|result| result.chapter_title)
        .collect::<Vec<_>>();
    if chronology_before != chronology_after {
        tracing::error!(
            chapter = chapter_title,
            before = ?chronology_before,
            after = ?chronology_after,
            "headless repair_chapter_state altered chapter chronology"
        );
    }

    Ok(RepairChapterStateResult {
        chapter_title,
        revision,
        settlement_delta: delta,
        settlement_apply,
        artifact_refs: artifacts.artifact_refs,
        settlement_replay_matches: Some(replay.matches_original),
        already_repaired: false,
    })
}

async fn embed_chapter_headless(
    data_dir: &Path,
    project_id: &str,
    settings: &crate::llm_runtime::LlmSettings,
    chapter_title: &str,
    content: &str,
) -> Result<(), String> {
    let chunks = agent_harness_core::chunk_text(content, 500);
    if chunks.is_empty() {
        return Ok(());
    }

    let (embedded_chunks, report) =
        crate::brain_service::embed_project_brain_chunks(settings, chapter_title, &chunks, 30)
            .await;
    if !matches!(
        report.status,
        crate::brain_service::ProjectBrainEmbeddingBatchStatus::Complete
    ) {
        tracing::warn!(
            "Headless Project Brain embedding batch for '{}' finished with {:?}: embedded={} skipped={} truncated={} errors={:?}",
            chapter_title,
            report.status,
            report.embedded_count,
            report.skipped_count,
            report.truncated_count,
            report.errors
        );
    }
    if embedded_chunks.is_empty() {
        return Ok(());
    }

    let path = brain_path(data_dir, project_id)?;
    let mut db = agent_harness_core::vector_db::VectorDB::load(&path).map_err(|error| {
        format!(
            "Project Brain index at '{}' is unreadable; restore a backup or rebuild the index: {}",
            path.display(),
            error
        )
    })?;
    let active_revision = embedded_chunks
        .first()
        .and_then(|chunk| chunk.source_revision.as_deref())
        .unwrap_or_default()
        .to_string();
    db.archive_chapter_revision(chapter_title, &active_revision);
    for chunk in embedded_chunks {
        db.upsert(chunk);
    }
    db.save(&path)
}

fn stable_node_id(left: &str, right: &str) -> String {
    storage::content_revision(&format!("{}:{}", left, right))
        .split('-')
        .next()
        .unwrap_or("0000000000000000")
        .to_string()
}

fn unique_keywords(mut seed: Vec<String>, text: &str) -> Vec<String> {
    seed.extend(agent_harness_core::extract_keywords(text));
    let mut seen = HashSet::new();
    seed.into_iter()
        .map(|keyword| keyword.trim().to_string())
        .filter(|keyword| keyword.chars().count() >= 2 && seen.insert(keyword.to_lowercase()))
        .take(12)
        .collect()
}

fn normalized_limited_keywords(seed: Vec<String>, limit: usize) -> Vec<String> {
    let mut seen = HashSet::new();
    seed.into_iter()
        .flat_map(|keyword| unique_keywords(vec![keyword.clone()], &keyword))
        .map(|keyword| keyword.trim().to_string())
        .filter(|keyword| keyword.chars().count() >= 2 && seen.insert(keyword.to_lowercase()))
        .take(limit)
        .collect()
}

fn normalized_keyword_set(seed: &[String]) -> BTreeSet<String> {
    normalized_limited_keywords(seed.to_vec(), 64)
        .into_iter()
        .collect()
}

fn compare_summary_terms(primary: &str, baseline: &str) -> Vec<String> {
    let baseline_terms = normalized_keyword_set(&agent_harness_core::extract_keywords(baseline));
    normalized_limited_keywords(agent_harness_core::extract_keywords(primary), 24)
        .into_iter()
        .filter(|term| !baseline_terms.contains(term))
        .take(8)
        .collect()
}

fn snippet_text(text: &str, limit: usize) -> String {
    text.chars().take(limit).collect()
}

fn write_json_pretty<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let json = serde_json::to_string_pretty(value).map_err(|error| error.to_string())?;
    storage::atomic_write(path, &json)
}

fn validate_backup_content(
    target: &storage::BackupTarget,
    content: &str,
    backup_path: &Path,
) -> Result<(), String> {
    match target {
        storage::BackupTarget::Lorebook => serde_json::from_str::<Vec<LoreEntry>>(content)
            .map(|_| ())
            .map_err(|error| {
                format!(
                    "Invalid lorebook backup '{}': {}",
                    backup_path.display(),
                    error
                )
            }),
        storage::BackupTarget::Outline => serde_json::from_str::<Vec<OutlineNode>>(content)
            .map(|_| ())
            .map_err(|error| {
                format!(
                    "Invalid outline backup '{}': {}",
                    backup_path.display(),
                    error
                )
            }),
        storage::BackupTarget::ProjectBrain => {
            serde_json::from_str::<Vec<agent_harness_core::vector_db::Chunk>>(content)
                .map(|_| ())
                .map_err(|error| {
                    format!(
                        "Invalid project brain backup '{}': {}",
                        backup_path.display(),
                        error
                    )
                })
        }
        storage::BackupTarget::Chapter { .. } => Ok(()),
    }
}

fn diagnose_json_array_file<T: for<'de> Deserialize<'de>>(
    label: &str,
    path: &Path,
) -> storage::StorageFileDiagnostic {
    let mut diagnostic = base_file_diagnostic(label, path);
    if !diagnostic.exists {
        diagnostic.status = "missing".to_string();
        return diagnostic;
    }

    match std::fs::read_to_string(path) {
        Ok(data) => match serde_json::from_str::<Vec<T>>(&data) {
            Ok(rows) => {
                diagnostic.record_count = Some(rows.len());
                diagnostic.status = "ok".to_string();
            }
            Err(error) => {
                diagnostic.status = "error".to_string();
                diagnostic.error = Some(format!("JSON parse failed: {}", error));
            }
        },
        Err(error) => {
            diagnostic.status = "error".to_string();
            diagnostic.error = Some(format!("Read failed: {}", error));
        }
    }

    diagnostic
}

fn diagnose_chapters_directory(path: &Path) -> storage::StorageFileDiagnostic {
    let mut diagnostic = base_file_diagnostic("chapters", path);
    if !diagnostic.exists {
        diagnostic.status = "missing".to_string();
        return diagnostic;
    }
    if !path.is_dir() {
        diagnostic.status = "error".to_string();
        diagnostic.error = Some("Expected a directory".to_string());
        return diagnostic;
    }

    match std::fs::read_dir(path) {
        Ok(entries) => {
            let mut count = 0usize;
            let mut bytes = 0u64;
            for entry in entries.flatten() {
                let entry_path = entry.path();
                if entry_path
                    .extension()
                    .is_some_and(|extension| extension == "md")
                {
                    count += 1;
                    if let Ok(metadata) = entry.metadata() {
                        bytes = bytes.saturating_add(metadata.len());
                    }
                }
            }
            diagnostic.record_count = Some(count);
            diagnostic.bytes = Some(bytes);
            diagnostic.status = "ok".to_string();
        }
        Err(error) => {
            diagnostic.status = "error".to_string();
            diagnostic.error = Some(format!("Read directory failed: {}", error));
        }
    }

    diagnostic
}

fn diagnose_sqlite_database(
    label: &str,
    path: &Path,
    tables: &[&str],
) -> storage::SqliteDatabaseDiagnostic {
    let mut diagnostic = storage::SqliteDatabaseDiagnostic {
        label: label.to_string(),
        path: path.to_string_lossy().to_string(),
        exists: path.exists(),
        bytes: file_size(path),
        user_version: None,
        quick_check: None,
        table_counts: vec![],
        status: "unknown".to_string(),
        error: None,
    };
    if !diagnostic.exists {
        diagnostic.status = "missing".to_string();
        diagnostic.error = Some("Database file is missing".to_string());
        return diagnostic;
    }

    let conn = match Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY) {
        Ok(conn) => conn,
        Err(error) => {
            diagnostic.status = "error".to_string();
            diagnostic.error = Some(format!("Open failed: {}", error));
            return diagnostic;
        }
    };

    diagnostic.user_version = conn
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .ok();
    match conn.query_row("PRAGMA quick_check", [], |row| row.get::<_, String>(0)) {
        Ok(result) => diagnostic.quick_check = Some(result),
        Err(error) => {
            diagnostic.status = "error".to_string();
            diagnostic.error = Some(format!("quick_check failed: {}", error));
            return diagnostic;
        }
    }

    for table in tables {
        match sqlite_table_row_count(&conn, table) {
            Ok(Some(rows)) => diagnostic.table_counts.push(storage::SqliteTableCount {
                table: (*table).to_string(),
                rows,
            }),
            Ok(None) => {}
            Err(error) => {
                diagnostic.status = "error".to_string();
                diagnostic.error = Some(format!("Count failed for '{}': {}", table, error));
                return diagnostic;
            }
        }
    }

    if diagnostic.quick_check.as_deref() == Some("ok") {
        diagnostic.status = "ok".to_string();
    } else {
        diagnostic.status = "error".to_string();
        diagnostic.error = diagnostic
            .quick_check
            .as_ref()
            .map(|check| format!("SQLite quick_check reported '{}'", check));
    }
    diagnostic
}

fn sqlite_table_row_count(conn: &Connection, table: &str) -> rusqlite::Result<Option<u64>> {
    if !sqlite_table_exists(conn, table)? {
        return Ok(None);
    }
    let count: i64 = conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
        row.get(0)
    })?;
    Ok(Some(count.max(0) as u64))
}

fn sqlite_table_exists(conn: &Connection, table: &str) -> rusqlite::Result<bool> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
        rusqlite::params![table],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}

fn base_file_diagnostic(label: &str, path: &Path) -> storage::StorageFileDiagnostic {
    storage::StorageFileDiagnostic {
        label: label.to_string(),
        path: path.to_string_lossy().to_string(),
        exists: path.exists(),
        bytes: file_size(path),
        record_count: None,
        backup_count: backup_count(path),
        status: "unknown".to_string(),
        error: None,
    }
}

fn file_size(path: &Path) -> Option<u64> {
    std::fs::metadata(path).ok().map(|metadata| metadata.len())
}

fn backup_count(path: &Path) -> usize {
    let Ok(dir) = backup_dir_for(path) else {
        return 0;
    };
    std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .filter(|entry| entry.path().is_file())
                .count()
        })
        .unwrap_or(0)
}

fn backup_dir_for(path: &Path) -> Result<PathBuf, String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("Path '{}' has no parent directory", path.display()))?;
    let file_stem = path
        .file_name()
        .ok_or_else(|| format!("Path '{}' has no filename", path.display()))?
        .to_string_lossy()
        .to_string();
    Ok(parent
        .join(".backups")
        .join(safe_backup_segment(&file_stem)))
}

fn safe_backup_segment(name: &str) -> String {
    name.chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.' {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

fn safe_backup_file_path(backup_dir: &Path, backup_id: &str) -> Result<PathBuf, String> {
    let path = Path::new(backup_id);
    if path.components().count() != 1 || path.file_name().is_none() {
        return Err(format!("Invalid backup id: {}", backup_id));
    }
    Ok(backup_dir.join(path))
}

fn backup_info(entry: std::fs::DirEntry) -> Result<storage::FileBackupInfo, String> {
    let path = entry.path();
    let metadata = entry.metadata().map_err(|error| {
        format!(
            "Failed to read backup metadata '{}': {}",
            path.display(),
            error
        )
    })?;
    let modified_at = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0);
    let filename = entry.file_name().to_string_lossy().to_string();
    Ok(storage::FileBackupInfo {
        id: filename.clone(),
        filename,
        path: path.to_string_lossy().to_string(),
        bytes: metadata.len(),
        modified_at,
    })
}

fn seed_story_model_if_empty(data_dir: &Path, project: &ProjectManifest, memory: &WriterMemory) {
    let lorebook = read_json_array(&lorebook_path(data_dir, &project.id).unwrap_or_default())
        .unwrap_or_default();
    let outline = read_json_array(&outline_path(data_dir, &project.id).unwrap_or_default())
        .unwrap_or_default();
    if let Err(error) = crate::writer_agent::context::seed_story_contract_from_project_assets(
        &project.id,
        &project.name,
        &lorebook,
        &outline,
        memory,
    ) {
        tracing::warn!("Story contract seed skipped: {}", error);
    }
    if let Err(error) = crate::writer_agent::context::seed_chapter_missions_from_outline(
        &project.id,
        &outline,
        memory,
    ) {
        tracing::warn!("Chapter mission seed skipped: {}", error);
    }
    if let Err(error) = memory.ensure_default_book_state(&project.id, &project.name) {
        tracing::warn!("Book state seed skipped: {}", error);
    }
}

fn chapter_save_observation(
    project_id: &str,
    title: &str,
    revision: &str,
    content: &str,
) -> WriterObservation {
    let plain = html_to_plain_text(content);
    let paragraph = last_non_empty_paragraph(&plain);
    let cursor = plain.chars().count();
    WriterObservation {
        id: format!("save-{}-{}", sanitize_id_segment(title), now_ms()),
        created_at: now_ms(),
        source: crate::writer_agent::observation::ObservationSource::ChapterSave,
        reason: crate::writer_agent::observation::ObservationReason::Save,
        project_id: project_id.to_string(),
        chapter_title: Some(title.to_string()),
        chapter_revision: Some(revision.to_string()),
        cursor: Some(crate::writer_agent::observation::TextRange {
            from: cursor,
            to: cursor,
        }),
        selection: None,
        prefix: text_tail(&plain, 3_000),
        suffix: String::new(),
        paragraph,
        full_text_digest: Some(storage::content_revision(&plain)),
        editor_dirty: false,
    }
}

fn html_to_plain_text(html: &str) -> String {
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

fn decode_html_entity(entity: &str) -> String {
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

fn last_non_empty_paragraph(text: &str) -> String {
    text.rsplit("\n\n")
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| text_tail(text, 1_200))
}

fn text_tail(text: &str, max_chars: usize) -> String {
    let chars = text.chars().collect::<Vec<_>>();
    let start = chars.len().saturating_sub(max_chars);
    chars[start..].iter().collect()
}

fn sanitize_id_segment(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

fn safe_filename_component(value: &str) -> String {
    let sanitized = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.' {
                ch
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string();
    if sanitized.is_empty() {
        "unnamed".to_string()
    } else {
        sanitized
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn required_string(params: &serde_json::Value, key: &str) -> Result<String, String> {
    params
        .get(key)
        .and_then(|value| value.as_str())
        .map(ToOwned::to_owned)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("{} is required", key))
}

fn string_arg<'a>(args: &'a serde_json::Value, names: &[&str]) -> &'a str {
    names
        .iter()
        .find_map(|name| args.get(*name).and_then(|value| value.as_str()))
        .unwrap_or("")
}

fn required_string_any(params: &serde_json::Value, keys: &[&str]) -> Result<String, String> {
    for key in keys {
        if let Some(value) = params
            .get(*key)
            .and_then(|value| value.as_str())
            .map(ToOwned::to_owned)
            .filter(|value| !value.trim().is_empty())
        {
            return Ok(value);
        }
    }
    Err(format!("{} is required", keys.join(" or ")))
}

fn optional_string_any(params: &serde_json::Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        params
            .get(*key)
            .and_then(|value| value.as_str())
            .map(ToOwned::to_owned)
            .filter(|value| !value.trim().is_empty())
    })
}

fn required_u64_any(params: &serde_json::Value, keys: &[&str]) -> Result<u64, String> {
    for key in keys {
        if let Some(value) = params.get(*key).and_then(|value| value.as_u64()) {
            return Ok(value);
        }
    }
    Err(format!("{} is required", keys.join(" or ")))
}

fn string_array_any(params: &serde_json::Value, keys: &[&str]) -> Result<Vec<String>, String> {
    for key in keys {
        if let Some(values) = params.get(*key).and_then(|value| value.as_array()) {
            return values
                .iter()
                .map(|value| {
                    value
                        .as_str()
                        .map(ToOwned::to_owned)
                        .filter(|value| !value.trim().is_empty())
                        .ok_or_else(|| format!("{} must contain only non-empty strings", key))
                })
                .collect();
        }
    }
    Err(format!("{} is required", keys.join(" or ")))
}

fn optional_usize(params: &serde_json::Value, key: &str) -> Option<usize> {
    params
        .get(key)
        .and_then(|value| value.as_u64())
        .map(|value| value as usize)
}

fn to_value<T: Serialize>(value: T) -> Result<serde_json::Value, String> {
    serde_json::to_value(value).map_err(|error| error.to_string())
}
