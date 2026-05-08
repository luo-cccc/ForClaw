const MAX_TRAVERSAL_DEPTH: usize = 4;
const DEFAULT_IMPACT_BUDGET_CHARS: usize = 2_400;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WriterStoryGraphNode {
    pub id: String,
    pub kind: StoryNodeKind,
    pub label: String,
    pub source_ref: String,
    pub source_revision: Option<String>,
    pub chapter: Option<String>,
    pub confidence: f32,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum StoryNodeKind {
    CanonEntity,
    CanonRule,
    PlotPromise,
    ChapterMission,
    ResultFeedback,
    ProjectBrainChunk,
    Decision,
    StoryContract,
    SeedTask,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WriterStoryGraphEdge {
    pub from: String,
    pub to: String,
    pub kind: StoryEdgeKind,
    pub evidence_ref: String,
    pub confidence: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum StoryEdgeKind {
    MentionsEntity,
    UpdatesPromise,
    SupportsMission,
    ContradictsCanon,
    DependsOnResult,
    SameSourceRevision,
    SharedKeyword,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WriterStoryImpactRadius {
    pub seed_nodes: Vec<WriterStoryGraphNode>,
    pub impacted_nodes: Vec<WriterStoryGraphNode>,
    pub impacted_sources: Vec<String>,
    pub dropped_nodes: Vec<WriterStoryGraphNode>,
    pub edges: Vec<WriterStoryGraphEdge>,
    pub risk: StoryImpactRisk,
    pub truncated: bool,
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum StoryImpactRisk {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StoryImpactBudgetReport {
    pub budget_limit: usize,
    pub requested_chars: usize,
    pub provided_chars: usize,
    pub truncated_node_count: usize,
    pub dropped_high_risk_sources: Vec<String>,
    pub reasons: Vec<String>,
}

impl StoryNodeKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            StoryNodeKind::CanonEntity => "canon_entity",
            StoryNodeKind::CanonRule => "canon_rule",
            StoryNodeKind::PlotPromise => "plot_promise",
            StoryNodeKind::ChapterMission => "chapter_mission",
            StoryNodeKind::ResultFeedback => "result_feedback",
            StoryNodeKind::ProjectBrainChunk => "project_brain_chunk",
            StoryNodeKind::Decision => "decision",
            StoryNodeKind::StoryContract => "story_contract",
            StoryNodeKind::SeedTask => "seed_task",
        }
    }
}

impl StoryEdgeKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            StoryEdgeKind::MentionsEntity => "mentions_entity",
            StoryEdgeKind::UpdatesPromise => "updates_promise",
            StoryEdgeKind::SupportsMission => "supports_mission",
            StoryEdgeKind::ContradictsCanon => "contradicts_canon",
            StoryEdgeKind::DependsOnResult => "depends_on_result",
            StoryEdgeKind::SameSourceRevision => "same_source_revision",
            StoryEdgeKind::SharedKeyword => "shared_keyword",
        }
    }
}

