pub fn rerank_project_brain_results<'a>(
    results: Vec<(f32, &'a Chunk)>,
    writing_focus: &str,
) -> Vec<(f32, Vec<String>, &'a Chunk)> {
    rerank_project_brain_results_with_focus(
        results,
        &ProjectBrainFocus {
            query_text: writing_focus.to_string(),
            memory_text: String::new(),
            active_chapter: None,
        },
    )
}

pub fn search_project_brain_results_with_focus<'a>(
    db: &'a VectorDB,
    focus: &ProjectBrainFocus,
    query_embedding: &[f32],
) -> Vec<(f32, Vec<String>, &'a Chunk)> {
    let search_text = focus.search_text();
    rerank_project_brain_results_with_focus(
        db.search_hybrid(
            &search_text,
            query_embedding,
            TOP_K * RERANK_CANDIDATE_MULTIPLIER,
        ),
        focus,
    )
}

pub fn rerank_project_brain_results_with_focus<'a>(
    results: Vec<(f32, &'a Chunk)>,
    focus: &ProjectBrainFocus,
) -> Vec<(f32, Vec<String>, &'a Chunk)> {
    let has_memory_focus = !focus.memory_str().trim().is_empty();
    let mut scored = results
        .into_iter()
        .map(|(base_score, chunk)| {
            let text = project_brain_chunk_text(chunk);
            let (query_score, query_reasons) =
                score_text_for_writing_focus(focus.query_str(), &text);
            if !has_memory_focus {
                return (base_score + query_score, query_reasons, chunk);
            }

            let (memory_score, memory_reasons) =
                score_text_for_writing_focus(focus.memory_str(), &text);
            let (chapter_score, chapter_reason) =
                project_brain_chapter_proximity_score(focus, chunk);
            let combined =
                memory_score * 1.8 + query_score * 0.45 + base_score * 0.1 + chapter_score;
            let mut reasons = Vec::new();
            if let Some(reason) = chapter_reason {
                reasons.push(reason);
            }
            for reason in memory_reasons {
                if reasons.len() >= 5 {
                    break;
                }
                if !reasons.contains(&reason) {
                    reasons.push(reason);
                }
            }
            for reason in query_reasons {
                if reasons.len() >= 5 {
                    break;
                }
                if !reasons.contains(&reason) {
                    reasons.push(reason);
                }
            }
            (combined, reasons, chunk)
        })
        .collect::<Vec<_>>();
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    scored.into_iter().take(TOP_K).collect()
}

fn project_brain_chapter_proximity_score(
    focus: &ProjectBrainFocus,
    chunk: &Chunk,
) -> (f32, Option<String>) {
    let Some(active_chapter) = focus.active_chapter.as_deref() else {
        return (0.0, None);
    };
    let Some(active_number) = project_brain_chapter_number(active_chapter) else {
        return (0.0, None);
    };
    let Some(chunk_number) = project_brain_chapter_number(&chunk.chapter) else {
        return (0.0, None);
    };
    let distance = active_number.abs_diff(chunk_number);
    match distance {
        0 => (18.0, Some("chapter proximity current chapter".to_string())),
        1 => (12.0, Some("chapter proximity adjacent chapter".to_string())),
        2 | 3 => (6.0, Some("chapter proximity nearby chapter".to_string())),
        _ => (0.0, None),
    }
}

fn project_brain_chapter_number(chapter: &str) -> Option<u64> {
    let mut digits = String::new();
    let mut started = false;
    for ch in chapter.chars() {
        if ch.is_ascii_digit() {
            digits.push(ch);
            started = true;
        } else if started {
            break;
        }
    }
    digits.parse().ok()
}

fn project_brain_chunk_text(chunk: &Chunk) -> String {
    format!(
        "{}\n{}\n{}\n{}",
        chunk.chapter,
        chunk.keywords.join("\n"),
        chunk.topic.clone().unwrap_or_default(),
        chunk.text
    )
}

fn build_context(results: &[(f32, Vec<String>, &Chunk)]) -> String {
    if results.is_empty() {
        return "No relevant chunks found in the book.".to_string();
    }

    results
        .iter()
        .enumerate()
        .map(|(i, (score, reasons, chunk))| {
            format!(
                "[Chunk {} · {} · score {:.3}]\n{}\n{}",
                i + 1,
                chunk.chapter,
                score,
                format_text_chunk_relevance(reasons),
                chunk.text
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}
