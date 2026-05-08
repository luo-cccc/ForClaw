fn extract_facts(evidence: &BeliefEvidence) -> Vec<FactBelief<'_>> {
    let mut facts = Vec::new();
    if evidence.source == BeliefSource::Canon {
        if let Some((subject, predicate)) = canon_subject_predicate(&evidence.reference) {
            if let Some(object) = value_after_equals(&evidence.snippet) {
                facts.push(FactBelief {
                    subject,
                    predicate,
                    object,
                    evidence,
                });
            }
        }
    }

    for predicate in ["来源", "身份", "下落", "位置"] {
        if let Some((subject, object)) = infer_chinese_fact(&evidence.snippet, predicate) {
            facts.push(FactBelief {
                subject,
                predicate: predicate.to_string(),
                object,
                evidence,
            });
        }
    }

    facts
}

fn canon_subject_predicate(reference: &str) -> Option<(String, String)> {
    let mut parts = reference.split(':');
    if parts.next()? != "canon" {
        return None;
    }
    let subject = parts.next()?.trim();
    let predicate = parts.next()?.trim();
    if subject.is_empty() || predicate.is_empty() || predicate == "summary" {
        return None;
    }
    Some((subject.to_string(), predicate.to_string()))
}

fn infer_chinese_fact(text: &str, predicate: &str) -> Option<(String, String)> {
    let predicate_pos = text.find(predicate)?;
    let subject = subject_before(text, predicate_pos)?;
    let after_predicate = &text[predicate_pos + predicate.len()..];
    let object = if let Some(value) = value_after_equals(after_predicate) {
        value
    } else if let Some(object) = object_after_marker(after_predicate, &["来自", "是", "为", "在"])
    {
        object
    } else {
        object_after_marker(text, &["来自", "是", "为", "在"])?
    };
    if subject == object || object.chars().count() < 1 {
        return None;
    }
    Some((subject, object))
}

fn subject_before(text: &str, byte_pos: usize) -> Option<String> {
    let prefix = &text[..byte_pos];
    let mut chars = Vec::new();
    for ch in prefix.chars().rev() {
        if is_boundary_char(ch) {
            break;
        }
        chars.push(ch);
        if chars.len() >= 16 {
            break;
        }
    }
    chars.reverse();
    let subject = trim_fact_edge_words(&chars.into_iter().collect::<String>());
    if subject.chars().count() >= 2 {
        Some(subject)
    } else {
        None
    }
}

