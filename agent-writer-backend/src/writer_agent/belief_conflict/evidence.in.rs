fn story_contract_evidence(contract: &StoryContractSummary) -> Vec<BeliefEvidence> {
    let confidence = match contract.quality() {
        StoryContractQuality::Strong => 0.94,
        StoryContractQuality::Usable => 0.86,
        StoryContractQuality::Vague => 0.62,
        StoryContractQuality::Missing => 0.3,
    };
    let mut evidence = Vec::new();
    push_evidence(
        &mut evidence,
        BeliefSource::StoryContract,
        "story_contract:reader_promise",
        &contract.reader_promise,
        confidence,
    );
    push_evidence(
        &mut evidence,
        BeliefSource::StoryContract,
        "story_contract:main_conflict",
        &contract.main_conflict,
        confidence,
    );
    push_evidence(
        &mut evidence,
        BeliefSource::StoryContract,
        "story_contract:structural_boundary",
        &contract.structural_boundary,
        confidence,
    );
    evidence
}

fn chapter_mission_evidence(mission: &ChapterMissionSummary) -> Vec<BeliefEvidence> {
    let confidence = match mission.status.as_str() {
        "active" | "draft" | "completed" => 0.9,
        "needs_review" | "drifted" => 0.72,
        "blocked" | "retired" => 0.55,
        _ => 0.68,
    };
    let prefix = format!("chapter_mission:{}", mission.chapter_title);
    let mut evidence = Vec::new();
    push_evidence(
        &mut evidence,
        BeliefSource::ChapterMission,
        &format!("{prefix}:mission"),
        &mission.mission,
        confidence,
    );
    push_evidence(
        &mut evidence,
        BeliefSource::ChapterMission,
        &format!("{prefix}:must_include"),
        &mission.must_include,
        confidence,
    );
    push_evidence(
        &mut evidence,
        BeliefSource::ChapterMission,
        &format!("{prefix}:must_not"),
        &mission.must_not,
        confidence,
    );
    push_evidence(
        &mut evidence,
        BeliefSource::ChapterMission,
        &format!("{prefix}:expected_ending"),
        &mission.expected_ending,
        confidence,
    );
    evidence
}

fn canon_evidence(entities: &[CanonEntitySummary]) -> Vec<BeliefEvidence> {
    let mut evidence = Vec::new();
    for entity in entities {
        push_evidence(
            &mut evidence,
            BeliefSource::Canon,
            &format!("canon:{}:summary", entity.name),
            &entity.summary,
            entity.confidence,
        );
        if let Some(attributes) = entity.attributes.as_object() {
            for (key, value) in attributes {
                let value = match value {
                    serde_json::Value::String(value) => value.trim().to_string(),
                    serde_json::Value::Null => String::new(),
                    other => other.to_string(),
                };
                if value.trim().is_empty() {
                    continue;
                }
                push_evidence(
                    &mut evidence,
                    BeliefSource::Canon,
                    &format!("canon:{}:{key}", entity.name),
                    &format!("{} {key}={value}", entity.name),
                    entity.confidence,
                );
            }
        }
    }
    evidence
}

fn promise_evidence(promises: &[PlotPromiseSummary]) -> Vec<BeliefEvidence> {
    let mut evidence = Vec::new();
    for promise in promises {
        let confidence = (0.64 + (promise.priority.clamp(0, 10) as f64 * 0.025)).min(0.9);
        push_evidence(
            &mut evidence,
            BeliefSource::PromiseLedger,
            &format!("promise:{}:description", promise.id),
            &format!("{}: {}", promise.title, promise.description),
            confidence,
        );
        push_evidence(
            &mut evidence,
            BeliefSource::PromiseLedger,
            &format!("promise:{}:expected_payoff", promise.id),
            &format!(
                "{} expected payoff: {}",
                promise.title, promise.expected_payoff
            ),
            confidence,
        );
    }
    evidence
}

fn push_evidence(
    evidence: &mut Vec<BeliefEvidence>,
    source: BeliefSource,
    reference: &str,
    snippet_text: &str,
    confidence: f64,
) {
    let snippet_text = snippet_text.trim();
    if snippet_text.is_empty() {
        return;
    }
    evidence.push(BeliefEvidence {
        source,
        reference: reference.to_string(),
        snippet: snippet(snippet_text, 260),
        confidence: clamp_confidence(confidence),
        signals: Vec::new(),
    });
}

