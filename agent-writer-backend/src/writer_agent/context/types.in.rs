#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextExcerpt {
    pub source: ContextSource,
    pub content: String,
    pub char_count: usize,
    pub truncated: bool,
    pub priority: u8,
    pub evidence_ref: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ContextSource {
    SystemContract,
    ProjectBrief,
    ChapterMission,
    NextBeat,
    ResultFeedback,
    AuthorStyle,
    CanonSlice,
    PromiseSlice,
    DecisionSlice,
    BookState,
    ArcSnapshot,
    VolumeSnapshot,
    OutlineSlice,
    RagExcerpt,
    CursorPrefix,
    CursorSuffix,
    SelectedText,
    PreviousChapter,
    NextChapter,
    NeighborText,
    StoryImpactRadius,
    ReaderCompensation,
}

/// Task type determines which sources to include and their priority order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AgentTask {
    GhostWriting,
    ContinuityDiagnostic,
    ChapterGeneration,
    InlineRewrite,
    PlanningReview,
    ProposalEvaluation,
    CanonMaintenance,
    ManualRequest,
}

impl AgentTask {
    pub fn default_budget(&self) -> usize {
        match self {
            AgentTask::GhostWriting => 3_000,
            AgentTask::ContinuityDiagnostic => 2_500,
            AgentTask::ChapterGeneration => 20_000,
            AgentTask::InlineRewrite => 4_500,
            AgentTask::PlanningReview => 6_000,
            AgentTask::ProposalEvaluation => 1_400,
            AgentTask::CanonMaintenance => 4_500,
            AgentTask::ManualRequest => 4_500,
        }
    }

