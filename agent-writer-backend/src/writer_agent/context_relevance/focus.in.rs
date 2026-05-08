struct WritingRelevanceFocus {
    raw_text: String,
    terms: Vec<String>,
    negative_terms: Vec<String>,
    scene_types: Vec<WritingSceneType>,
}

impl WritingRelevanceFocus {
    fn new(text: &str) -> Self {
        let negative_terms = negative_relevance_terms(text);
        let terms = relevance_terms(text)
            .into_iter()
            .filter(|term| !is_blocked_by_negative_terms(term, &negative_terms))
            .collect();
        Self {
            raw_text: text.to_string(),
            terms,
            negative_terms,
            scene_types: infer_scene_types(text),
        }
    }
}

fn score_text_chunk(focus: &WritingRelevanceFocus, text: &str) -> RelevanceScore {
    let mut score = RelevanceScore::default();
    let chunk_scene_types = infer_scene_types(text);
    add_scene_type_scores(
        &mut score,
        &focus.scene_types,
        &chunk_scene_types,
        28,
        "focus",
    );
    let mut matched_terms = focus
        .terms
        .iter()
        .filter(|term| text.contains(term.as_str()))
        .map(|term| {
            let weight = if focus.raw_text.contains(term.as_str()) {
                1
            } else {
                0
            };
            let points = 18 + (term.chars().count().min(8) as i32 * 2) + weight;
            (points, term)
        })
        .collect::<Vec<_>>();
    matched_terms.sort_by(|(left_points, left_term), (right_points, right_term)| {
        right_points
            .cmp(left_points)
            .then_with(|| right_term.chars().count().cmp(&left_term.chars().count()))
    });
    let mut explained_terms: Vec<&String> = Vec::new();
    for (points, term) in matched_terms {
        if explained_terms
            .iter()
            .any(|explained| explained.contains(term.as_str()) || term.contains(explained.as_str()))
        {
            continue;
        }
        score.add(points, format!("writing term {}", term));
        explained_terms.push(term);
        if score.reasons.len() >= 5 {
            break;
        }
    }
    for term in focus
        .negative_terms
        .iter()
        .filter(|term| text.contains(term.as_str()))
        .take(3)
    {
        score.add(-72, format!("avoid term {}", term));
    }
    score
}
