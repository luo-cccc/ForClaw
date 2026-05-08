//! Product metric derivation from WriterAgent trace state.

use super::helpers::{operation_affected_scope, operation_kind_label};
use crate::writer_agent::feedback::{FeedbackAction, ProposalFeedback};
use crate::writer_agent::kernel::{WriterOperationLifecycleState, WriterOperationLifecycleTrace};
use crate::writer_agent::memory::{ChapterMissionSummary, ContextRecallSummary};
use crate::writer_agent::observation::{ObservationSource, WriterObservation};
use crate::writer_agent::proposal::{AgentProposal, ProposalKind};

mod trends;
pub(crate) use trends::product_metrics_trend_from_run_events;

#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WriterProductMetrics {
    pub proposal_count: u64,
    pub feedback_count: u64,
    pub accepted_count: u64,
    pub rejected_count: u64,
    pub edited_count: u64,
    pub snoozed_count: u64,
    pub explained_count: u64,
    pub ignored_count: u64,
    pub positive_feedback_count: u64,
    pub negative_feedback_count: u64,
    pub proposal_acceptance_rate: f64,
    pub ignored_repeated_suggestion_rate: f64,
    pub manual_ask_converted_to_operation_rate: f64,
    pub promise_recall_hit_rate: f64,
    pub canon_false_positive_rate: f64,
    pub chapter_mission_completion_rate: f64,
    pub durable_save_success_rate: f64,
    pub average_save_to_feedback_ms: Option<u64>,
    pub pressure_to_payoff_ratio: f64,
    pub unearned_payoff_count: u64,
    pub open_emotional_debt_count: u64,
    pub overdue_emotional_debt_count: u64,
    pub payoff_path_diversity: f64,
    pub interest_mechanism_repetition: f64,
    pub relationship_soil_coverage: f64,
    pub next_lack_handoff_rate: f64,
    pub cache_hit_token_ratio: f64,
    pub static_prefix_churn_rate: f64,
    pub focus_pack_rebuild_count: u64,
    pub ttft_ms_p50: Option<u64>,
    pub provider_duration_ms_p50: Option<u64>,
    pub estimated_cost_saved: f64,
    pub cache_miss_reason_counts: std::collections::HashMap<String, u64>,
}

#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WriterProductMetricsTrend {
    pub source_event_count: usize,
    pub session_count: usize,
    pub overall_average_save_to_feedback_ms: Option<u64>,
    pub recent_average_save_to_feedback_ms: Option<u64>,
    pub previous_average_save_to_feedback_ms: Option<u64>,
    pub save_to_feedback_delta_ms: Option<i64>,
    pub overall_context_coverage_rate: f64,
    pub recent_context_coverage_rate: f64,
    pub previous_context_coverage_rate: f64,
    pub context_coverage_delta: Option<f64>,
    pub recent_sessions: Vec<WriterProductMetricSessionTrend>,
}

#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WriterProductMetricSessionTrend {
    pub session_id: String,
    pub first_event_at: u64,
    pub last_event_at: u64,
    pub event_count: u64,
    pub proposal_count: u64,
    pub manual_ask_proposal_count: u64,
    pub manual_ask_operation_count: u64,
    pub manual_ask_converted_to_operation_rate: f64,
    pub feedback_count: u64,
    pub accepted_count: u64,
    pub rejected_count: u64,
    pub edited_count: u64,
    pub ignored_count: u64,
    pub proposal_acceptance_rate: f64,
    pub durable_save_success_rate: f64,
    pub average_save_to_feedback_ms: Option<u64>,
    pub save_feedback_sample_count: u64,
    pub context_pack_count: u64,
    pub context_requested_chars: u64,
    pub context_provided_chars: u64,
    pub context_coverage_rate: f64,
    pub context_truncated_source_count: u64,
    pub context_dropped_source_count: u64,
}