    pub fn source_priorities(&self) -> Vec<(ContextSource, u8, usize)> {
        match self {
            AgentTask::GhostWriting => vec![
                (ContextSource::CursorPrefix, 10, 800),
                (ContextSource::ChapterMission, 10, 520),
                (ContextSource::NextBeat, 10, 420),
                (ContextSource::StoryImpactRadius, 9, 420),
                (ContextSource::ResultFeedback, 9, 420),
                (ContextSource::ProjectBrief, 9, 420),
                (ContextSource::BookState, 8, 320),
                (ContextSource::ArcSnapshot, 8, 320),
                (ContextSource::VolumeSnapshot, 7, 280),
                (ContextSource::CursorSuffix, 9, 400),
                (ContextSource::CanonSlice, 8, 600),
                (ContextSource::PromiseSlice, 7, 400),
                (ContextSource::DecisionSlice, 7, 300),
                (ContextSource::OutlineSlice, 6, 500),
                (ContextSource::AuthorStyle, 5, 300),
                (ContextSource::ReaderCompensation, 5, 250),
                (ContextSource::RagExcerpt, 4, 400),
            ],
            AgentTask::ContinuityDiagnostic => vec![
                (ContextSource::CursorPrefix, 10, 300),
                (ContextSource::CanonSlice, 10, 800),
                (ContextSource::StoryImpactRadius, 9, 500),
                (ContextSource::ChapterMission, 9, 420),
                (ContextSource::NextBeat, 9, 420),
                (ContextSource::ResultFeedback, 9, 420),
                (ContextSource::ProjectBrief, 9, 400),
                (ContextSource::BookState, 8, 500),
                (ContextSource::ArcSnapshot, 8, 420),
                (ContextSource::VolumeSnapshot, 7, 360),
                (ContextSource::DecisionSlice, 9, 300),
                (ContextSource::OutlineSlice, 9, 500),
                (ContextSource::RagExcerpt, 8, 600),
                (ContextSource::ReaderCompensation, 7, 300),
            ],
            AgentTask::ChapterGeneration => vec![
                (ContextSource::ProjectBrief, 11, 1600),
                (ContextSource::BookState, 11, 1200),
                (ContextSource::ChapterMission, 11, 2200),
                (ContextSource::ArcSnapshot, 10, 1000),
                (ContextSource::VolumeSnapshot, 10, 900),
                (ContextSource::NextBeat, 11, 1600),
                (ContextSource::StoryImpactRadius, 10, 1200),
                (ContextSource::ResultFeedback, 10, 2200),
                (ContextSource::OutlineSlice, 10, 6000),
                (ContextSource::ReaderCompensation, 10, 500),
                (ContextSource::PreviousChapter, 9, 5000),
                (ContextSource::PromiseSlice, 8, 4000),
                (ContextSource::DecisionSlice, 8, 2000),
                (ContextSource::CanonSlice, 7, 4000),
                (ContextSource::AuthorStyle, 6, 2000),
                (ContextSource::RagExcerpt, 5, 4000),
                (ContextSource::NeighborText, 4, 3000),
            ],
            AgentTask::InlineRewrite => vec![
                (ContextSource::SelectedText, 10, 2000),
                (ContextSource::CursorPrefix, 9, 500),
                (ContextSource::CursorSuffix, 8, 500),
                (ContextSource::StoryImpactRadius, 8, 360),
                (ContextSource::ChapterMission, 8, 500),
                (ContextSource::NextBeat, 8, 360),
                (ContextSource::ProjectBrief, 8, 400),
                (ContextSource::BookState, 7, 260),
                (ContextSource::ArcSnapshot, 7, 240),
                (ContextSource::ResultFeedback, 7, 360),
                (ContextSource::CanonSlice, 7, 400),
                (ContextSource::DecisionSlice, 7, 300),
                (ContextSource::AuthorStyle, 6, 300),
                (ContextSource::ReaderCompensation, 7, 300),
            ],
            AgentTask::PlanningReview => vec![
                (ContextSource::ChapterMission, 11, 900),
                (ContextSource::ProjectBrief, 10, 800),
                (ContextSource::BookState, 10, 700),
                (ContextSource::ArcSnapshot, 9, 600),
                (ContextSource::VolumeSnapshot, 9, 520),
                (ContextSource::ResultFeedback, 10, 900),
                (ContextSource::ReaderCompensation, 10, 400),
                (ContextSource::NextBeat, 10, 700),
                (ContextSource::StoryImpactRadius, 9, 700),
                (ContextSource::PromiseSlice, 9, 900),
                (ContextSource::CanonSlice, 9, 900),
                (ContextSource::DecisionSlice, 8, 600),
                (ContextSource::OutlineSlice, 8, 900),
                (ContextSource::AuthorStyle, 7, 500),
                (ContextSource::SelectedText, 7, 700),
                (ContextSource::CursorPrefix, 6, 700),
                (ContextSource::RagExcerpt, 5, 800),
            ],
            AgentTask::ProposalEvaluation => vec![
                (ContextSource::CanonSlice, 10, 500),
                (ContextSource::PromiseSlice, 9, 300),
                (ContextSource::DecisionSlice, 9, 300),
                (ContextSource::StoryImpactRadius, 8, 260),
                (ContextSource::NextBeat, 8, 260),
                (ContextSource::ChapterMission, 8, 360),
                (ContextSource::ProjectBrief, 8, 300),
                (ContextSource::BookState, 7, 220),
                (ContextSource::ResultFeedback, 8, 260),
                (ContextSource::AuthorStyle, 8, 300),
                (ContextSource::ReaderCompensation, 4, 180),
            ],
            AgentTask::CanonMaintenance => vec![
                (ContextSource::CanonSlice, 10, 2000),
                (ContextSource::PromiseSlice, 9, 1000),
                (ContextSource::NextBeat, 9, 800),
                (ContextSource::StoryImpactRadius, 9, 700),
                (ContextSource::ResultFeedback, 9, 900),
                (ContextSource::DecisionSlice, 9, 800),
                (ContextSource::ChapterMission, 8, 800),
                (ContextSource::ProjectBrief, 8, 600),
                (ContextSource::BookState, 8, 520),
                (ContextSource::ArcSnapshot, 8, 420),
                (ContextSource::OutlineSlice, 8, 1000),
                (ContextSource::ReaderCompensation, 4, 180),
            ],
            AgentTask::ManualRequest => vec![
                (ContextSource::SelectedText, 10, 1200),
                (ContextSource::CursorPrefix, 9, 1400),
                (ContextSource::CursorSuffix, 8, 500),
                (ContextSource::StoryImpactRadius, 8, 500),
                (ContextSource::ChapterMission, 8, 700),
                (ContextSource::NextBeat, 8, 600),
                (ContextSource::ProjectBrief, 8, 600),
                (ContextSource::BookState, 7, 420),
                (ContextSource::ArcSnapshot, 7, 360),
                (ContextSource::ResultFeedback, 8, 700),
                (ContextSource::CanonSlice, 8, 800),
                (ContextSource::PromiseSlice, 7, 600),
                (ContextSource::DecisionSlice, 7, 500),
                (ContextSource::AuthorStyle, 6, 400),
                (ContextSource::RagExcerpt, 5, 400),
                (ContextSource::ReaderCompensation, 4, 180),
            ],
        }
    }

