//! Story debt, review queue, and priority ordering helpers.
//! Extracted from kernel.rs.

use crate::writer_agent::kernel::{
    StoryDebtCategory, StoryDebtEntry, StoryDebtStatus, StoryReviewQueueEntry,
};
use crate::writer_agent::memory::PlotPromiseSummary;
use crate::writer_agent::operation::WriterOperation;
use crate::writer_agent::proposal::{
    AgentProposal, EvidenceRef, EvidenceSource, ProposalKind, ProposalPriority,
};

pub(crate) fn story_review_queue_entry(
    proposal: &AgentProposal,
    created_at: u64,
    status: crate::writer_agent::kernel::StoryReviewQueueStatus,
) -> StoryReviewQueueEntry {
    StoryReviewQueueEntry {
        id: format!("review_{}", proposal.id),
        proposal_id: proposal.id.clone(),
        category: proposal.kind.clone(),
        severity: review_severity_for_priority(&proposal.priority),
        title: review_title_for_proposal(proposal),
        message: proposal.preview.clone(),
        target: proposal.target.clone(),
        evidence: proposal.evidence.clone(),
        operations: proposal.operations.clone(),
        status,
        created_at,
        expires_at: proposal.expires_at,
    }
}

pub(crate) fn review_severity_for_priority(
    priority: &ProposalPriority,
) -> crate::writer_agent::kernel::StoryReviewSeverity {
    match priority {
        ProposalPriority::Urgent => crate::writer_agent::kernel::StoryReviewSeverity::Error,
        ProposalPriority::Normal => crate::writer_agent::kernel::StoryReviewSeverity::Warning,
        ProposalPriority::Ambient => crate::writer_agent::kernel::StoryReviewSeverity::Info,
    }
}

pub(crate) fn review_title_for_proposal(proposal: &AgentProposal) -> String {
    match proposal.kind {
        ProposalKind::ContinuityWarning => "Story truth conflict".to_string(),
        ProposalKind::StoryContract => "Story contract guard".to_string(),
        ProposalKind::ChapterMission => "Chapter mission guard".to_string(),
        ProposalKind::PlotPromise => {
            if proposal
                .operations
                .iter()
                .any(|operation| matches!(operation, WriterOperation::PromiseResolve { .. }))
            {
                "Plot promise payoff".to_string()
            } else {
                "Plot promise memory".to_string()
            }
        }
        ProposalKind::CanonUpdate => "Canon memory candidate".to_string(),
        ProposalKind::StyleNote => "Style review note".to_string(),
        ProposalKind::ChapterStructure => "Chapter structure note".to_string(),
        ProposalKind::Question => "Open story question".to_string(),
        ProposalKind::ParallelDraft => "Parallel draft".to_string(),
        ProposalKind::Ghost => "Draft continuation".to_string(),
    }
}

pub(crate) fn story_debt_from_review_entry(
    entry: &StoryReviewQueueEntry,
    active_chapter: &Option<String>,
) -> StoryDebtEntry {
    StoryDebtEntry {
        id: format!("debt_{}", entry.id),
        chapter_title: chapter_from_operations(&entry.operations)
            .or_else(|| active_chapter.clone()),
        category: story_debt_category_for_review(entry),
        severity: entry.severity.clone(),
        status: match entry.status {
            crate::writer_agent::kernel::StoryReviewQueueStatus::Snoozed => {
                StoryDebtStatus::Snoozed
            }
            crate::writer_agent::kernel::StoryReviewQueueStatus::Expired => StoryDebtStatus::Stale,
            _ => StoryDebtStatus::Open,
        },
        title: entry.title.clone(),
        message: entry.message.clone(),
        evidence: story_debt_review_evidence(entry),
        related_review_ids: vec![entry.id.clone()],
        operations: entry.operations.clone(),
        created_at: entry.created_at,
    }
}

pub(crate) fn story_debt_review_evidence(entry: &StoryReviewQueueEntry) -> Vec<EvidenceRef> {
    entry
        .evidence
        .iter()
        .map(|evidence| {
            if evidence.source == EvidenceSource::PromiseLedger {
                let mut enriched = evidence.clone();
                if let Some(last_seen) =
                    promise_last_seen_chapter_from_operations(&entry.operations)
                {
                    if !enriched.snippet.contains("last seen:") {
                        enriched
                            .snippet
                            .push_str(&format!(" | last seen: {}", last_seen));
                    }
                }
                enriched
            } else {
                evidence.clone()
            }
        })
        .collect()
}

pub(crate) fn promise_last_seen_chapter_from_operations(
    operations: &[WriterOperation],
) -> Option<String> {
    operations.iter().find_map(|operation| match operation {
        WriterOperation::PromiseResolve { chapter, .. }
        | WriterOperation::PromiseDefer { chapter, .. }
        | WriterOperation::PromiseAbandon { chapter, .. } => Some(chapter.clone()),
        _ => None,
    })
}

