//! Task packet and context trace helpers for WriterAgentKernel.

use agent_harness_core::{RequiredContext, TaskBelief, TaskPacket, TaskScope};

use super::helpers::{feedback_contract_for_task, tool_policy_for_task};
use crate::writer_agent::context::{AgentTask, ContextSource, WritingContextPack};
use crate::writer_agent::memory::{
    ContextBudgetTrace, ContextSourceBudgetTrace, StoryContractQuality,
};
use crate::writer_agent::observation::WriterObservation;
use crate::writer_agent::story_impact::{
    story_impact_task_summary, StoryImpactBudgetReport, WriterStoryImpactRadius,
};

pub(crate) fn context_budget_trace(pack: &WritingContextPack) -> ContextBudgetTrace {
    ContextBudgetTrace {
        task: format!("{:?}", pack.task),
        used: pack.budget_report.used,
        total_budget: pack.budget_report.total_budget,
        wasted: pack.budget_report.wasted,
        source_reports: pack
            .budget_report
            .source_reports
            .iter()
            .map(|source| ContextSourceBudgetTrace {
                source: source.source.clone(),
                requested: source.requested,
                provided: source.provided,
                truncated: source.truncated,
                reason: source.reason.clone(),
                truncation_reason: source.truncation_reason.clone(),
            })
            .collect(),
    }
}

pub fn build_task_packet_for_observation(
    project_id: &str,
    session_id: &str,
    task: AgentTask,
    observation: &WriterObservation,
    context_pack: &WritingContextPack,
    objective: &str,
    success_criteria: Vec<String>,
) -> TaskPacket {
    let scope_ref = observation
        .chapter_title
        .clone()
        .or_else(|| {
            observation
                .cursor
                .as_ref()
                .map(|cursor| format!("{}..{}", cursor.from, cursor.to))
        })
        .unwrap_or_else(|| project_id.to_string());
    let scope = match task {
        AgentTask::GhostWriting => TaskScope::CursorWindow,
        AgentTask::InlineRewrite => TaskScope::Selection,
        AgentTask::ChapterGeneration => TaskScope::Chapter,
        AgentTask::ManualRequest => TaskScope::Chapter,
        AgentTask::PlanningReview => TaskScope::Chapter,
        AgentTask::ContinuityDiagnostic | AgentTask::CanonMaintenance => TaskScope::Scene,
        AgentTask::ProposalEvaluation => TaskScope::Custom,
    };
    let mut packet = TaskPacket::new(
        format!("{}:{}:{:?}", session_id, observation.id, task),
        objective,
        scope,
        observation.created_at,
    );
    packet.scope_ref = Some(scope_ref);
    packet.intent = Some(match task {
        AgentTask::GhostWriting | AgentTask::ChapterGeneration | AgentTask::InlineRewrite => {
            agent_harness_core::Intent::GenerateContent
        }
        AgentTask::PlanningReview
        | AgentTask::ContinuityDiagnostic
        | AgentTask::CanonMaintenance
        | AgentTask::ProposalEvaluation => agent_harness_core::Intent::AnalyzeText,
        AgentTask::ManualRequest => agent_harness_core::Intent::Chat,
    });
    packet.constraints = constraints_for_task(&task);
    packet.success_criteria = success_criteria;
    packet.beliefs = beliefs_from_context_pack(context_pack);
    packet.required_context = required_context_from_pack(context_pack);
    packet.tool_policy = tool_policy_for_task(&task);
    packet.feedback = feedback_contract_for_task(&task);
    packet
}

pub(crate) fn attach_story_impact_to_task_packet(
    packet: &mut TaskPacket,
    radius: &WriterStoryImpactRadius,
    budget: &StoryImpactBudgetReport,
) {
    let summary = story_impact_task_summary(radius, budget);
    if summary.trim().is_empty() {
        return;
    }

    packet.beliefs.push(
        TaskBelief::new("Story Impact Radius", summary.clone(), 0.82)
            .with_source("writer.story_impact_radius_built"),
    );
    packet.required_context.push(RequiredContext::new(
        "StoryImpactRadius",
        "Review impacted story facts, risk, and budget truncation before drafting or planning.",
        summary.chars().count().max(1),
        true,
    ));
}