    pub fn required_source_budgets(&self) -> Vec<(ContextSource, usize)> {
        match self {
            AgentTask::GhostWriting => vec![
                (ContextSource::CursorPrefix, 240),
                (ContextSource::ChapterMission, 180),
                (ContextSource::NextBeat, 160),
                (ContextSource::ProjectBrief, 160),
                (ContextSource::BookState, 140),
                (ContextSource::ResultFeedback, 140),
                (ContextSource::StoryImpactRadius, 140),
                (ContextSource::CanonSlice, 180),
                (ContextSource::PromiseSlice, 140),
            ],
            AgentTask::ContinuityDiagnostic => vec![
                (ContextSource::CursorPrefix, 160),
                (ContextSource::CanonSlice, 240),
                (ContextSource::ChapterMission, 140),
                (ContextSource::NextBeat, 140),
                (ContextSource::ProjectBrief, 120),
                (ContextSource::BookState, 120),
                (ContextSource::ResultFeedback, 120),
                (ContextSource::StoryImpactRadius, 140),
            ],
            AgentTask::ChapterGeneration => vec![
                (ContextSource::ProjectBrief, 500),
                (ContextSource::BookState, 400),
                (ContextSource::ChapterMission, 700),
                (ContextSource::ArcSnapshot, 300),
                (ContextSource::VolumeSnapshot, 260),
                (ContextSource::NextBeat, 700),
                (ContextSource::ResultFeedback, 700),
                (ContextSource::StoryImpactRadius, 300),
                (ContextSource::OutlineSlice, 1_000),
                (ContextSource::PreviousChapter, 800),
                (ContextSource::PromiseSlice, 600),
                (ContextSource::CanonSlice, 600),
            ],
            AgentTask::InlineRewrite => vec![
                (ContextSource::SelectedText, 400),
                (ContextSource::CursorPrefix, 160),
                (ContextSource::ChapterMission, 160),
                (ContextSource::NextBeat, 140),
                (ContextSource::ProjectBrief, 120),
                (ContextSource::BookState, 100),
                (ContextSource::ResultFeedback, 120),
                (ContextSource::StoryImpactRadius, 120),
            ],
            AgentTask::PlanningReview => vec![
                (ContextSource::ChapterMission, 300),
                (ContextSource::ProjectBrief, 220),
                (ContextSource::BookState, 220),
                (ContextSource::ArcSnapshot, 180),
                (ContextSource::ResultFeedback, 220),
                (ContextSource::StoryImpactRadius, 220),
                (ContextSource::PromiseSlice, 240),
                (ContextSource::CanonSlice, 240),
            ],
            AgentTask::ProposalEvaluation => vec![
                (ContextSource::CanonSlice, 180),
                (ContextSource::DecisionSlice, 120),
                (ContextSource::NextBeat, 120),
                (ContextSource::ChapterMission, 120),
                (ContextSource::ProjectBrief, 120),
                (ContextSource::BookState, 100),
                (ContextSource::ResultFeedback, 120),
                (ContextSource::StoryImpactRadius, 120),
            ],
            AgentTask::CanonMaintenance => vec![
                (ContextSource::CanonSlice, 600),
                (ContextSource::PromiseSlice, 240),
                (ContextSource::NextBeat, 240),
                (ContextSource::ResultFeedback, 240),
                (ContextSource::StoryImpactRadius, 180),
                (ContextSource::ChapterMission, 240),
                (ContextSource::ProjectBrief, 180),
                (ContextSource::BookState, 160),
            ],
            AgentTask::ManualRequest => vec![
                (ContextSource::SelectedText, 300),
                (ContextSource::CursorPrefix, 300),
                (ContextSource::ChapterMission, 220),
                (ContextSource::NextBeat, 220),
                (ContextSource::ProjectBrief, 180),
                (ContextSource::BookState, 160),
                (ContextSource::ResultFeedback, 180),
                (ContextSource::StoryImpactRadius, 180),
                (ContextSource::CanonSlice, 220),
                (ContextSource::PromiseSlice, 180),
            ],
        }
    }
}

