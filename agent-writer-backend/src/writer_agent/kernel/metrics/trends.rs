use std::collections::{BTreeMap, HashMap};

use crate::writer_agent::memory::RunEventSummary;

use super::{average_u64, ratio, WriterProductMetricSessionTrend, WriterProductMetricsTrend};

pub(crate) fn product_metrics_trend_from_run_events(
    events: &[RunEventSummary],
    session_limit: usize,
) -> WriterProductMetricsTrend {
    let mut sessions = BTreeMap::<String, SessionMetricAccumulator>::new();
    for event in events {
        let accumulator = sessions.entry(event.session_id.clone()).or_default();
        accumulator.record_event(event);
    }

    let mut session_trends = sessions
        .into_iter()
        .map(|(session_id, accumulator)| accumulator.into_trend(session_id))
        .filter(|trend| trend.event_count > 0)
        .collect::<Vec<_>>();
    session_trends.sort_by(|left, right| {
        right
            .last_event_at
            .cmp(&left.last_event_at)
            .then_with(|| right.first_event_at.cmp(&left.first_event_at))
            .then_with(|| left.session_id.cmp(&right.session_id))
    });

    let all_save_feedback_samples = session_trends
        .iter()
        .filter_map(|trend| {
            trend
                .average_save_to_feedback_ms
                .map(|average| (average, trend.save_feedback_sample_count))
        })
        .flat_map(|(average, count)| std::iter::repeat_n(average, count as usize))
        .collect::<Vec<_>>();
    let overall_average_save_to_feedback_ms = average_u64(&all_save_feedback_samples);
    let recent_average_save_to_feedback_ms = session_trends
        .iter()
        .find_map(|trend| trend.average_save_to_feedback_ms);
    let previous_average_save_to_feedback_ms = session_trends
        .iter()
        .filter_map(|trend| trend.average_save_to_feedback_ms)
        .nth(1);
    let save_to_feedback_delta_ms = recent_average_save_to_feedback_ms
        .zip(previous_average_save_to_feedback_ms)
        .map(|(recent, previous)| recent as i64 - previous as i64);
    let total_context_requested = session_trends
        .iter()
        .map(|trend| trend.context_requested_chars)
        .sum::<u64>();
    let total_context_provided = session_trends
        .iter()
        .map(|trend| trend.context_provided_chars)
        .sum::<u64>();
    let overall_context_coverage_rate = ratio(total_context_provided, total_context_requested);
    let recent_context_coverage_rate = session_trends
        .iter()
        .find(|trend| trend.context_pack_count > 0)
        .map(|trend| trend.context_coverage_rate)
        .unwrap_or_default();
    let previous_context_coverage_rate = session_trends
        .iter()
        .filter(|trend| trend.context_pack_count > 0)
        .map(|trend| trend.context_coverage_rate)
        .nth(1)
        .unwrap_or_default();
    let context_coverage_delta = session_trends
        .iter()
        .filter(|trend| trend.context_pack_count > 0)
        .map(|trend| trend.context_coverage_rate)
        .take(2)
        .collect::<Vec<_>>();
    let context_coverage_delta = context_coverage_delta
        .first()
        .zip(context_coverage_delta.get(1))
        .map(|(recent, previous)| recent - previous);
    let session_count = session_trends.len();
    let recent_sessions = session_trends
        .into_iter()
        .take(session_limit)
        .collect::<Vec<_>>();

    WriterProductMetricsTrend {
        source_event_count: events.len(),
        session_count,
        overall_average_save_to_feedback_ms,
        recent_average_save_to_feedback_ms,
        previous_average_save_to_feedback_ms,
        save_to_feedback_delta_ms,
        overall_context_coverage_rate,
        recent_context_coverage_rate,
        previous_context_coverage_rate,
        context_coverage_delta,
        recent_sessions,
    }
}

