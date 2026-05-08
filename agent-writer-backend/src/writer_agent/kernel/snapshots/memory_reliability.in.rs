#[derive(Default)]
struct MemoryReliabilityAccumulator {
    slot: String,
    category: String,
    reinforcement_count: u64,
    correction_count: u64,
    net_confidence_delta: f64,
    last_action: String,
    last_source_error: Option<String>,
    last_reason: Option<String>,
    last_proposal_id: String,
    updated_at: u64,
}

fn memory_reliability_summary(
    feedback: Vec<super::memory::MemoryFeedbackSummary>,
) -> Vec<WriterMemoryReliabilitySummary> {
    let mut slots = BTreeMap::<String, MemoryReliabilityAccumulator>::new();
    for event in feedback {
        let entry =
            slots
                .entry(event.slot.clone())
                .or_insert_with(|| MemoryReliabilityAccumulator {
                    slot: event.slot.clone(),
                    category: event.category.clone(),
                    ..Default::default()
                });
        if entry.category.trim().is_empty() || entry.category == "unknown" {
            entry.category = event.category.clone();
        }
        match event.action.as_str() {
            "reinforcement" => {
                entry.reinforcement_count = entry.reinforcement_count.saturating_add(1)
            }
            "correction" => entry.correction_count = entry.correction_count.saturating_add(1),
            _ => {}
        }
        entry.net_confidence_delta += event.confidence_delta;
        if event.created_at >= entry.updated_at {
            entry.updated_at = event.created_at;
            entry.last_action = event.action.clone();
            entry.last_source_error = event.source_error.clone();
            entry.last_reason = event.reason.clone();
            entry.last_proposal_id = event.proposal_id.clone();
        }
    }

    let mut summaries = slots
        .into_values()
        .map(|entry| {
            let reliability = (0.5 + entry.net_confidence_delta).clamp(0.0, 1.0);
            let status = if entry.correction_count > 0
                && entry.correction_count >= entry.reinforcement_count
            {
                "needs_review"
            } else if reliability >= 0.55 && entry.reinforcement_count > 0 {
                "trusted"
            } else {
                "unproven"
            };
            WriterMemoryReliabilitySummary {
                slot: entry.slot,
                category: entry.category,
                status: status.to_string(),
                reliability,
                reinforcement_count: entry.reinforcement_count,
                correction_count: entry.correction_count,
                net_confidence_delta: entry.net_confidence_delta,
                last_action: entry.last_action,
                last_source_error: entry.last_source_error,
                last_reason: entry.last_reason,
                last_proposal_id: entry.last_proposal_id,
                updated_at: entry.updated_at,
            }
        })
        .collect::<Vec<_>>();
    summaries.sort_by(|left, right| {
        reliability_status_weight(&right.status)
            .cmp(&reliability_status_weight(&left.status))
            .then_with(|| right.updated_at.cmp(&left.updated_at))
            .then_with(|| left.slot.cmp(&right.slot))
    });
    summaries
}

fn reliability_status_weight(status: &str) -> u8 {
    match status {
        "needs_review" => 3,
        "unproven" => 2,
        "trusted" => 1,
        _ => 0,
    }
}
