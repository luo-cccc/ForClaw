//! Inspector-safe timeline views derived from Writer Agent trace snapshots.
//!
//! The default companion view stays product-facing and omits internal packets,
//! tool policies, lifecycle traces, and raw run events. The inspector view is
//! explicit debug surface for replay and diagnosis.

use serde::{Deserialize, Serialize};

use super::kernel::WriterAgentTraceSnapshot;
use super::run_events::WriterRunEvent;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WriterTimelineAudience {
    Companion,
    Inspector,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WriterTimelineEventKind {
    Observation,
    TaskPacket,
    Proposal,
    Feedback,
    OperationLifecycle,
    RunEvent,
    Failure,
    Subtask,
    TaskReceipt,
    TaskArtifact,
    ContextRecall,
    ProductMetrics,
    Metacognition,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WriterTimelineEvent {
    pub audience: WriterTimelineAudience,
    pub kind: WriterTimelineEventKind,
    pub label: String,
    pub ts_ms: u64,
    pub task_id: Option<String>,
    pub source_refs: Vec<String>,
    pub summary: String,
    pub detail: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WriterInspectorTimeline {
    pub audience: WriterTimelineAudience,
    pub includes_internal_trace: bool,
    pub events: Vec<WriterTimelineEvent>,
}

pub fn build_inspector_timeline(
    snapshot: &WriterAgentTraceSnapshot,
    limit: usize,
) -> WriterInspectorTimeline {
    let mut events = Vec::new();
    for observation in &snapshot.recent_observations {
        events.push(WriterTimelineEvent {
            audience: WriterTimelineAudience::Inspector,
            kind: WriterTimelineEventKind::Observation,
            label: "Observation".to_string(),
            ts_ms: observation.created_at,
            task_id: Some(observation.id.clone()),
            source_refs: observation
                .chapter_title
                .iter()
                .map(|chapter| format!("chapter:{}", chapter))
                .collect(),
            summary: observation.paragraph_snippet.clone(),
            detail: Some(serde_json::json!({
                "reason": observation.reason,
                "chapterTitle": observation.chapter_title,
            })),
        });
    }
    for task_packet in &snapshot.task_packets {
        events.push(WriterTimelineEvent {
            audience: WriterTimelineAudience::Inspector,
            kind: WriterTimelineEventKind::TaskPacket,
            label: format!("TaskPacket {}", task_packet.task),
            ts_ms: task_packet.packet.created_at_ms,
            task_id: Some(task_packet.id.clone()),
            source_refs: vec![task_packet.observation_id.clone()],
            summary: task_packet.objective.clone(),
            detail: Some(serde_json::json!({
                "scope": task_packet.scope,
                "intent": task_packet.intent,
                "maxSideEffectLevel": task_packet.max_side_effect_level,
                "requiredContextCount": task_packet.required_context_count,
                "beliefCount": task_packet.belief_count,
                "feedbackCheckpointCount": task_packet.feedback_checkpoint_count,
                "foundationComplete": task_packet.foundation_complete,
            })),
        });
    }
    for proposal in &snapshot.recent_proposals {
        events.push(WriterTimelineEvent {
            audience: WriterTimelineAudience::Inspector,
            kind: WriterTimelineEventKind::Proposal,
            label: format!("Proposal {}", proposal.kind),
            ts_ms: 0,
            task_id: Some(proposal.id.clone()),
            source_refs: vec![proposal.observation_id.clone()],
            summary: proposal.preview_snippet.clone(),
            detail: Some(serde_json::json!({
                "priority": proposal.priority,
                "state": proposal.state,
                "confidence": proposal.confidence,
                "evidenceCount": proposal.evidence.len(),
                "hasContextBudget": proposal.context_budget.is_some(),
            })),
        });
    }
    for feedback in &snapshot.recent_feedback {
        events.push(WriterTimelineEvent {
            audience: WriterTimelineAudience::Inspector,
            kind: WriterTimelineEventKind::Feedback,
            label: format!("Feedback {}", feedback.action),
            ts_ms: feedback.created_at,
            task_id: Some(feedback.proposal_id.clone()),
            source_refs: vec![feedback.proposal_id.clone()],
            summary: feedback.reason.clone().unwrap_or_default(),
            detail: None,
        });
    }
    for lifecycle in &snapshot.operation_lifecycle {
        events.push(WriterTimelineEvent {
            audience: WriterTimelineAudience::Inspector,
            kind: WriterTimelineEventKind::OperationLifecycle,
            label: format!("Operation {}", lifecycle.operation_kind),
            ts_ms: lifecycle.created_at,
            task_id: lifecycle.proposal_id.clone(),
            source_refs: lifecycle.proposal_id.iter().cloned().collect(),
            summary: format!("{:?}", lifecycle.state),
            detail: Some(serde_json::json!({
                "sourceTask": lifecycle.source_task,
                "approvalSource": lifecycle.approval_source,
                "affectedScope": lifecycle.affected_scope,
                "saveResult": lifecycle.save_result,
                "feedbackResult": lifecycle.feedback_result,
            })),
        });
    }
    for run_event in &snapshot.run_events {
        if run_event.event_type == "writer.error" {
            events.push(failure_event_from_run_event(run_event));
            continue;
        }
        if run_event.event_type == "writer.subtask_started"
            || run_event.event_type == "writer.subtask_completed"
        {
            events.push(subtask_event_from_run_event(run_event));
            continue;
        }
        if run_event.event_type == "writer.task_receipt" {
            events.push(task_receipt_event_from_run_event(run_event));
            continue;
        }
        if run_event.event_type == "writer.task_artifact" {
            events.push(task_artifact_event_from_run_event(run_event));
            continue;
        }
        events.push(WriterTimelineEvent {
            audience: WriterTimelineAudience::Inspector,
            kind: WriterTimelineEventKind::RunEvent,
            label: run_event.event_type.clone(),
            ts_ms: run_event.ts_ms,
            task_id: run_event.task_id.clone(),
            source_refs: run_event.source_refs.clone(),
            summary: run_event.event_type.clone(),
            detail: Some(run_event.data.clone()),
        });
    }
    for recall in &snapshot.context_recalls {
        events.push(WriterTimelineEvent {
            audience: WriterTimelineAudience::Inspector,
            kind: WriterTimelineEventKind::ContextRecall,
            label: format!("Context {}", recall.source),
            ts_ms: recall.last_recalled_at,
            task_id: Some(recall.last_proposal_id.clone()),
            source_refs: vec![recall.reference.clone()],
            summary: recall.snippet.clone(),
            detail: Some(serde_json::json!({
                "recallCount": recall.recall_count,
                "lastObservationId": recall.last_observation_id,
            })),
        });
    }
    events.push(WriterTimelineEvent {
        audience: WriterTimelineAudience::Inspector,
        kind: WriterTimelineEventKind::ProductMetrics,
        label: "Product metrics".to_string(),
        ts_ms: snapshot
            .recent_feedback
            .iter()
            .map(|feedback| feedback.created_at)
            .max()
            .unwrap_or(0),
        task_id: None,
        source_refs: Vec::new(),
        summary: format!(
            "acceptance={:.2} durable_save={:.2} sessions={} save_feedback_delta_ms={}",
            snapshot.product_metrics.proposal_acceptance_rate,
            snapshot.product_metrics.durable_save_success_rate,
            snapshot.product_metrics_trend.session_count,
            snapshot
                .product_metrics_trend
                .save_to_feedback_delta_ms
                .map(|delta| delta.to_string())
                .unwrap_or_else(|| "n/a".to_string())
        ),
        detail: Some(serde_json::json!({
            "metrics": snapshot.product_metrics,
            "trend": snapshot.product_metrics_trend,
        })),
    });
    events.push(WriterTimelineEvent {
        audience: WriterTimelineAudience::Inspector,
        kind: WriterTimelineEventKind::Metacognition,
        label: "Metacognitive gate".to_string(),
        ts_ms: snapshot
            .run_events
            .iter()
            .map(|event| event.ts_ms)
            .max()
            .or_else(|| {
                snapshot
                    .recent_feedback
                    .iter()
                    .map(|feedback| feedback.created_at)
                    .max()
            })
            .unwrap_or(0),
        task_id: None,
        source_refs: Vec::new(),
        summary: snapshot.metacognitive_snapshot.summary.clone(),
        detail: Some(serde_json::json!(snapshot.metacognitive_snapshot)),
    });

    events.sort_by(|left, right| {
        left.ts_ms
            .cmp(&right.ts_ms)
            .then_with(|| event_kind_weight(&left.kind).cmp(&event_kind_weight(&right.kind)))
            .then_with(|| left.label.cmp(&right.label))
    });
    let keep = limit.min(events.len());
    let events = events.into_iter().rev().take(keep).collect::<Vec<_>>();
    WriterInspectorTimeline {
        audience: WriterTimelineAudience::Inspector,
        includes_internal_trace: true,
        events,
    }
}

pub fn build_companion_timeline_summary(
    snapshot: &WriterAgentTraceSnapshot,
) -> WriterInspectorTimeline {
    let metrics = &snapshot.product_metrics;
    let mut events = Vec::new();
    events.push(WriterTimelineEvent {
        audience: WriterTimelineAudience::Companion,
        kind: WriterTimelineEventKind::ProductMetrics,
        label: "Writing health".to_string(),
        ts_ms: snapshot
            .recent_feedback
            .iter()
            .map(|feedback| feedback.created_at)
            .max()
            .unwrap_or(0),
        task_id: None,
        source_refs: Vec::new(),
        summary: format!(
            "acceptance={:.2} ignored={:.2} durable_save={:.2}",
            metrics.proposal_acceptance_rate,
            metrics.ignored_repeated_suggestion_rate,
            metrics.durable_save_success_rate
        ),
        detail: Some(serde_json::json!({
            "proposalCount": metrics.proposal_count,
            "feedbackCount": metrics.feedback_count,
            "promiseRecallHitRate": metrics.promise_recall_hit_rate,
            "chapterMissionCompletionRate": metrics.chapter_mission_completion_rate,
        })),
    });
    if snapshot.metacognitive_snapshot.recommended_action
        != crate::writer_agent::metacognition::WriterMetacognitiveAction::Proceed
    {
        events.push(WriterTimelineEvent {
            audience: WriterTimelineAudience::Companion,
            kind: WriterTimelineEventKind::Metacognition,
            label: "Run guard".to_string(),
            ts_ms: snapshot
                .run_events
                .iter()
                .map(|event| event.ts_ms)
                .max()
                .unwrap_or(0),
            task_id: None,
            source_refs: Vec::new(),
            summary: snapshot.metacognitive_snapshot.summary.clone(),
            detail: Some(serde_json::json!({
                "riskLevel": snapshot.metacognitive_snapshot.risk_level,
                "recommendedAction": snapshot.metacognitive_snapshot.recommended_action,
                "confidence": snapshot.metacognitive_snapshot.confidence,
                "reasonCount": snapshot.metacognitive_snapshot.reasons.len(),
            })),
        });
    }
    for proposal in snapshot.recent_proposals.iter().take(3) {
        events.push(WriterTimelineEvent {
            audience: WriterTimelineAudience::Companion,
            kind: WriterTimelineEventKind::Proposal,
            label: proposal.kind.clone(),
            ts_ms: 0,
            task_id: Some(proposal.id.clone()),
            source_refs: Vec::new(),
            summary: proposal.preview_snippet.clone(),
            detail: Some(serde_json::json!({
                "priority": proposal.priority,
                "state": proposal.state,
            })),
        });
    }

    WriterInspectorTimeline {
        audience: WriterTimelineAudience::Companion,
        includes_internal_trace: false,
        events,
    }
}

fn event_kind_weight(kind: &WriterTimelineEventKind) -> u8 {
    match kind {
        WriterTimelineEventKind::Observation => 0,
        WriterTimelineEventKind::TaskPacket => 1,
        WriterTimelineEventKind::RunEvent => 2,
        WriterTimelineEventKind::Subtask => 3,
        WriterTimelineEventKind::TaskReceipt => 4,
        WriterTimelineEventKind::TaskArtifact => 5,
        WriterTimelineEventKind::Failure => 6,
        WriterTimelineEventKind::Proposal => 7,
        WriterTimelineEventKind::OperationLifecycle => 8,
        WriterTimelineEventKind::Feedback => 9,
        WriterTimelineEventKind::ContextRecall => 10,
        WriterTimelineEventKind::ProductMetrics => 11,
        WriterTimelineEventKind::Metacognition => 12,
    }
}

fn task_artifact_event_from_run_event(run_event: &WriterRunEvent) -> WriterTimelineEvent {
    let task_kind = run_event
        .data
        .get("taskKind")
        .and_then(|value| value.as_str())
        .unwrap_or("task");
    let artifact_kind = run_event
        .data
        .get("artifactKind")
        .and_then(|value| value.as_str())
        .unwrap_or("artifact");
    let chapter = run_event
        .data
        .get("chapter")
        .and_then(|value| value.as_str())
        .unwrap_or("chapter n/a");
    let content_char_count = run_event
        .data
        .get("contentCharCount")
        .and_then(|value| value.as_u64())
        .unwrap_or(0);
    let truncated = run_event
        .data
        .get("contentTruncated")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    let evidence_count = run_event
        .data
        .get("requiredEvidence")
        .and_then(|value| value.as_array())
        .map(|items| items.iter().filter(|item| item.as_str().is_some()).count())
        .unwrap_or(0);

    WriterTimelineEvent {
        audience: WriterTimelineAudience::Inspector,
        kind: WriterTimelineEventKind::TaskArtifact,
        label: format!("TaskArtifact {} {}", task_kind, artifact_kind),
        ts_ms: run_event.ts_ms,
        task_id: run_event.task_id.clone(),
        source_refs: run_event.source_refs.clone(),
        summary: format!(
            "{} {} for {} chars={} truncated={} evidence={}",
            task_kind, artifact_kind, chapter, content_char_count, truncated, evidence_count
        ),
        detail: Some(run_event.data.clone()),
    }
}

fn task_receipt_event_from_run_event(run_event: &WriterRunEvent) -> WriterTimelineEvent {
    let task_kind = run_event
        .data
        .get("taskKind")
        .and_then(|value| value.as_str())
        .unwrap_or("task");
    let chapter = run_event
        .data
        .get("chapter")
        .and_then(|value| value.as_str())
        .unwrap_or("chapter n/a");
    let evidence_count = run_event
        .data
        .get("requiredEvidence")
        .and_then(|value| value.as_array())
        .map(|items| items.iter().filter(|item| item.as_str().is_some()).count())
        .unwrap_or(0);
    let artifact_count = run_event
        .data
        .get("expectedArtifacts")
        .and_then(|value| value.as_array())
        .map(|items| items.iter().filter(|item| item.as_str().is_some()).count())
        .unwrap_or(0);
    let guard_count = run_event
        .data
        .get("mustNot")
        .and_then(|value| value.as_array())
        .map(|items| items.iter().filter(|item| item.as_str().is_some()).count())
        .unwrap_or(0);

    WriterTimelineEvent {
        audience: WriterTimelineAudience::Inspector,
        kind: WriterTimelineEventKind::TaskReceipt,
        label: format!("TaskReceipt {}", task_kind),
        ts_ms: run_event.ts_ms,
        task_id: run_event.task_id.clone(),
        source_refs: run_event.source_refs.clone(),
        summary: format!(
            "{} receipt for {} evidence={} artifacts={} guards={}",
            task_kind, chapter, evidence_count, artifact_count, guard_count
        ),
        detail: Some(run_event.data.clone()),
    }
}

fn subtask_event_from_run_event(run_event: &WriterRunEvent) -> WriterTimelineEvent {
    let kind = run_event
        .data
        .get("kind")
        .and_then(|value| value.as_str())
        .unwrap_or("subtask");
    let status = run_event
        .data
        .get("status")
        .and_then(|value| value.as_str())
        .unwrap_or("recorded");
    let summary = run_event
        .data
        .get("summary")
        .and_then(|value| value.as_str())
        .or_else(|| {
            run_event
                .data
                .get("objective")
                .and_then(|value| value.as_str())
        })
        .unwrap_or("Writer subtask event recorded.");
    let evidence_count = run_event
        .data
        .get("evidenceCount")
        .and_then(|value| value.as_u64())
        .unwrap_or(0);
    let artifact_count = run_event
        .data
        .get("artifactCount")
        .and_then(|value| value.as_u64())
        .unwrap_or(0);
    let blocked_count = run_event
        .data
        .get("blockedOperationCount")
        .and_then(|value| value.as_u64())
        .unwrap_or(0);

    WriterTimelineEvent {
        audience: WriterTimelineAudience::Inspector,
        kind: WriterTimelineEventKind::Subtask,
        label: format!("Subtask {} {}", kind, status),
        ts_ms: run_event.ts_ms,
        task_id: run_event.task_id.clone(),
        source_refs: run_event.source_refs.clone(),
        summary: format!(
            "{} evidence={} artifacts={} blocked_ops={}",
            summary, evidence_count, artifact_count, blocked_count
        ),
        detail: Some(run_event.data.clone()),
    }
}

fn failure_event_from_run_event(run_event: &WriterRunEvent) -> WriterTimelineEvent {
    let code = run_event
        .data
        .get("code")
        .and_then(|value| value.as_str())
        .unwrap_or("WRITER_ERROR");
    let category = run_event
        .data
        .get("category")
        .and_then(|value| value.as_str())
        .unwrap_or("unknown");
    let message = run_event
        .data
        .get("message")
        .and_then(|value| value.as_str())
        .unwrap_or("Writer failure recorded.");
    let remediation_count = run_event
        .data
        .get("remediation")
        .and_then(|value| value.as_array())
        .map(|items| items.iter().filter(|item| item.as_str().is_some()).count())
        .unwrap_or(0);
    let first_remediation = run_event
        .data
        .get("remediation")
        .and_then(|value| value.as_array())
        .and_then(|items| items.iter().find_map(|item| item.as_str()))
        .unwrap_or("Inspect the failure bundle before retrying.");

    WriterTimelineEvent {
        audience: WriterTimelineAudience::Inspector,
        kind: WriterTimelineEventKind::Failure,
        label: format!("Failure {}", code),
        ts_ms: run_event.ts_ms,
        task_id: run_event.task_id.clone(),
        source_refs: run_event.source_refs.clone(),
        summary: format!(
            "{} [{}] remediation={} first={}",
            message, category, remediation_count, first_remediation
        ),
        detail: Some(run_event.data.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::writer_agent::kernel::WriterProductMetrics;

    #[test]
    fn companion_summary_excludes_internal_events() {
        let snapshot = WriterAgentTraceSnapshot {
            recent_observations: Vec::new(),
            task_packets: Vec::new(),
            recent_proposals: Vec::new(),
            recent_feedback: Vec::new(),
            operation_lifecycle: Vec::new(),
            run_events: Vec::new(),
            post_write_diagnostics: Vec::new(),
            context_source_trends: Vec::new(),
            context_recalls: Vec::new(),
            product_metrics: WriterProductMetrics::default(),
            product_metrics_trend: Default::default(),
            metacognitive_snapshot: Default::default(),
        };
        let summary = build_companion_timeline_summary(&snapshot);
        assert!(!summary.includes_internal_trace);
        assert!(summary.events.iter().all(|event| {
            event.audience == WriterTimelineAudience::Companion
                && !matches!(
                    event.kind,
                    WriterTimelineEventKind::TaskPacket
                        | WriterTimelineEventKind::RunEvent
                        | WriterTimelineEventKind::Failure
                        | WriterTimelineEventKind::Subtask
                        | WriterTimelineEventKind::TaskReceipt
                        | WriterTimelineEventKind::TaskArtifact
                        | WriterTimelineEventKind::OperationLifecycle
                        | WriterTimelineEventKind::Metacognition
                )
        }));
    }
}