#[derive(Default)]
struct SessionMetricAccumulator {
    first_event_at: Option<u64>,
    last_event_at: u64,
    event_count: u64,
    proposal_count: u64,
    manual_ask_proposal_count: u64,
    manual_ask_operation_count: u64,
    feedback_count: u64,
    accepted_count: u64,
    rejected_count: u64,
    edited_count: u64,
    snoozed_count: u64,
    explained_count: u64,
    durable_save_count: u64,
    failed_save_count: u64,
    durable_saves_by_proposal: HashMap<String, Vec<u64>>,
    feedback_by_proposal: Vec<(String, u64)>,
    context_pack_count: u64,
    context_requested_chars: u64,
    context_provided_chars: u64,
    context_truncated_source_count: u64,
    context_dropped_source_count: u64,
}

impl SessionMetricAccumulator {
    fn record_event(&mut self, event: &RunEventSummary) {
        self.event_count = self.event_count.saturating_add(1);
        self.first_event_at = Some(
            self.first_event_at
                .map(|existing| existing.min(event.ts_ms))
                .unwrap_or(event.ts_ms),
        );
        self.last_event_at = self.last_event_at.max(event.ts_ms);

        match event.event_type.as_str() {
            "writer.proposal_created" => {
                self.proposal_count = self.proposal_count.saturating_add(1);
                self.record_proposal_created_event(event);
            }
            "writer.feedback_recorded" => {
                self.record_feedback_event(event);
            }
            "writer.operation_lifecycle" => {
                self.record_operation_lifecycle_event(event);
            }
            "writer.context_pack_built" => {
                self.record_context_pack_event(event);
            }
            _ => {}
        }
    }

    fn record_proposal_created_event(&mut self, event: &RunEventSummary) {
        let is_manual_ask = json_string(&event.data, "observationSource")
            .map(|source| normalize_metric_label(&source) == "manual_request")
            .unwrap_or(false);
        if !is_manual_ask {
            return;
        }

        self.manual_ask_proposal_count = self.manual_ask_proposal_count.saturating_add(1);
        if json_number(&event.data, "operationCount").unwrap_or_default() > 0 {
            self.manual_ask_operation_count = self.manual_ask_operation_count.saturating_add(1);
        }
    }

    fn record_feedback_event(&mut self, event: &RunEventSummary) {
        self.feedback_count = self.feedback_count.saturating_add(1);
        let action = json_string(&event.data, "action")
            .as_deref()
            .map(normalize_metric_label)
            .unwrap_or_default();
        match action.as_str() {
            "accepted" => self.accepted_count = self.accepted_count.saturating_add(1),
            "rejected" => self.rejected_count = self.rejected_count.saturating_add(1),
            "edited" => self.edited_count = self.edited_count.saturating_add(1),
            "snoozed" => self.snoozed_count = self.snoozed_count.saturating_add(1),
            "explained" => self.explained_count = self.explained_count.saturating_add(1),
            _ => {}
        }
        if let Some(proposal_id) = json_string(&event.data, "proposalId") {
            self.feedback_by_proposal.push((proposal_id, event.ts_ms));
        }
    }

    fn record_operation_lifecycle_event(&mut self, event: &RunEventSummary) {
        let state = json_string(&event.data, "state")
            .map(|state| normalize_metric_label(&state))
            .unwrap_or_default();
        if state == "durably_saved" {
            self.durable_save_count = self.durable_save_count.saturating_add(1);
            if let Some(proposal_id) = json_string(&event.data, "proposalId") {
                self.durable_saves_by_proposal
                    .entry(proposal_id)
                    .or_default()
                    .push(event.ts_ms);
            }
        } else if state == "rejected"
            && json_string(&event.data, "saveResult").is_some_and(|value| !value.trim().is_empty())
        {
            self.failed_save_count = self.failed_save_count.saturating_add(1);
        }
    }