pub(crate) fn product_metrics_from_trace(
    observations: &[WriterObservation],
    proposals: &[AgentProposal],
    feedback_events: &[ProposalFeedback],
    operation_lifecycle: &[WriterOperationLifecycleTrace],
    context_recalls: Result<Vec<ContextRecallSummary>, rusqlite::Error>,
    chapter_missions: Result<Vec<ChapterMissionSummary>, rusqlite::Error>,
    emotional_debts: Result<
        Vec<crate::writer_agent::memory::EmotionalDebtSummary>,
        rusqlite::Error,
    >,
) -> WriterProductMetrics {
    let proposal_count = proposals.len() as u64;
    let feedback_count = feedback_events.len() as u64;
    let accepted_count = feedback_events
        .iter()
        .filter(|feedback| matches!(feedback.action, FeedbackAction::Accepted))
        .count() as u64;
    let rejected_count = feedback_events
        .iter()
        .filter(|feedback| matches!(feedback.action, FeedbackAction::Rejected))
        .count() as u64;
    let edited_count = feedback_events
        .iter()
        .filter(|feedback| matches!(feedback.action, FeedbackAction::Edited))
        .count() as u64;
    let snoozed_count = feedback_events
        .iter()
        .filter(|feedback| matches!(feedback.action, FeedbackAction::Snoozed))
        .count() as u64;
    let explained_count = feedback_events
        .iter()
        .filter(|feedback| matches!(feedback.action, FeedbackAction::Explained))
        .count() as u64;
    let ignored_count = rejected_count + snoozed_count + explained_count;
    let positive_feedback_count = feedback_events
        .iter()
        .filter(|feedback| feedback.is_positive())
        .count() as u64;
    let negative_feedback_count = feedback_events
        .iter()
        .filter(|feedback| feedback.is_negative())
        .count() as u64;

    let proposal_acceptance_rate = ratio(accepted_count + edited_count, feedback_count);
    let ignored_repeated_suggestion_rate = ratio(ignored_count, feedback_count);

    let manual_observation_ids = observations
        .iter()
        .filter(|observation| observation.source == ObservationSource::ManualRequest)
        .map(|observation| observation.id.as_str())
        .collect::<std::collections::HashSet<_>>();
    let manual_proposals = proposals
        .iter()
        .filter(|proposal| manual_observation_ids.contains(proposal.observation_id.as_str()))
        .count() as u64;
    let manual_operations = proposals
        .iter()
        .filter(|proposal| {
            manual_observation_ids.contains(proposal.observation_id.as_str())
                && !proposal.operations.is_empty()
        })
        .count() as u64;
    let manual_ask_converted_to_operation_rate = ratio(manual_operations, manual_proposals);

    let recalls = context_recalls.unwrap_or_default();
    let promise_recalls = recalls
        .iter()
        .filter(|recall| is_promise_context_recall(&recall.source))
        .count() as u64;
    let promise_recall_hit_rate = ratio(promise_recalls, recalls.len() as u64);

    let canon_feedback = proposals
        .iter()
        .filter(|proposal| matches!(proposal.kind, ProposalKind::CanonUpdate))
        .filter(|proposal| {
            feedback_events
                .iter()
                .any(|feedback| feedback.proposal_id == proposal.id)
        })
        .count() as u64;
    let canon_negative = proposals
        .iter()
        .filter(|proposal| matches!(proposal.kind, ProposalKind::CanonUpdate))
        .filter(|proposal| {
            feedback_events.iter().any(|feedback| {
                feedback.proposal_id == proposal.id
                    && (feedback.is_negative()
                        || matches!(feedback.action, FeedbackAction::Explained))
            })
        })
        .count() as u64;
    let canon_false_positive_rate = ratio(canon_negative, canon_feedback);

    let missions = chapter_missions.unwrap_or_default();
    let completed_missions = missions
        .iter()
        .filter(|mission| mission.status == "completed")
        .count() as u64;
    let chapter_mission_completion_rate = ratio(completed_missions, missions.len() as u64);
    let mission_with_next: usize = missions
        .iter()
        .filter(|m| !m.next_lack_opened.trim().is_empty())
        .count();
    let next_lack_handoff_rate = ratio(mission_with_next as u64, missions.len() as u64);

    let debts = emotional_debts.unwrap_or_default();
    let open_debt_count = debts.iter().filter(|d| d.payoff_status == "open").count() as u64;
    let overdue_debt_count = debts
        .iter()
        .filter(|d| d.overdue_risk == "high" && d.payoff_status == "open")
        .count() as u64;
    let paid_count = debts.iter().filter(|d| d.payoff_status == "paid").count() as u64;
    let pressure_to_payoff_ratio = ratio(paid_count, open_debt_count + paid_count);
    let relationship_soil_coverage = ratio(
        debts
            .iter()
            .filter(|d| !d.relationship_soil.trim().is_empty())
            .count() as u64,
        debts.len() as u64,
    );
    let payoff_paths: std::collections::HashSet<&str> = debts
        .iter()
        .filter_map(|d| {
            if d.payoff_path.trim().is_empty() {
                None
            } else {
                Some(d.payoff_path.as_str())
            }
        })
        .collect();
    let payoff_path_diversity = if !payoff_paths.is_empty() {
        (payoff_paths.len() as f64).ln() / (debts.len().max(1) as f64).ln().max(1.0)
    } else {
        0.0
    };
    let interest_mechanisms: std::collections::HashSet<&str> = debts
        .iter()
        .filter_map(|d| {
            if d.interest_mechanism.trim().is_empty() {
                None
            } else {
                Some(d.interest_mechanism.as_str())
            }
        })
        .collect();
    let interest_mechanism_repetition = if !debts.is_empty() && !interest_mechanisms.is_empty() {
        1.0 - (interest_mechanisms.len() as f64 / debts.len() as f64).min(1.0)
    } else {
        0.0
    };

    let durable_saves = operation_lifecycle
        .iter()
        .filter(|trace| trace.state == WriterOperationLifecycleState::DurablySaved)
        .count() as u64;
    let failed_saves = operation_lifecycle
        .iter()
        .filter(|trace| {
            trace.state == WriterOperationLifecycleState::Rejected && trace.save_result.is_some()
        })
        .count() as u64;
    let durable_save_success_rate = ratio(durable_saves, durable_saves + failed_saves);

    let mut save_to_feedback = Vec::new();
    for feedback in feedback_events {
        let Some(proposal) = proposals
            .iter()
            .find(|proposal| proposal.id == feedback.proposal_id)
        else {
            continue;
        };
        for operation in &proposal.operations {
            let Some(saved_at) = operation_lifecycle
                .iter()
                .filter(|trace| {
                    trace.proposal_id.as_deref() == Some(proposal.id.as_str())
                        && trace.operation_kind == operation_kind_label(operation)
                        && trace.affected_scope == operation_affected_scope(operation)
                        && trace.state == WriterOperationLifecycleState::DurablySaved
                })
                .map(|trace| trace.created_at)
                .max()
            else {
                continue;
            };
            if feedback.created_at >= saved_at {
                save_to_feedback.push(feedback.created_at - saved_at);
            }
        }
    }
    let average_save_to_feedback_ms = if save_to_feedback.is_empty() {
        None
    } else {
        Some(save_to_feedback.iter().sum::<u64>() / save_to_feedback.len() as u64)
    };

    WriterProductMetrics {
        proposal_count,
        feedback_count,
        accepted_count,
        rejected_count,
        edited_count,
        snoozed_count,
        explained_count,
        ignored_count,
        positive_feedback_count,
        negative_feedback_count,
        proposal_acceptance_rate,
        ignored_repeated_suggestion_rate,
        manual_ask_converted_to_operation_rate,
        promise_recall_hit_rate,
        canon_false_positive_rate,
        chapter_mission_completion_rate,
        durable_save_success_rate,
        average_save_to_feedback_ms,
        pressure_to_payoff_ratio,
        unearned_payoff_count: 0,
        open_emotional_debt_count: open_debt_count,
        overdue_emotional_debt_count: overdue_debt_count,
        payoff_path_diversity,
        interest_mechanism_repetition,
        relationship_soil_coverage,
        next_lack_handoff_rate,
        cache_hit_token_ratio: 0.0,
        static_prefix_churn_rate: 0.0,
        focus_pack_rebuild_count: 0,
        ttft_ms_p50: None,
        provider_duration_ms_p50: None,
        estimated_cost_saved: 0.0,
        cache_miss_reason_counts: std::collections::HashMap::new(),
    }
}

fn ratio(numerator: u64, denominator: u64) -> f64 {
    if denominator == 0 {
        0.0
    } else {
        numerator as f64 / denominator as f64
    }
}

fn is_promise_context_recall(source: &str) -> bool {
    matches!(source, "PromiseSlice" | "PromiseLedger")
}

fn average_u64(values: &[u64]) -> Option<u64> {
    if values.is_empty() {
        None
    } else {
        Some(values.iter().sum::<u64>() / values.len() as u64)
    }
}
