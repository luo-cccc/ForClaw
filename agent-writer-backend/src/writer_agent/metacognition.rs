//! Metacognitive run-health signals for Writer Agent.
//!
//! This module turns existing trace evidence into an explicit recommendation:
//! continue, continue with caution, ask the author, switch to read-only review,
//! run diagnostics, or block writes until confirmation.

use serde::{Deserialize, Serialize};

use super::kernel::{WriterAgentTask, WriterAgentTraceSnapshot};
use super::operation::WriterOperation;
use super::run_events::WriterRunEvent;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum WriterMetacognitiveRiskLevel {
    Low,
    Medium,
    High,
    Blocked,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WriterMetacognitiveAction {
    Proceed,
    ProceedWithWarning,
    AskClarifyingQuestion,
    SwitchToPlanningReview,
    RunContinuityDiagnostic,
    BlockWriteUntilAuthorConfirms,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WriterMetacognitiveSnapshot {
    pub risk_level: WriterMetacognitiveRiskLevel,
    pub recommended_action: WriterMetacognitiveAction,
    pub confidence: f64,
    pub summary: String,
    pub reasons: Vec<String>,
    pub remediation: Vec<String>,
    pub context_coverage_rate: f64,
    pub context_truncated_source_count: u64,
    pub context_dropped_source_count: u64,
    pub recent_failure_count: u64,
    pub post_write_error_count: u64,
    pub low_confidence_proposal_count: u64,
    pub ignored_repeated_suggestion_rate: f64,
}

impl Default for WriterMetacognitiveSnapshot {
    fn default() -> Self {
        Self {
            risk_level: WriterMetacognitiveRiskLevel::Low,
            recommended_action: WriterMetacognitiveAction::Proceed,
            confidence: 0.72,
            summary: "Run health is clear enough to continue.".to_string(),
            reasons: Vec::new(),
            remediation: Vec::new(),
            context_coverage_rate: 1.0,
            context_truncated_source_count: 0,
            context_dropped_source_count: 0,
            recent_failure_count: 0,
            post_write_error_count: 0,
            low_confidence_proposal_count: 0,
            ignored_repeated_suggestion_rate: 0.0,
        }
    }
}

pub fn metacognitive_snapshot_from_trace(
    snapshot: &WriterAgentTraceSnapshot,
) -> WriterMetacognitiveSnapshot {
    let mut reasons = Vec::new();
    let mut remediation = Vec::new();
    let mut risk_level = WriterMetacognitiveRiskLevel::Low;
    let mut recommended_action = WriterMetacognitiveAction::Proceed;

    let has_context_trend = snapshot
        .product_metrics_trend
        .recent_sessions
        .iter()
        .any(|session| session.context_pack_count > 0);
    let context_coverage_rate = if has_context_trend {
        snapshot
            .product_metrics_trend
            .recent_context_coverage_rate
            .max(snapshot.product_metrics_trend.overall_context_coverage_rate)
    } else {
        1.0
    };
    let context_truncated_source_count = if has_context_trend {
        snapshot
            .product_metrics_trend
            .recent_sessions
            .iter()
            .map(|session| session.context_truncated_source_count)
            .sum::<u64>()
    } else {
        0
    };
    let context_dropped_source_count = if has_context_trend {
        snapshot
            .product_metrics_trend
            .recent_sessions
            .iter()
            .map(|session| session.context_dropped_source_count)
            .sum::<u64>()
    } else {
        0
    };
    let recent_failure_count = snapshot
        .run_events
        .iter()
        .filter(|event| event.event_type == "writer.error")
        .count() as u64;
    let post_write_error_count = snapshot
        .post_write_diagnostics
        .iter()
        .map(|report| report.error_count as u64)
        .sum::<u64>();
    let low_confidence_proposal_count = snapshot
        .recent_proposals
        .iter()
        .filter(|proposal| proposal.confidence < 0.55)
        .count() as u64;
    let ignored_repeated_suggestion_rate =
        snapshot.product_metrics.ignored_repeated_suggestion_rate;

    if context_dropped_source_count > 0 || context_coverage_rate < 0.55 {
        raise(
            &mut risk_level,
            &mut recommended_action,
            WriterMetacognitiveRiskLevel::High,
            WriterMetacognitiveAction::SwitchToPlanningReview,
        );
        reasons.push(format!(
            "Context coverage is weak ({:.0}%) with {} dropped sources.",
            context_coverage_rate * 100.0,
            context_dropped_source_count
        ));
        remediation.push(
            "Switch to Planning Review and rebuild the context pack before drafting.".to_string(),
        );
    } else if context_truncated_source_count > 0 || context_coverage_rate < 0.75 {
        raise(
            &mut risk_level,
            &mut recommended_action,
            WriterMetacognitiveRiskLevel::Medium,
            WriterMetacognitiveAction::ProceedWithWarning,
        );
        reasons.push(format!(
            "Context pressure is visible ({:.0}% coverage, {} truncated sources).",
            context_coverage_rate * 100.0,
            context_truncated_source_count
        ));
        remediation.push(
            "Keep suggestions narrow and inspect context pressure if quality drops.".to_string(),
        );
    }

    if post_write_error_count > 0 {
        raise(
            &mut risk_level,
            &mut recommended_action,
            WriterMetacognitiveRiskLevel::Blocked,
            WriterMetacognitiveAction::RunContinuityDiagnostic,
        );
        reasons.push(format!(
            "Post-write diagnostics found {} blocking errors.",
            post_write_error_count
        ));
        remediation.push(
            "Run Continuity Diagnostic before accepting another manuscript write.".to_string(),
        );
    }

    if recent_failure_count > 0 {
        let latest = latest_failure(snapshot.run_events.as_slice());
        raise(
            &mut risk_level,
            &mut recommended_action,
            WriterMetacognitiveRiskLevel::High,
            WriterMetacognitiveAction::AskClarifyingQuestion,
        );
        reasons.push(format!(
            "Recent run failures need review{}.",
            latest
                .as_deref()
                .map(|code| format!(" ({code})"))
                .unwrap_or_default()
        ));
        remediation.push(
            "Ask a clarifying question or inspect the latest failure bundle before retrying."
                .to_string(),
        );
    }

    if low_confidence_proposal_count > 0 {
        raise(
            &mut risk_level,
            &mut recommended_action,
            WriterMetacognitiveRiskLevel::Medium,
            WriterMetacognitiveAction::AskClarifyingQuestion,
        );
        reasons.push(format!(
            "{} recent proposals are below the confidence floor.",
            low_confidence_proposal_count
        ));
        remediation.push(
            "Ask the author for scope or evidence before broadening the proposed edit.".to_string(),
        );
    }

    if ignored_repeated_suggestion_rate >= 0.5 && snapshot.product_metrics.feedback_count >= 2 {
        raise(
            &mut risk_level,
            &mut recommended_action,
            WriterMetacognitiveRiskLevel::Medium,
            WriterMetacognitiveAction::ProceedWithWarning,
        );
        reasons.push(format!(
            "Author ignored or rejected repeated suggestions at {:.0}%.",
            ignored_repeated_suggestion_rate * 100.0
        ));
        remediation
            .push("Reduce proactive suggestions and wait for stronger evidence.".to_string());
    }

    if snapshot.product_metrics.durable_save_success_rate < 1.0
        && snapshot.product_metrics.feedback_count > 0
    {
        raise(
            &mut risk_level,
            &mut recommended_action,
            WriterMetacognitiveRiskLevel::Blocked,
            WriterMetacognitiveAction::BlockWriteUntilAuthorConfirms,
        );
        reasons.push("Durable save success is below the safe threshold.".to_string());
        remediation
            .push("Block further write actions until the author confirms save state.".to_string());
    }

    if reasons.is_empty() {
        remediation.push("Continue normal writer-agent flow.".to_string());
    }

    let confidence = confidence_for(risk_level, reasons.len());
    let summary = summary_for(risk_level, recommended_action, &reasons);

    WriterMetacognitiveSnapshot {
        risk_level,
        recommended_action,
        confidence,
        summary,
        reasons,
        remediation,
        context_coverage_rate,
        context_truncated_source_count,
        context_dropped_source_count,
        recent_failure_count,
        post_write_error_count,
        low_confidence_proposal_count,
        ignored_repeated_suggestion_rate,
    }
}

pub fn metacognitive_write_gate_reason(snapshot: &WriterMetacognitiveSnapshot) -> Option<String> {
    if !metacognitive_snapshot_blocks_writes(snapshot) {
        return None;
    }
    Some(format!(
        "Metacognitive gate blocked this write-sensitive action: risk={:?}, action={:?}, confidence={:.0}%. {}",
        snapshot.risk_level,
        snapshot.recommended_action,
        snapshot.confidence * 100.0,
        snapshot.summary
    ))
}

pub fn metacognitive_task_is_write_sensitive(task: &WriterAgentTask) -> bool {
    matches!(
        task,
        WriterAgentTask::GhostWriting
            | WriterAgentTask::InlineRewrite
            | WriterAgentTask::ChapterGeneration
    )
}

pub fn metacognitive_operation_is_write_sensitive(operation: &WriterOperation) -> bool {
    matches!(
        operation,
        WriterOperation::TextInsert { .. }
            | WriterOperation::TextReplace { .. }
            | WriterOperation::OutlineUpdate { .. }
    )
}

fn metacognitive_snapshot_blocks_writes(snapshot: &WriterMetacognitiveSnapshot) -> bool {
    snapshot.recent_failure_count > 0
        || snapshot.post_write_error_count > 0
        || matches!(
            snapshot.recommended_action,
            WriterMetacognitiveAction::RunContinuityDiagnostic
                | WriterMetacognitiveAction::BlockWriteUntilAuthorConfirms
        )
}

fn raise(
    risk_level: &mut WriterMetacognitiveRiskLevel,
    recommended_action: &mut WriterMetacognitiveAction,
    candidate_risk: WriterMetacognitiveRiskLevel,
    candidate_action: WriterMetacognitiveAction,
) {
    if candidate_risk > *risk_level {
        *risk_level = candidate_risk;
        *recommended_action = candidate_action;
    }
}

fn latest_failure(events: &[WriterRunEvent]) -> Option<String> {
    events
        .iter()
        .rev()
        .find(|event| event.event_type == "writer.error")
        .and_then(|event| event.data.get("code"))
        .and_then(|value| value.as_str())
        .map(|value| value.to_string())
}

fn confidence_for(risk_level: WriterMetacognitiveRiskLevel, reason_count: usize) -> f64 {
    let base = match risk_level {
        WriterMetacognitiveRiskLevel::Low => 0.72,
        WriterMetacognitiveRiskLevel::Medium => 0.78,
        WriterMetacognitiveRiskLevel::High => 0.84,
        WriterMetacognitiveRiskLevel::Blocked => 0.9,
    };
    (base + (reason_count.saturating_sub(1) as f64 * 0.03)).min(0.96)
}

fn summary_for(
    risk_level: WriterMetacognitiveRiskLevel,
    recommended_action: WriterMetacognitiveAction,
    reasons: &[String],
) -> String {
    if reasons.is_empty() {
        return "Run health is clear enough to continue.".to_string();
    }
    format!(
        "{:?} risk: {:?}. {}",
        risk_level, recommended_action, reasons[0]
    )
}
