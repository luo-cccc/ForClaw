//! WriterAgentKernel — persistent project agent that owns observations,
//! proposals, memory, canon, and feedback.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use agent_harness_core::agent_loop::EventCallback;
use agent_harness_core::provider::Provider;
use agent_harness_core::{
    default_writing_tool_registry, AgentLoop, AgentLoopConfig, TaskPacket, ToolHandler,
};
use agent_harness_core::{PermissionMode, PermissionPolicy};

use super::canon::CanonEngine;
use super::context::{
    append_context_source_with_budget, assemble_observation_context_with_default_budget, AgentTask,
    ContextSource, WritingContextPack,
};
use super::diagnostics::{
    DiagnosticCategory, DiagnosticResult, DiagnosticSeverity, DiagnosticsEngine,
};
use super::feedback::{FeedbackAction, ProposalFeedback};
use super::intent::{AgentBehavior, IntentEngine};
use super::memory::{
    ArcSnapshotSummary, BookStateSummary, ChapterResultSummary, ContextBudgetTrace,
    ContextRecallSummary, ManualAgentTurnSummary, StoryContractQuality, VolumeSnapshotSummary,
    VolumeSummary, WriterMemory,
};
use super::observation::WriterObservation;
use super::operation::{OperationResult, WriterOperation};
use super::post_write_diagnostics::WriterPostWriteDiagnosticReport;
use super::proposal::{AgentProposal, EvidenceRef, EvidenceSource, ProposalKind, ProposalPriority};
use super::run_events::{WriterRunEvent, WriterRunEventStore};
use super::{memory, observation, operation, trajectory};

pub use super::inspector::{
    WriterInspectorTimeline, WriterTimelineAudience, WriterTimelineEvent, WriterTimelineEventKind,
};
pub use super::metacognition::{
    WriterMetacognitiveAction, WriterMetacognitiveRiskLevel, WriterMetacognitiveSnapshot,
};
pub(crate) use chapters::{
    calibrated_mission_status, chapter_mission_result_proposals, chapter_result_from_observation,
    cue_hit_score, derive_next_beat, mission_result_haystack, proposal_id, proposal_trace_summary,
};
pub(crate) use ghost::{
    context_pack_evidence, draft_continuation, ghost_alternatives, ghost_consume_style_preferences,
    sanitize_continuation,
};
pub use helpers::tool_filter_for_task;
pub(crate) use helpers::{
    approval_sources, normalize_chapter_mission_status, operation_affected_scope,
    operation_has_kernel_durable_save, operation_is_write_capable, operation_kind_label,
    operation_requires_durable_save, save_result_is_success,
};
pub use helpers::{ghost_confidence, ghost_quality_risks, task_requires_story_grounding};
pub(crate) use memory_candidates::{
    canon_attribute_merge_candidate_proposal, canon_candidate_proposal,
    canon_conflict_candidate_proposal, extract_new_canon_entities,
    llm_memory_candidates_from_value, memory_candidates_from_observation,
    promise_candidate_proposal, sentence_snippet, split_sentences, CandidateSource,
};
pub use memory_candidates::{
    extract_plot_promises, style_preference_memory_key, style_preference_taxonomy_slot,
    validate_canon_candidate, validate_canon_candidate_with_memory, validate_promise_candidate,
    validate_promise_candidate_with_dedup, validate_style_preference,
    validate_style_preference_with_memory, MemoryCandidateQuality,
};
pub(crate) use memory_feedback::{
    memory_operation_slot, proposal_slot_key, record_memory_audit_event,
    record_memory_candidate_feedback, suppression_slot_key, MemoryCandidate,
    MemoryExtractionFeedback,
};
pub(crate) use metrics::product_metrics_from_trace;
pub(crate) use metrics::product_metrics_trend_from_run_events;
pub use metrics::{
    WriterProductMetricSessionTrend, WriterProductMetrics, WriterProductMetricsTrend,
};
pub(crate) use ops::execute_writer_operation;
pub(crate) use prompts::{
    objective_for_run_task, render_context_pack_for_prompt, render_run_system_prompt,
    source_refs_from_context_pack, success_criteria_for_run_task, tool_filter_for_run_request,
};
pub(crate) use proposals_ext::{priority_weight, proposal_expired, should_replace_proposal};
pub(crate) use review::{
    queue_severity_weight, queue_status_weight, story_debt_category_weight,
    story_debt_from_open_promise, story_debt_from_review_entry, story_debt_status_weight,
    story_review_queue_entry,
};
pub use run_loop_ext::{
    WriterAgentApprovalMode, WriterAgentContextPackSummary, WriterAgentFrontendState,
    WriterAgentPreparedRun, WriterAgentRunRequest, WriterAgentRunResult, WriterAgentStreamMode,
    WriterAgentTask,
};
pub use task_packet::build_task_packet_for_observation;
pub(crate) use task_packet::{
    attach_story_contract_quality_gate_to_task_packet, attach_story_impact_to_task_packet,
    context_budget_trace, trace_state_with_expiry,
};
pub(crate) use task_packet::{story_impact_context_budget, story_impact_context_priority};