pub(crate) fn story_debt_from_open_promise(
    promise: &PlotPromiseSummary,
    active_chapter: &Option<String>,
) -> StoryDebtEntry {
    let current_number = active_chapter
        .as_deref()
        .and_then(chapter_number_from_title)
        .or_else(|| chapter_number_from_title(&promise.last_seen_chapter))
        .unwrap_or_default();
    let introduced_number =
        chapter_number_from_title(&promise.introduced_chapter).unwrap_or_default();
    let age = current_number.saturating_sub(introduced_number);
    let stale = age >= 8;
    let blocked = promise_is_blocked(promise);
    let status = promise_debt_status(promise, stale, blocked);
    StoryDebtEntry {
        id: format!("debt_promise_{}", promise.id),
        chapter_title: active_chapter.clone(),
        category: StoryDebtCategory::Promise,
        severity: promise_debt_severity(promise, stale, blocked),
        status,
        title: format!("Open promise: {}", promise.title),
        message: promise_debt_message(promise, age, stale, blocked),
        evidence: vec![EvidenceRef {
            source: EvidenceSource::PromiseLedger,
            reference: promise.title.clone(),
            snippet: promise_debt_evidence_snippet(promise),
        }],
        related_review_ids: Vec::new(),
        operations: story_debt_promise_operations(promise, active_chapter),
        created_at: 0,
    }
}

fn chapter_number_from_title(chapter: &str) -> Option<i64> {
    let digits = chapter
        .chars()
        .filter(|ch| ch.is_ascii_digit())
        .collect::<String>();
    if digits.is_empty() {
        None
    } else {
        digits.parse::<i64>().ok()
    }
}

pub(crate) fn story_debt_promise_operations(
    promise: &PlotPromiseSummary,
    active_chapter: &Option<String>,
) -> Vec<WriterOperation> {
    let chapter = active_chapter
        .clone()
        .unwrap_or_else(|| promise.expected_payoff.clone());
    vec![
        WriterOperation::PromiseResolve {
            promise_id: promise.id.to_string(),
            chapter: chapter.clone(),
        },
        WriterOperation::PromiseDefer {
            promise_id: promise.id.to_string(),
            chapter: chapter.clone(),
            expected_payoff: next_story_debt_payoff(&chapter),
        },
        WriterOperation::PromiseAbandon {
            promise_id: promise.id.to_string(),
            chapter,
            reason: format!(
                "Author decided '{}' no longer needs payoff in the current story shape.",
                promise.title
            ),
        },
    ]
}

pub(crate) fn next_story_debt_payoff(chapter: &str) -> String {
    let mut numbers = Vec::new();
    let mut current = String::new();
    for ch in chapter.chars() {
        if ch.is_ascii_digit() {
            current.push(ch);
        } else if !current.is_empty() {
            if let Ok(number) = current.parse::<i64>() {
                numbers.push(number);
            }
            current.clear();
        }
    }
    if !current.is_empty() {
        if let Ok(number) = current.parse::<i64>() {
            numbers.push(number);
        }
    }
    numbers
        .last()
        .map(|number| format!("Chapter-{}", number + 1))
        .unwrap_or_else(|| "later chapter".to_string())
}

pub(crate) fn story_debt_category_for_review(entry: &StoryReviewQueueEntry) -> StoryDebtCategory {
    match entry.category {
        ProposalKind::ContinuityWarning => {
            if entry.message.contains("时间线")
                || entry.evidence.iter().any(|evidence| {
                    evidence.snippet.contains("死亡") || evidence.snippet.contains("已死亡")
                })
            {
                StoryDebtCategory::TimelineRisk
            } else {
                StoryDebtCategory::CanonRisk
            }
        }
        ProposalKind::StoryContract => StoryDebtCategory::StoryContract,
        ProposalKind::ChapterMission => StoryDebtCategory::ChapterMission,
        ProposalKind::PlotPromise => StoryDebtCategory::Promise,
        ProposalKind::StyleNote => StoryDebtCategory::Pacing,
        ProposalKind::CanonUpdate => StoryDebtCategory::Memory,
        ProposalKind::Question => StoryDebtCategory::Question,
        ProposalKind::ChapterStructure => StoryDebtCategory::Pacing,
        ProposalKind::ParallelDraft | ProposalKind::Ghost => StoryDebtCategory::Question,
    }
}

pub(crate) fn story_debt_category_weight(category: &StoryDebtCategory) -> i32 {
    match category {
        StoryDebtCategory::StoryContract => 80,
        StoryDebtCategory::ChapterMission => 70,
        StoryDebtCategory::CanonRisk | StoryDebtCategory::TimelineRisk => 60,
        StoryDebtCategory::Promise => 50,
        StoryDebtCategory::Pacing => 40,
        StoryDebtCategory::Memory => 30,
        StoryDebtCategory::Question => 20,
    }
}

