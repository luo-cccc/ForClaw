#[derive(Default)]
struct ContextSourceTrendAccumulator {
    appearances: usize,
    provided_count: usize,
    truncated_count: usize,
    dropped_count: usize,
    total_requested: usize,
    total_provided: usize,
    last_reason: Option<String>,
    last_truncation_reason: Option<String>,
}

fn context_source_trends(proposals: &[WriterProposalTrace]) -> Vec<WriterContextSourceTrend> {
    let mut trends = BTreeMap::<String, ContextSourceTrendAccumulator>::new();
    for report in proposals
        .iter()
        .filter_map(|proposal| proposal.context_budget.as_ref())
        .flat_map(|budget| budget.source_reports.iter())
    {
        let trend = trends.entry(report.source.clone()).or_default();
        trend.appearances += 1;
        trend.total_requested += report.requested;
        trend.total_provided += report.provided;
        if report.provided > 0 {
            trend.provided_count += 1;
        } else {
            trend.dropped_count += 1;
        }
        if report.truncated {
            trend.truncated_count += 1;
        }
        if !report.reason.trim().is_empty() {
            trend.last_reason = Some(report.reason.clone());
        }
        if let Some(reason) = report
            .truncation_reason
            .as_ref()
            .filter(|reason| !reason.trim().is_empty())
        {
            trend.last_truncation_reason = Some(reason.clone());
        }
    }

    let mut trends = trends
        .into_iter()
        .map(|(source, trend)| WriterContextSourceTrend {
            source,
            appearances: trend.appearances,
            provided_count: trend.provided_count,
            truncated_count: trend.truncated_count,
            dropped_count: trend.dropped_count,
            total_requested: trend.total_requested,
            total_provided: trend.total_provided,
            average_provided: if trend.appearances == 0 {
                0.0
            } else {
                trend.total_provided as f64 / trend.appearances as f64
            },
            last_reason: trend.last_reason,
            last_truncation_reason: trend.last_truncation_reason,
        })
        .collect::<Vec<_>>();
    trends.sort_by(|left, right| {
        right
            .truncated_count
            .cmp(&left.truncated_count)
            .then_with(|| right.dropped_count.cmp(&left.dropped_count))
            .then_with(|| right.appearances.cmp(&left.appearances))
            .then_with(|| left.source.cmp(&right.source))
    });
    trends
}
