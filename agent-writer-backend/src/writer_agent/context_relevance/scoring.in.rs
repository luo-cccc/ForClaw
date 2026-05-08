pub(crate) fn score_canon_entity(
    entity: &CanonEntitySummary,
    observation: &WriterObservation,
    relevance: &WritingRelevance,
    open_promises: &[PlotPromiseSummary],
    memory: &WriterMemory,
    chapter_title: &str,
) -> RelevanceScore {
    let mut score = RelevanceScore::default();
    let entity_text = canon_entity_text(entity);
    if relevance.cursor_contains(&entity.name) {
        score.add(90, format!("cursor mentions entity {}", entity.name));
    }
    if relevance.foundation_contains(&entity.name) {
        score.add(
            70,
            format!("mission/result mentions entity {}", entity.name),
        );
    }
    let entity_scene_types = infer_scene_types(&entity_text);
    add_scene_type_scores(
        &mut score,
        &relevance.cursor_scene_types,
        &entity_scene_types,
        14,
        "cursor",
    );
    add_scene_type_scores(
        &mut score,
        &relevance.foundation_scene_types,
        &entity_scene_types,
        10,
        "foundation",
    );

    for term in relevance
        .cursor_terms
        .iter()
        .filter(|term| entity_text.contains(term.as_str()))
        .take(3)
    {
        score.add(16, format!("cursor term {}", term));
    }
    for term in relevance
        .foundation_terms
        .iter()
        .filter(|term| entity_text.contains(term.as_str()))
        .take(3)
    {
        score.add(12, format!("foundation term {}", term));
    }

    for promise in open_promises.iter().take(12) {
        let promise_text = promise_text(promise);
        if promise_text.contains(&entity.name) || entity_text.contains(&promise.title) {
            score.add(24, format!("linked open promise {}", promise.title));
        }
    }

    if score.score == 0 && observation.paragraph.contains(&entity.summary) {
        score.add(8, "cursor overlaps entity summary");
    }

    let filter = apply_typed_filter(&entity_text, chapter_title, memory);
    score.score = (score.score as f64 * filter.total_multiplier()) as i32;
    for reason in &filter.reasons {
        score.reasons.push(format!("typedFilter:{}", reason));
    }
    score
}

pub(crate) fn score_promise(
    promise: &PlotPromiseSummary,
    observation: &WriterObservation,
    relevance: &WritingRelevance,
    decisions: &[CreativeDecisionSummary],
    memory: &WriterMemory,
    chapter_title: &str,
) -> RelevanceScore {
    let mut score = RelevanceScore::default();
    score.add(
        promise.priority.clamp(0, 10),
        format!("ledger priority {}", promise.priority),
    );
    if relevance.cursor_contains(&promise.title) {
        score.add(90, format!("cursor mentions promise {}", promise.title));
    }
    if relevance.foundation_contains(&promise.title) {
        score.add(
            70,
            format!("mission/result mentions promise {}", promise.title),
        );
    }
    if observation
        .chapter_title
        .as_deref()
        .is_some_and(|chapter| !chapter.is_empty() && promise.expected_payoff.contains(chapter))
    {
        score.add(42, "current chapter is expected payoff");
    }

    let promise_text = promise_text(promise);
    let mut promise_scene_types = infer_scene_types(&promise_text);
    for scene_type in promise_kind_scene_types(&promise.kind) {
        if !promise_scene_types.contains(&scene_type) {
            promise_scene_types.push(scene_type);
        }
    }
    add_scene_type_scores(
        &mut score,
        &relevance.cursor_scene_types,
        &promise_scene_types,
        16,
        "cursor",
    );
    add_scene_type_scores(
        &mut score,
        &relevance.foundation_scene_types,
        &promise_scene_types,
        12,
        "foundation",
    );
    for term in relevance
        .cursor_terms
        .iter()
        .filter(|term| promise_text.contains(term.as_str()))
        .take(4)
    {
        score.add(15, format!("cursor term {}", term));
    }
    for term in relevance
        .foundation_terms
        .iter()
        .filter(|term| promise_text.contains(term.as_str()))
        .take(4)
    {
        score.add(12, format!("foundation term {}", term));
    }
    for decision in decisions.iter().take(6) {
        let decision_text = format!("{} {}", decision.title, decision.rationale);
        if decision_text.contains(&promise.title)
            || promise_text_contains_terms(&promise_text, &decision_text)
        {
            score.add(18, format!("recent decision {}", decision.title));
        }
    }

    let filter = apply_typed_filter(&promise_text, chapter_title, memory);
    score.score = (score.score as f64 * filter.total_multiplier()) as i32;
    for reason in &filter.reasons {
        score.reasons.push(format!("typedFilter:{}", reason));
    }
    score
}

pub(crate) fn format_canon_line(entity: &CanonEntitySummary, reasons: &[String]) -> String {
    let attrs = canon_attributes_text(entity);
    format!(
        "WHY writing_relevance: {} | {} [{}] {} {}",
        relevance_reason_text(reasons),
        entity.name,
        entity.kind,
        entity.summary,
        attrs
    )
}

pub(crate) fn format_promise_line(promise: &PlotPromiseSummary, reasons: &[String]) -> String {
    let mut line = format!(
        "WHY writing_relevance: {} | {} [{}]: {} -> {}",
        relevance_reason_text(reasons),
        promise.title,
        promise.kind,
        promise.description,
        promise.expected_payoff
    );
    if !promise.last_seen_chapter.trim().is_empty() {
        line.push_str(&format!(" | last seen: {}", promise.last_seen_chapter));
    }
    line
}

fn promise_text_contains_terms(promise_text: &str, decision_text: &str) -> bool {
    relevance_terms(decision_text)
        .into_iter()
        .take(6)
        .any(|term| promise_text.contains(&term))
}