pub(crate) fn promise_debt_evidence_snippet(promise: &PlotPromiseSummary) -> String {
    let mut parts = vec![promise.description.clone()];
    if !promise.introduced_chapter.trim().is_empty() {
        parts.push(format!("introduced: {}", promise.introduced_chapter));
    }
    if !promise.last_seen_chapter.trim().is_empty() {
        parts.push(format!("last seen: {}", promise.last_seen_chapter));
    }
    if promise.promoted {
        parts.push("status: promoted".to_string());
    }
    if promise.core {
        parts.push("status: core".to_string());
    }
    if !promise.blocked_reason.trim().is_empty() {
        parts.push(format!("blocked: {}", promise.blocked_reason));
    }
    parts
        .into_iter()
        .filter(|part| !part.trim().is_empty())
        .collect::<Vec<_>>()
        .join(" | ")
}

pub(crate) fn chapter_from_operations(operations: &[WriterOperation]) -> Option<String> {
    operations.first().and_then(|operation| match operation {
        WriterOperation::TextInsert { chapter, .. }
        | WriterOperation::TextReplace { chapter, .. }
        | WriterOperation::TextAnnotate { chapter, .. }
        | WriterOperation::PromiseResolve { chapter, .. }
        | WriterOperation::PromiseDefer { chapter, .. }
        | WriterOperation::PromiseAbandon { chapter, .. } => Some(chapter.clone()),
        _ => None,
    })
}

pub(crate) fn story_debt_status_weight(status: &StoryDebtStatus) -> i32 {
    match status {
        StoryDebtStatus::Core => 5,
        StoryDebtStatus::Blocked => 4,
        StoryDebtStatus::Promoted => 3,
        StoryDebtStatus::Open => 2,
        StoryDebtStatus::Snoozed => 1,
        StoryDebtStatus::Stale => 0,
    }
}

fn promise_is_blocked(promise: &PlotPromiseSummary) -> bool {
    !promise.blocked_reason.trim().is_empty()
}

fn promise_debt_status(
    promise: &PlotPromiseSummary,
    stale: bool,
    blocked: bool,
) -> StoryDebtStatus {
    if promise.core {
        StoryDebtStatus::Core
    } else if blocked {
        StoryDebtStatus::Blocked
    } else if promise.promoted {
        StoryDebtStatus::Promoted
    } else if stale {
        StoryDebtStatus::Stale
    } else {
        StoryDebtStatus::Open
    }
}

fn promise_debt_severity(
    promise: &PlotPromiseSummary,
    stale: bool,
    blocked: bool,
) -> crate::writer_agent::kernel::StoryReviewSeverity {
    if promise.core || blocked || stale || promise.priority >= 5 {
        crate::writer_agent::kernel::StoryReviewSeverity::Warning
    } else {
        crate::writer_agent::kernel::StoryReviewSeverity::Info
    }
}

fn promise_debt_message(
    promise: &PlotPromiseSummary,
    age: i64,
    stale: bool,
    blocked: bool,
) -> String {
    let mut parts = vec![promise.description.clone()];
    if !promise.expected_payoff.trim().is_empty() {
        parts.push(format!("Expected payoff: {}", promise.expected_payoff));
    }
    if promise.core {
        parts.push("This is marked as a core hook and cannot keep drifting.".to_string());
    } else if promise.promoted {
        parts.push("This hook has been promoted and should enter near-term planning.".to_string());
    }
    if blocked {
        parts.push(format!("Blocked by: {}", promise.blocked_reason));
    }
    if stale {
        parts.push(format!("Stale for {} chapters.", age));
    }
    parts
        .into_iter()
        .filter(|part| !part.trim().is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

pub(crate) fn queue_status_weight(
    status: &crate::writer_agent::kernel::StoryReviewQueueStatus,
) -> i32 {
    match status {
        crate::writer_agent::kernel::StoryReviewQueueStatus::Pending => 4,
        crate::writer_agent::kernel::StoryReviewQueueStatus::Snoozed => 3,
        crate::writer_agent::kernel::StoryReviewQueueStatus::Expired => 2,
        crate::writer_agent::kernel::StoryReviewQueueStatus::Accepted => 1,
        crate::writer_agent::kernel::StoryReviewQueueStatus::Ignored => 0,
    }
}

pub(crate) fn queue_severity_weight(
    severity: &crate::writer_agent::kernel::StoryReviewSeverity,
) -> i32 {
    match severity {
        crate::writer_agent::kernel::StoryReviewSeverity::Error => 2,
        crate::writer_agent::kernel::StoryReviewSeverity::Warning => 1,
        crate::writer_agent::kernel::StoryReviewSeverity::Info => 0,
    }
}