    fn record_context_pack_event(&mut self, event: &RunEventSummary) {
        self.context_pack_count = self.context_pack_count.saturating_add(1);
        let Some(source_reports) = event
            .data
            .get("sourceReports")
            .and_then(|value| value.as_array())
        else {
            self.context_requested_chars = self
                .context_requested_chars
                .saturating_add(json_number(&event.data, "budgetLimit").unwrap_or_default());
            self.context_provided_chars = self
                .context_provided_chars
                .saturating_add(json_number(&event.data, "totalChars").unwrap_or_default());
            self.context_truncated_source_count =
                self.context_truncated_source_count.saturating_add(
                    json_number(&event.data, "truncatedSourceCount").unwrap_or_default(),
                );
            return;
        };

        for report in source_reports {
            let provided = json_number(report, "provided").unwrap_or_default();
            let requested = json_number(report, "requested")
                .or_else(|| json_number(report, "originalChars"))
                .unwrap_or(provided);
            self.context_requested_chars = self.context_requested_chars.saturating_add(requested);
            self.context_provided_chars = self.context_provided_chars.saturating_add(provided);
            if json_bool(report, "truncated").unwrap_or(false) {
                self.context_truncated_source_count =
                    self.context_truncated_source_count.saturating_add(1);
            }
            if provided == 0 {
                self.context_dropped_source_count =
                    self.context_dropped_source_count.saturating_add(1);
            }
        }
    }

    fn into_trend(self, session_id: String) -> WriterProductMetricSessionTrend {
        let mut save_to_feedback = Vec::new();
        for (proposal_id, feedback_at) in &self.feedback_by_proposal {
            if let Some(saved_events) = self.durable_saves_by_proposal.get(proposal_id) {
                for saved_at in saved_events {
                    if feedback_at >= saved_at {
                        save_to_feedback.push(feedback_at - saved_at);
                    }
                }
            }
        }

        let ignored_count = self
            .rejected_count
            .saturating_add(self.snoozed_count)
            .saturating_add(self.explained_count);
        WriterProductMetricSessionTrend {
            session_id,
            first_event_at: self.first_event_at.unwrap_or_default(),
            last_event_at: self.last_event_at,
            event_count: self.event_count,
            proposal_count: self.proposal_count,
            manual_ask_proposal_count: self.manual_ask_proposal_count,
            manual_ask_operation_count: self.manual_ask_operation_count,
            manual_ask_converted_to_operation_rate: ratio(
                self.manual_ask_operation_count,
                self.manual_ask_proposal_count,
            ),
            feedback_count: self.feedback_count,
            accepted_count: self.accepted_count,
            rejected_count: self.rejected_count,
            edited_count: self.edited_count,
            ignored_count,
            proposal_acceptance_rate: ratio(
                self.accepted_count.saturating_add(self.edited_count),
                self.feedback_count,
            ),
            durable_save_success_rate: ratio(
                self.durable_save_count,
                self.durable_save_count
                    .saturating_add(self.failed_save_count),
            ),
            average_save_to_feedback_ms: average_u64(&save_to_feedback),
            save_feedback_sample_count: save_to_feedback.len() as u64,
            context_pack_count: self.context_pack_count,
            context_requested_chars: self.context_requested_chars,
            context_provided_chars: self.context_provided_chars,
            context_coverage_rate: ratio(self.context_provided_chars, self.context_requested_chars),
            context_truncated_source_count: self.context_truncated_source_count,
            context_dropped_source_count: self.context_dropped_source_count,
        }
    }
}

fn json_string(value: &serde_json::Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(|field| field.as_str())
        .map(str::trim)
        .filter(|field| !field.is_empty())
        .map(ToOwned::to_owned)
}

fn json_number(value: &serde_json::Value, key: &str) -> Option<u64> {
    value.get(key).and_then(|field| field.as_u64())
}

fn json_bool(value: &serde_json::Value, key: &str) -> Option<bool> {
    value.get(key).and_then(|field| field.as_bool())
}

fn normalize_metric_label(value: &str) -> String {
    value
        .chars()
        .enumerate()
        .flat_map(|(index, ch)| {
            let mut output = Vec::new();
            if ch.is_ascii_uppercase() && index > 0 {
                output.push('_');
            }
            output.push(ch.to_ascii_lowercase());
            output
        })
        .collect()
}