fn value_after_equals(text: &str) -> Option<String> {
    let split_at = text
        .find('=')
        .or_else(|| text.find(':'))
        .or_else(|| text.find('：'))?;
    let value = trim_fact_edge_words(&take_until_boundary(&text[split_at + 1..]));
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

fn object_after_marker(text: &str, markers: &[&str]) -> Option<String> {
    let (marker_pos, marker) = markers
        .iter()
        .filter_map(|marker| text.find(marker).map(|pos| (pos, *marker)))
        .min_by_key(|(pos, _)| *pos)?;
    let value = trim_fact_edge_words(&take_until_boundary(&text[marker_pos + marker.len()..]));
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

fn take_until_boundary(text: &str) -> String {
    text.chars()
        .take_while(|ch| !is_boundary_char(*ch))
        .collect::<String>()
}

fn trim_fact_edge_words(text: &str) -> String {
    let mut value = text
        .trim_matches(|ch: char| {
            ch.is_whitespace()
                || matches!(
                    ch,
                    '"' | '\'' | '`' | '“' | '”' | '「' | '」' | '《' | '》' | '-' | '_' | '='
                )
        })
        .trim()
        .to_string();
    for prefix in ["已经", "已", "仍", "还", "会", "将", "就是", "其实", "原来"] {
        value = value.trim_start_matches(prefix).trim().to_string();
    }
    for suffix in ["已经揭示", "已揭示", "已经确认", "已确认"] {
        value = value.trim_end_matches(suffix).trim().to_string();
    }
    value
}

fn objects_conflict(left: &str, right: &str) -> bool {
    let left = normalize_fact_value(left);
    let right = normalize_fact_value(right);
    if left.is_empty() || right.is_empty() || left == right {
        return false;
    }
    if is_unknown_value(&left) != is_unknown_value(&right) {
        return true;
    }
    !left.contains(&right) && !right.contains(&left)
}

fn normalize_fact_value(value: &str) -> String {
    value
        .chars()
        .filter(|ch| !ch.is_whitespace() && !matches!(ch, '。' | '，' | ',' | '.' | ';' | '；'))
        .collect::<String>()
        .to_lowercase()
}

fn is_unknown_value(value: &str) -> bool {
    ["未知", "不明", "未揭示", "unknown", "unrevealed"]
        .iter()
        .any(|marker| value.contains(marker))
}

fn has_forbid_signal(text: &str) -> bool {
    text_contains_any(
        text,
        &[
            "不得",
            "不要",
            "不能",
            "禁止",
            "不许",
            "避免",
            "do not",
            "must not",
            "forbid",
            "forbidden",
        ],
    )
}

fn has_deferred_payoff_signal(text: &str) -> bool {
    text_contains_any(
        text,
        &[
            "expected payoff",
            "payoff",
            "later",
            "defer",
            "第",
            "后",
            "再",
            "延后",
            "回收",
            "兑现",
            "揭示",
        ],
    )
}

fn has_reveal_signal(text: &str) -> bool {
    text_contains_any(
        text,
        &[
            "已经",
            "已",
            "真相",
            "来自",
            "揭示",
            "揭露",
            "揭开",
            "说出",
            "确认",
            "兑现",
            "回收",
            "resolved",
            "revealed",
            "confirmed",
            "paid off",
        ],
    )
}

fn is_reveal_claim(evidence: &BeliefEvidence) -> bool {
    let text = evidence.snippet.as_str();
    !(!has_reveal_signal(text)
        || has_forbid_signal(text)
        || (evidence.source == BeliefSource::PromiseLedger && has_deferred_payoff_signal(text))
        || text_contains_any(
            text,
            &["未知", "不明", "未揭示", "仍是悬念", "保持悬念", "保留"],
        ))
}

fn guard_terms(text: &str) -> Vec<String> {
    let mut terms = Vec::new();
    for term in meaningful_terms(text) {
        if is_guard_stop_term(&term) {
            continue;
        }
        push_unique_term(&mut terms, &term);
    }

    let chars = text.chars().collect::<Vec<_>>();
    for pair in chars.windows(2) {
        let term = pair.iter().collect::<String>();
        if term.chars().all(is_term_char) && !is_guard_stop_term(&term) {
            push_unique_term(&mut terms, &term);
        }
    }
    terms
}

fn meaningful_terms(text: &str) -> Vec<String> {
    let mut terms = Vec::new();
    let mut current = String::new();
    for ch in text.chars() {
        if is_term_char(ch) {
            current.push(ch);
        } else {
            push_unique_term(&mut terms, &current);
            current.clear();
        }
    }
    push_unique_term(&mut terms, &current);
    terms
}

fn push_unique_term(terms: &mut Vec<String>, term: &str) {
    let term = term.trim();
    if term.chars().count() < 2 || is_guard_stop_term(term) {
        return;
    }
    if !terms.iter().any(|existing| existing == term) {
        terms.push(term.to_string());
    }
}

fn terms_overlap(terms: &[String], text: &str) -> bool {
    terms.iter().any(|term| text.contains(term))
}

fn is_guard_stop_term(term: &str) -> bool {
    const STOP_TERMS: &[&str] = &[
        "不得",
        "不要",
        "不能",
        "禁止",
        "不许",
        "避免",
        "提前",
        "泄露",
        "揭露",
        "揭示",
        "揭开",
        "来源",
        "身份",
        "真相",
        "expected",
        "payoff",
        "later",
        "defer",
        "must",
        "not",
        "forbidden",
    ];
    STOP_TERMS
        .iter()
        .any(|stop| term.eq_ignore_ascii_case(stop) || term.contains(stop))
}

fn is_term_char(ch: char) -> bool {
    ch.is_alphanumeric() || ('\u{4e00}'..='\u{9fff}').contains(&ch)
}

fn is_boundary_char(ch: char) -> bool {
    ch.is_whitespace()
        || matches!(
            ch,
            '。' | '，'
                | ','
                | '.'
                | ';'
                | '；'
                | ':'
                | '：'
                | '!'
                | '！'
                | '?'
                | '？'
                | '\n'
                | '\r'
        )
}

fn text_contains_any(text: &str, needles: &[&str]) -> bool {
    let lower = text.to_lowercase();
    needles
        .iter()
        .any(|needle| lower.contains(&needle.to_lowercase()))
}

fn conflict_confidence(evidence: &[BeliefEvidence]) -> f64 {
    if evidence.is_empty() {
        return 0.0;
    }
    let average = evidence.iter().map(|item| item.confidence).sum::<f64>() / evidence.len() as f64;
    let source_bonus = unique_sources(evidence).len().saturating_sub(2) as f64 * 0.03;
    clamp_confidence(average + source_bonus)
}

fn unique_sources(evidence: &[BeliefEvidence]) -> BTreeSet<BeliefSource> {
    evidence.iter().map(|item| item.source).collect()
}

fn stable_conflict_id(kind: BeliefConflictKind, evidence: &[BeliefEvidence]) -> String {
    let mut refs = evidence
        .iter()
        .map(|item| item.reference.as_str())
        .collect::<Vec<_>>();
    refs.sort_unstable();
    let kind = match kind {
        BeliefConflictKind::ForbiddenReveal => "forbidden_reveal",
        BeliefConflictKind::FactContradiction => "fact_contradiction",
    };
    format!("belief_conflict:{kind}:{}", refs.join("|"))
}

fn dedupe_conflicts(conflicts: Vec<BeliefConflictExplanation>) -> Vec<BeliefConflictExplanation> {
    let mut seen = BTreeSet::new();
    let mut deduped = Vec::new();
    for conflict in conflicts {
        if seen.insert(conflict.id.clone()) {
            deduped.push(conflict);
        }
    }
    deduped.sort_by(|left, right| {
        right
            .confidence
            .partial_cmp(&left.confidence)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.id.cmp(&right.id))
    });
    deduped
}

fn clamp_confidence(confidence: f64) -> f64 {
    confidence.clamp(0.0, 1.0)
}

fn snippet(text: &str, limit: usize) -> String {
    text.chars().take(limit).collect()
}