mod chapters;
mod context_pack;
mod feedback;
mod ghost;
mod helpers;
mod memory_candidates;
mod memory_feedback;
mod metrics;
mod observations;
mod operations;
mod ops;
mod prompts;
mod proposal_creation;
mod proposals;
mod proposals_ext;
mod review;
mod run_loop;
mod run_loop_ext;
mod run_loop_spine;
mod snapshots;
mod task_packet;
mod trace_recording;
pub use snapshots::ReaderCompensationReviewChain;
pub use trace_recording::{ModelStartedEventContext, SaveCompletedEventContext};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WriterAgentStatus {
    pub project_id: String,
    pub session_id: String,
    pub active_chapter: Option<String>,
    pub observation_count: u64,
    pub proposal_count: u64,
    pub open_promise_count: usize,
    pub pending_proposals: usize,
    pub total_feedback_events: u64,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WriterAgentLedgerSnapshot {
    pub story_contract: Option<super::memory::StoryContractSummary>,
    pub book_state: Option<BookStateSummary>,
    pub active_volume: Option<VolumeSummary>,
    pub arc_snapshots: Vec<ArcSnapshotSummary>,
    pub volume_snapshots: Vec<VolumeSnapshotSummary>,
    pub active_chapter_mission: Option<super::memory::ChapterMissionSummary>,
    pub chapter_missions: Vec<super::memory::ChapterMissionSummary>,
    pub recent_chapter_results: Vec<super::memory::ChapterResultSummary>,
    pub next_beat: Option<super::memory::NextBeatSummary>,
    pub canon_entities: Vec<super::memory::CanonEntitySummary>,
    pub canon_rules: Vec<super::memory::CanonRuleSummary>,
    pub open_promises: Vec<super::memory::PlotPromiseSummary>,
    pub recent_decisions: Vec<super::memory::CreativeDecisionSummary>,
    pub memory_audit: Vec<super::memory::MemoryAuditSummary>,
    pub memory_reliability: Vec<WriterMemoryReliabilitySummary>,
    pub context_recalls: Vec<ContextRecallSummary>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WriterMemoryReliabilitySummary {
    pub slot: String,
    pub category: String,
    pub status: String,
    pub reliability: f64,
    pub reinforcement_count: u64,
    pub correction_count: u64,
    pub net_confidence_delta: f64,
    pub last_action: String,
    pub last_source_error: Option<String>,
    pub last_reason: Option<String>,
    pub last_proposal_id: String,
    pub updated_at: u64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoryReviewQueueEntry {
    pub id: String,
    pub proposal_id: String,
    pub category: ProposalKind,
    pub severity: StoryReviewSeverity,
    pub title: String,
    pub message: String,
    pub target: Option<super::observation::TextRange>,
    pub evidence: Vec<EvidenceRef>,
    pub operations: Vec<WriterOperation>,
    pub status: StoryReviewQueueStatus,
    pub created_at: u64,
    pub expires_at: Option<u64>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum StoryReviewSeverity {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum StoryReviewQueueStatus {
    Pending,
    Accepted,
    Ignored,
    Snoozed,
    Expired,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StoryDebtSnapshot {
    pub chapter_title: Option<String>,
    pub total: usize,
    pub open_count: usize,
    pub contract_count: usize,
    pub mission_count: usize,
    pub canon_risk_count: usize,
    pub promise_count: usize,
    pub pacing_count: usize,
    pub entries: Vec<StoryDebtEntry>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TodayFiveSummary {
    pub chapter_title: Option<String>,
    pub items: Vec<TodayFiveItem>,
    pub is_onboarding: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TodayFiveItem {
    pub slot: String,
    pub label: String,
    pub value: String,
    pub detail: String,
    pub tone: String,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorySnapshot {
    pub character_count: usize,
    pub protagonist_name: Option<String>,
    pub open_promise_count: usize,
    pub total_chapters: usize,
    pub latest_reveal: String,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSummary {
    pub chapters_written: usize,
    pub total_words: usize,
    pub promises_advanced: usize,
    pub new_characters: usize,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StoryDebtEntry {
    pub id: String,
    pub chapter_title: Option<String>,
    pub category: StoryDebtCategory,
    pub severity: StoryReviewSeverity,
    pub status: StoryDebtStatus,
    pub title: String,
    pub message: String,
    pub evidence: Vec<EvidenceRef>,
    pub related_review_ids: Vec<String>,
    pub operations: Vec<WriterOperation>,
    pub created_at: u64,
}

#[derive(Debug, Clone, serde::Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum StoryDebtCategory {
    StoryContract,
    ChapterMission,
    CanonRisk,
    TimelineRisk,
    Promise,
    Pacing,
    Memory,
    Question,
}

#[derive(Debug, Clone, serde::Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum StoryDebtStatus {
    Open,
    Snoozed,
    Stale,
    Blocked,
    Promoted,
    Core,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WriterAgentTraceSnapshot {
    pub recent_observations: Vec<WriterObservationTrace>,
    pub task_packets: Vec<WriterTaskPacketTrace>,
    pub recent_proposals: Vec<WriterProposalTrace>,
    pub recent_feedback: Vec<WriterFeedbackTrace>,
    pub operation_lifecycle: Vec<WriterOperationLifecycleTrace>,
    pub run_events: Vec<WriterRunEvent>,
    pub post_write_diagnostics: Vec<WriterPostWriteDiagnosticReport>,
    pub context_source_trends: Vec<WriterContextSourceTrend>,
    pub context_recalls: Vec<ContextRecallSummary>,
    pub product_metrics: WriterProductMetrics,
    pub product_metrics_trend: WriterProductMetricsTrend,
    pub metacognitive_snapshot: WriterMetacognitiveSnapshot,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WriterTaskPacketTrace {
    pub id: String,
    pub observation_id: String,
    pub task: String,
    pub objective: String,
    pub scope: String,
    pub intent: Option<String>,
    pub required_context_count: usize,
    pub belief_count: usize,
    pub success_criteria_count: usize,
    pub max_side_effect_level: String,
    pub feedback_checkpoint_count: usize,
    pub foundation_complete: bool,
    pub packet: TaskPacket,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WriterContextPackBuiltRunEvent {
    pub task_id: String,
    pub task: String,
    pub source_count: usize,
    pub total_chars: usize,
    pub budget_limit: usize,
    pub wasted: usize,
    pub truncated_source_count: usize,
    pub source_reports: Vec<WriterContextPackBuiltSourceReport>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WriterContextPackBuiltSourceReport {
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requested: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original_chars: Option<usize>,
    pub provided: usize,
    pub truncated: bool,
    pub required: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub truncation_reason: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WriterObservationTrace {
    pub id: String,
    pub created_at: u64,
    pub reason: String,
    pub chapter_title: Option<String>,
    pub paragraph_snippet: String,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WriterProposalTrace {
    pub id: String,
    pub observation_id: String,
    pub kind: String,
    pub priority: String,
    pub state: String,
    pub confidence: f64,
    pub preview_snippet: String,
    pub evidence: Vec<EvidenceRef>,
    pub context_budget: Option<ContextBudgetTrace>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WriterFeedbackTrace {
    pub proposal_id: String,
    pub action: String,
    pub reason: Option<String>,
    pub created_at: u64,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WriterContextSourceTrend {
    pub source: String,
    pub appearances: usize,
    pub provided_count: usize,
    pub truncated_count: usize,
    pub dropped_count: usize,
    pub total_requested: usize,
    pub total_provided: usize,
    pub average_provided: f64,
    pub last_reason: Option<String>,
    pub last_truncation_reason: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum WriterOperationLifecycleState {
    Proposed,
    Approved,
    Applied,
    DurablySaved,
    FeedbackRecorded,
    Rejected,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WriterOperationLifecycleTrace {
    pub proposal_id: Option<String>,
    pub operation_kind: String,
    pub source_task: Option<String>,
    pub approval_source: Option<String>,
    pub affected_scope: Option<String>,
    pub state: WriterOperationLifecycleState,
    pub save_result: Option<String>,
    pub feedback_result: Option<String>,
    pub created_at: u64,
}

pub struct WriterAgentKernel {
    pub project_id: String,
    pub session_id: String,
    pub memory: WriterMemory,
    pub canon: CanonEngine,
    diagnostics: DiagnosticsEngine,
    intent: IntentEngine,
    observations: Vec<WriterObservation>,
    proposals: Vec<AgentProposal>,
    proposal_context_budgets: HashMap<String, ContextBudgetTrace>,
    task_packets: Vec<WriterTaskPacketTrace>,
    feedback_events: Vec<ProposalFeedback>,
    operation_lifecycle: Vec<WriterOperationLifecycleTrace>,
    run_events: WriterRunEventStore,
    superseded_proposals: HashSet<String>,
    suppressed_slots: Vec<SuppressedProposalSlot>,
    ignored_ghost_slots: Vec<IgnoredGhostSlot>,
    observation_counter: u64,
    proposal_counter: u64,
    pub active_chapter: Option<String>,
    pub last_spine: Option<crate::writer_agent::context::ContextSpine>,
    pub focus: crate::writer_agent::context::FocusState,
}

struct SuppressedProposalSlot {
    slot: String,
    until: u64,
}

struct IgnoredGhostSlot {
    slot: String,
    count: u8,
    last_seen: u64,
}

impl WriterAgentKernel {
    pub fn new(project_id: &str, memory: WriterMemory) -> Self {
        Self {
            project_id: project_id.into(),
            session_id: format!(
                "session-{}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_millis()
            ),
            memory,
            canon: CanonEngine::new(),
            diagnostics: DiagnosticsEngine::new(),
            intent: IntentEngine::new(),
            observations: Vec::new(),
            proposals: Vec::new(),
            proposal_context_budgets: HashMap::new(),
            task_packets: Vec::new(),
            feedback_events: Vec::new(),
            operation_lifecycle: Vec::new(),
            run_events: WriterRunEventStore::default(),
            superseded_proposals: HashSet::new(),
            suppressed_slots: Vec::new(),
            ignored_ghost_slots: Vec::new(),
            observation_counter: 0,
            proposal_counter: 0,
            active_chapter: None,
            last_spine: None,
            focus: crate::writer_agent::context::FocusState::default(),
        }
    }
}

pub(crate) fn snippet(text: &str, limit: usize) -> String {
    text.chars().take(limit).collect()
}

fn diagnostic_to_proposal(
    diagnostic: DiagnosticResult,
    observation: &WriterObservation,
    observation_id: &str,
    proposal_id: &str,
) -> AgentProposal {
    let priority = match diagnostic.severity {
        DiagnosticSeverity::Error => ProposalPriority::Urgent,
        DiagnosticSeverity::Warning => ProposalPriority::Normal,
        DiagnosticSeverity::Info => ProposalPriority::Ambient,
    };
    let kind = match diagnostic.category {
        DiagnosticCategory::UnresolvedPromise => ProposalKind::PlotPromise,
        DiagnosticCategory::CanonConflict => ProposalKind::ContinuityWarning,
        DiagnosticCategory::StoryContractViolation => ProposalKind::StoryContract,
        DiagnosticCategory::ChapterMissionViolation => ProposalKind::ChapterMission,
        DiagnosticCategory::PacingNote => ProposalKind::StyleNote,
        DiagnosticCategory::CharacterVoiceInconsistency => ProposalKind::StyleNote,
        DiagnosticCategory::TimelineIssue => ProposalKind::ContinuityWarning,
        DiagnosticCategory::PayoffGap => ProposalKind::PlotPromise,
    };
    let evidence = diagnostic
        .evidence
        .iter()
        .map(|item| EvidenceRef {
            source: match item.source.as_str() {
                "canon" => EvidenceSource::Canon,
                "promise" => EvidenceSource::PromiseLedger,
                "story_contract" => EvidenceSource::StoryContract,
                "chapter_mission" => EvidenceSource::ChapterMission,
                "outline" => EvidenceSource::Outline,
                "style" => EvidenceSource::StyleLedger,
                _ => EvidenceSource::ChapterText,
            },
            reference: item.reference.clone(),
            snippet: item.snippet.clone(),
        })
        .collect::<Vec<_>>();
    let operations = diagnostic
        .operations
        .into_iter()
        .map(|operation| operation.with_observation_revision(observation))
        .collect();
    let mut risks = Vec::new();
    if let Some(fix) = &diagnostic.fix_suggestion {
        risks.push(fix.clone());
    }

    AgentProposal {
        id: proposal_id.to_string(),
        observation_id: observation_id.to_string(),
        kind,
        priority,
        target: Some(super::observation::TextRange {
            from: diagnostic.from,
            to: diagnostic.to,
        }),
        preview: diagnostic.message,
        operations,
        rationale: format!(
            "Ambient diagnostic from {}.",
            observation
                .chapter_title
                .as_deref()
                .unwrap_or("current chapter")
        ),
        evidence,
        risks,
        alternatives: vec![],
        confidence: 0.72,
        expires_at: Some(observation.created_at + 120_000),
    }
}

trait OperationObservationRevision {
    fn with_observation_revision(self, observation: &WriterObservation) -> Self;
}

impl OperationObservationRevision for WriterOperation {
    fn with_observation_revision(self, observation: &WriterObservation) -> Self {
        let revision = observation
            .chapter_revision
            .clone()
            .unwrap_or_else(|| "missing".to_string());
        match self {
            WriterOperation::TextInsert {
                chapter,
                at,
                text,
                revision: _,
            } => WriterOperation::TextInsert {
                chapter,
                at,
                text,
                revision,
            },
            WriterOperation::TextReplace {
                chapter,
                from,
                to,
                text,
                revision: _,
            } => WriterOperation::TextReplace {
                chapter,
                from,
                to,
                text,
                revision,
            },
            other => other,
        }
    }
}

#[cfg(test)]
fn proposal_feedback(
    proposal_id: String,
    action: FeedbackAction,
    reason: Option<String>,
) -> ProposalFeedback {
    ProposalFeedback {
        proposal_id,
        action,
        final_text: None,
        reason,
        created_at: now_ms(),
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests;