/// The assembled context for an agent action.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WritingContextPack {
    pub task: AgentTask,
    pub sources: Vec<ContextExcerpt>,
    pub total_chars: usize,
    pub budget_limit: usize,
    pub budget_report: ContextBudgetReport,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextBudgetReport {
    pub total_budget: usize,
    pub used: usize,
    pub wasted: usize,
    pub source_reports: Vec<SourceReport>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceReport {
    pub source: String,
    pub requested: usize,
    pub provided: usize,
    pub truncated: bool,
    pub reason: String,
    pub truncation_reason: Option<String>,
}

impl WritingContextPack {
    /// Human-readable explanation of which sources were included, truncated, or excluded and why.
    pub fn explain(&self) -> String {
        let mut lines = Vec::new();
        lines.push(format!(
            "ContextPack for {:?}: {} chars / {} budget ({} wasted)",
            self.task, self.total_chars, self.budget_limit, self.budget_report.wasted
        ));
        lines.push(format!("{} sources included:", self.sources.len()));
        for source in &self.sources {
            let trunc_note = if source.truncated { " [TRUNCATED]" } else { "" };
            lines.push(format!(
                "  - {:?} (priority {}, {} chars{})",
                source.source, source.priority, source.char_count, trunc_note
            ));
        }
        let included_sources: Vec<_> = self
            .sources
            .iter()
            .map(|s| format!("{:?}", s.source))
            .collect();
        let all_known = [
            "CursorPrefix",
            "ChapterMission",
            "NextBeat",
            "ResultFeedback",
            "ProjectBrief",
            "BookState",
            "ArcSnapshot",
            "VolumeSnapshot",
            "CanonSlice",
            "PromiseSlice",
            "DecisionSlice",
            "OutlineSlice",
            "CursorSuffix",
            "AuthorStyle",
            "RagExcerpt",
            "StoryImpactRadius",
            "ReaderCompensation",
        ];
        let excluded: Vec<_> = all_known
            .iter()
            .filter(|name| !included_sources.iter().any(|s| s == *name))
            .collect();
        if !excluded.is_empty() {
            lines.push(format!(
                "Excluded sources: {}",
                excluded
                    .iter()
                    .map(|s| s.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        for report in &self.budget_report.source_reports {
            if report.truncated {
                lines.push(format!(
                    "  {:?} truncated: requested {} chars, provided {} chars ({})",
                    report.source,
                    report.requested,
                    report.provided,
                    report
                        .truncation_reason
                        .as_deref()
                        .unwrap_or("budget exhausted")
                ));
            }
        }
        lines.join(
            "
",
        )
    }
}
