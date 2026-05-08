
pub use crate::storage::{LoreEntry, OutlineNode};

const CHUNK_MAX_CHARS: usize = 500;
const MIN_CHUNK_CHARS: usize = 20;
const TOP_K: usize = 5;
const RERANK_CANDIDATE_MULTIPLIER: usize = 6;
const KNOWLEDGE_INDEX_FILENAME: &str = "knowledge_index.json";
const DEFAULT_EMBEDDING_DIMENSIONS: usize = 1536;
const DEFAULT_EMBEDDING_INPUT_LIMIT_CHARS: usize = 8_000;

#[derive(Debug, Clone)]
pub struct ProjectBrainFocus {
    query_text: String,
    memory_text: String,
    active_chapter: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectBrainKnowledgeIndex {
    pub project_id: String,
    pub nodes: Vec<ProjectBrainKnowledgeNode>,
    pub edges: Vec<ProjectBrainKnowledgeEdge>,
    #[serde(default)]
    pub source_history: Vec<ProjectBrainSourceHistory>,
    pub source_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectBrainKnowledgeNode {
    pub id: String,
    pub kind: String,
    pub label: String,
    pub source_ref: String,
    #[serde(default)]
    pub source_revision: Option<String>,
    #[serde(default)]
    pub source_kind: Option<String>,
    #[serde(default)]
    pub chunk_index: Option<usize>,
    #[serde(default)]
    pub archived: bool,
    pub keywords: Vec<String>,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectBrainKnowledgeEdge {
    pub from: String,
    pub to: String,
    pub relation: String,
    pub evidence_ref: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectBrainSourceHistory {
    pub source_ref: String,
    pub source_kind: String,
    pub revisions: Vec<ProjectBrainSourceRevision>,
    pub node_count: usize,
    pub chunk_count: usize,
    pub latest_summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectBrainSourceRevision {
    pub revision: String,
    pub node_count: usize,
    pub chunk_indexes: Vec<usize>,
    #[serde(default)]
    pub active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectBrainSourceCompare {
    pub source_ref: String,
    pub source_kind: String,
    pub active_revision: Option<String>,
    pub revisions: Vec<ProjectBrainSourceCompareRevision>,
    pub added_keywords: Vec<String>,
    pub removed_keywords: Vec<String>,
    pub shared_keywords: Vec<String>,
    pub added_summary: Vec<String>,
    pub removed_summary: Vec<String>,
    pub evidence_refs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectBrainSourceCompareRevision {
    pub revision: String,
    pub active: bool,
    pub node_count: usize,
    pub chunk_count: usize,
    pub chunk_indexes: Vec<usize>,
    pub keywords: Vec<String>,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectBrainSourceRevisionRestore {
    pub source_ref: String,
    pub source_kind: String,
    pub restored_revision: String,
    pub previous_active_revisions: Vec<String>,
    pub changed_chunk_count: usize,
    pub active_chunk_count: usize,
    pub archived_chunk_count: usize,
    pub total_source_chunk_count: usize,
    pub evidence_refs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectBrainCrossReference {
    pub source_node_id: String,
    pub target_node_id: String,
    pub relation: String,
    pub confidence: f64,
    pub evidence_keywords: Vec<String>,
    pub created_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectBrainCrossReferenceResult {
    pub reference: ProjectBrainCrossReference,
    pub source_label: String,
    pub target_label: String,
    pub shared_keywords: Vec<String>,
    pub suggested_action: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ExternalResearchSource {
    pub provider: String,
    pub url_or_path: String,
    pub title: String,
    pub content_snippet: String,
    pub relevance_score: f64,
    pub source_kind: String,
    pub ingestion_mode: String,
    pub author_approved: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ExternalResearchIngestResult {
    pub source: ExternalResearchSource,
    pub chunk_count: usize,
    pub node_ids: Vec<String>,
    pub evidence_refs: Vec<String>,
    pub created_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ProjectBrainEmbeddingBatchStatus {
    Complete,
    Partial,
    Empty,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectBrainEmbeddingProviderProfile {
    pub provider_id: String,
    pub model: String,
    pub dimensions: usize,
    pub input_limit_chars: usize,
    pub batch_limit: usize,
    pub retry_limit: usize,
    pub provider_status: ProjectBrainEmbeddingRegistryStatus,
    pub model_status: ProjectBrainEmbeddingRegistryStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ProjectBrainEmbeddingRegistryStatus {
    RegistryKnown,
    CompatibilityFallback,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectBrainEmbeddingModelSpec {
    pub model: String,
    pub dimensions: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectBrainEmbeddingProviderSpec {
    pub provider_id: String,
    pub api_base_markers: Vec<String>,
    pub default_input_limit_chars: usize,
    pub batch_limit: usize,
    pub retry_limit: usize,
    pub models: Vec<ProjectBrainEmbeddingModelSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectBrainEmbeddingProviderRegistry {
    pub providers: Vec<ProjectBrainEmbeddingProviderSpec>,
    pub fallback_provider_id: String,
    pub fallback_dimensions: usize,
    pub fallback_input_limit_chars: usize,
    pub fallback_batch_limit: usize,
    pub fallback_retry_limit: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectBrainEmbeddingBatchReport {
    pub profile: ProjectBrainEmbeddingProviderProfile,
    pub requested_count: usize,
    pub embedded_count: usize,
    pub skipped_count: usize,
    pub truncated_count: usize,
    pub status: ProjectBrainEmbeddingBatchStatus,
    pub errors: Vec<String>,
}

impl ProjectBrainFocus {
    pub fn from_query(query: &str) -> Self {
        Self {
            query_text: query.trim().to_string(),
            memory_text: String::new(),
            active_chapter: None,
        }
    }

    pub fn from_kernel(query: &str, kernel: &WriterAgentKernel) -> Self {
        let mut memory_parts = Vec::new();
        if let Some(chapter) = kernel.active_chapter.as_deref() {
            memory_parts.push(format!("active chapter: {}", chapter));
            if let Ok(Some(mission)) = kernel
                .memory
                .get_chapter_mission(&kernel.project_id, chapter)
            {
                memory_parts.push(mission.render_for_context());
            }
        }
        let recent_results = kernel
            .memory
            .list_recent_chapter_results(&kernel.project_id, 4)
            .unwrap_or_default();
        if let Some(next_beat) = crate::writer_agent::kernel::derive_next_beat(
            kernel.active_chapter.as_deref(),
            None,
            &recent_results,
            &kernel
                .memory
                .get_open_promise_summaries()
                .unwrap_or_default(),
        ) {
            memory_parts.push(next_beat.render_for_context());
        }
        for result in recent_results.iter().take(2) {
            memory_parts.push(result.render_for_context());
        }
        for decision in kernel.memory.list_recent_decisions(4).unwrap_or_default() {
            memory_parts.push(format!(
                "recent decision: {} {} {}",
                decision.title, decision.decision, decision.rationale
            ));
        }
        Self {
            query_text: query.trim().to_string(),
            memory_text: memory_parts
                .into_iter()
                .filter(|part| !part.trim().is_empty())
                .collect::<Vec<_>>()
                .join("\n"),
            active_chapter: kernel.active_chapter.clone(),
        }
    }

    pub fn as_str(&self) -> &str {
        if self.memory_text.trim().is_empty() {
            self.query_text.as_str()
        } else {
            self.memory_text.as_str()
        }
    }

    fn query_str(&self) -> &str {
        self.query_text.as_str()
    }

    fn memory_str(&self) -> &str {
        self.memory_text.as_str()
    }

    pub fn search_text(&self) -> String {
        [self.query_str(), self.memory_str()]
            .into_iter()
            .filter(|part| !part.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n")
    }
}