pub(crate) fn attach_story_contract_quality_gate_to_task_packet(
    packet: &mut TaskPacket,
    task: &AgentTask,
    quality: StoryContractQuality,
    quality_gaps: &[String],
) {
    if !agent_task_requires_story_grounding(task) || quality > StoryContractQuality::Vague {
        return;
    }

    let gap_summary = if quality_gaps.is_empty() {
        "no detailed gaps recorded".to_string()
    } else {
        quality_gaps
            .iter()
            .take(3)
            .cloned()
            .collect::<Vec<_>>()
            .join("; ")
    };
    let statement = format!(
        "StoryContract quality is {:?}: this task may lack book-level grounding. Gaps: {}. Strengthen the Story Contract before trusting generation or diagnosis.",
        quality, gap_summary
    );

    packet.beliefs.push(
        TaskBelief::new("Story Contract Quality", statement.clone(), 0.9)
            .with_source("story_contract_quality_gate"),
    );
    packet.required_context.push(RequiredContext::new(
        "StoryContractQuality",
        "Warn when story-level grounding is missing or too vague for this task.",
        statement.chars().count().max(1),
        true,
    ));
}

pub(crate) fn agent_task_requires_story_grounding(task: &AgentTask) -> bool {
    matches!(
        task,
        AgentTask::ChapterGeneration
            | AgentTask::GhostWriting
            | AgentTask::InlineRewrite
            | AgentTask::ContinuityDiagnostic
            | AgentTask::PlanningReview
    )
}

fn constraints_for_task(task: &AgentTask) -> Vec<String> {
    let mut constraints = vec![
        "Preserve established canon unless the user explicitly approves a change.".to_string(),
        "Respect active chapter mission and story contract boundaries.".to_string(),
    ];
    match task {
        AgentTask::GhostWriting => {
            constraints.push("Keep proactive text short and easy to ignore.".to_string());
        }
        AgentTask::ManualRequest => {
            constraints
                .push("Answer the author directly before proposing broad rewrites.".to_string());
        }
        AgentTask::ChapterGeneration => {
            constraints
                .push("Generate chapter prose only; no analysis or markdown wrapper.".to_string());
        }
        AgentTask::InlineRewrite => {
            constraints
                .push("Limit edits to the selected range or cursor insertion point.".to_string());
        }
        AgentTask::PlanningReview => {
            constraints.push(
                "Plan and review only; do not draft manuscript text, propose typed write operations, or write project memory."
                    .to_string(),
            );
            constraints.push(
                "Separate evidence-backed risks from candidate next actions and author-confirmation questions."
                    .to_string(),
            );
        }
        AgentTask::ContinuityDiagnostic | AgentTask::CanonMaintenance => {
            constraints.push("Surface evidence before recommending canon changes.".to_string());
        }
        AgentTask::ProposalEvaluation => {
            constraints
                .push("Judge the proposal against evidence and feedback history.".to_string());
        }
    }
    constraints
}

fn beliefs_from_context_pack(context_pack: &WritingContextPack) -> Vec<TaskBelief> {
    let mut beliefs = Vec::new();
    for source in &context_pack.sources {
        if beliefs.len() >= 8 {
            break;
        }
        let subject = format!("{:?}", source.source);
        let statement = crate::writer_agent::kernel::snippet(&source.content, 180);
        if statement.trim().is_empty() {
            continue;
        }
        beliefs.push(
            TaskBelief::new(subject, statement, belief_confidence(&source.source))
                .with_source(format!("{:?}", source.source)),
        );
    }

    if beliefs.is_empty() {
        beliefs.push(TaskBelief::new(
            "editor_context",
            "Only the current editor observation is available for this task.",
            0.55,
        ));
    }
    beliefs
}

fn belief_confidence(source: &ContextSource) -> f32 {
    match source {
        ContextSource::SystemContract
        | ContextSource::ProjectBrief
        | ContextSource::BookState
        | ContextSource::ChapterMission
        | ContextSource::CanonSlice
        | ContextSource::PromiseSlice => 0.9,
        ContextSource::ResultFeedback | ContextSource::DecisionSlice | ContextSource::NextBeat => {
            0.8
        }
        ContextSource::CursorPrefix
        | ContextSource::CursorSuffix
        | ContextSource::SelectedText
        | ContextSource::PreviousChapter
        | ContextSource::NextChapter
        | ContextSource::NeighborText => 0.75,
        ContextSource::AuthorStyle
        | ContextSource::OutlineSlice
        | ContextSource::RagExcerpt
        | ContextSource::ArcSnapshot
        | ContextSource::VolumeSnapshot => 0.7,
        ContextSource::StoryImpactRadius => 0.82,
        ContextSource::ReaderCompensation => 0.75,
    }
}

fn required_context_from_pack(context_pack: &WritingContextPack) -> Vec<RequiredContext> {
    let mut contexts = context_pack
        .sources
        .iter()
        .take(12)
        .map(|source| {
            RequiredContext::new(
                format!("{:?}", source.source),
                context_source_purpose(&source.source),
                source.char_count.max(1),
                is_required_context_source(&context_pack.task, &source.source),
            )
        })
        .collect::<Vec<_>>();

    if !contexts.iter().any(|context| context.required) {
        if let Some(first) = contexts.first_mut() {
            first.required = true;
        } else {
            contexts.push(RequiredContext::new(
                "editor_observation",
                "Fallback sensory context for the current writing task.",
                1,
                true,
            ));
        }
    }
    contexts
}

fn context_source_purpose(source: &ContextSource) -> &'static str {
    match source {
        ContextSource::SystemContract | ContextSource::ProjectBrief => {
            "Keep the task inside the book-level contract."
        }
        ContextSource::BookState => {
            "Keep the task inside book-level long-term state and irreversible changes."
        }
        ContextSource::ChapterMission => "Preserve this chapter's active mission.",
        ContextSource::ArcSnapshot => {
            "Respect the active arc summary and mid-range story commitments."
        }
        ContextSource::VolumeSnapshot => {
            "Carry forward recent volume-level settled outcomes and inherited pressure."
        }
        ContextSource::NextBeat => "Carry forward the next intended story beat.",
        ContextSource::ResultFeedback => "Use the previous chapter result feedback loop.",
        ContextSource::AuthorStyle => "Preserve learned author style preferences.",
        ContextSource::CanonSlice => "Avoid contradictions against established canon.",
        ContextSource::PromiseSlice => "Track open promises and story debts.",
        ContextSource::DecisionSlice => "Respect recent creative decisions.",
        ContextSource::OutlineSlice => "Stay aligned with the outline.",
        ContextSource::RagExcerpt => "Ground the task in retrieved project memory.",
        ContextSource::CursorPrefix => "Read the local prose before the cursor.",
        ContextSource::CursorSuffix => "Avoid clashing with local prose after the cursor.",
        ContextSource::SelectedText => "Constrain edits to the selected text.",
        ContextSource::PreviousChapter => "Maintain continuity from previous chapters.",
        ContextSource::NextChapter => "Avoid blocking the next planned chapter.",
        ContextSource::NeighborText => "Maintain nearby prose flow.",
        ContextSource::StoryImpactRadius => {
            "Review impacted story facts, risk, and budget truncation before drafting or planning."
        }
        ContextSource::ReaderCompensation => {
            "Ground the task in reader emotional compensation context."
        }
    }
}

fn is_required_context_source(task: &AgentTask, source: &ContextSource) -> bool {
    task.required_source_budgets()
        .iter()
        .any(|(required, _)| required == source)
}

pub(crate) fn story_impact_context_budget(task: &AgentTask) -> usize {
    task.source_priorities()
        .iter()
        .find(|(source, _, _)| source == &ContextSource::StoryImpactRadius)
        .map(|(_, _, budget)| *budget)
        .unwrap_or(420)
}

pub(crate) fn story_impact_context_priority(task: &AgentTask) -> u8 {
    task.source_priorities()
        .iter()
        .find(|(source, _, _)| source == &ContextSource::StoryImpactRadius)
        .map(|(_, priority, _)| *priority)
        .unwrap_or(8)
}

pub(crate) fn trace_state_with_expiry(state: &str, expires_at: Option<u64>, now: u64) -> String {
    if state == "pending" && expires_at.is_some_and(|expiry| expiry <= now) {
        "expired".to_string()
    } else {
        state.to_string()
    }
}
