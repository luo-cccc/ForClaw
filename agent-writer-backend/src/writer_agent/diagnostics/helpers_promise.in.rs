#[derive(Default)]
struct PromiseMatch {
    is_match: bool,
    from: Option<usize>,
    to: Option<usize>,
}

fn match_promise(paragraph: &str, promise: &super::memory::PlotPromiseSummary) -> PromiseMatch {
    let terms = promise_terms(
        &promise.title,
        &promise.description,
        &promise.expected_payoff,
    );
    let mut score = 0usize;
    let mut first_span = None;

    for (index, term) in terms.iter().enumerate() {
        if let Some(byte_pos) = paragraph.find(term) {
            let from = byte_to_char_index(paragraph, byte_pos);
            let to = from + term.chars().count();
            first_span.get_or_insert((from, to));
            score += if index == 0 { 3 } else { 1 };
        }
    }

    let (from, to) = first_span.unwrap_or((0, paragraph.chars().count()));
    PromiseMatch {
        is_match: score >= 2,
        from: Some(from),
        to: Some(to),
    }
}

fn promise_terms(title: &str, description: &str, expected_payoff: &str) -> Vec<String> {
    let mut terms = Vec::new();
    push_term(&mut terms, title);
    for alias in promise_aliases(title) {
        push_term(&mut terms, alias);
    }

    for text in [description, expected_payoff] {
        for term in meaningful_terms(text) {
            push_term(&mut terms, &term);
        }
    }

    terms
}

fn push_term(terms: &mut Vec<String>, term: &str) {
    let normalized = term.trim();
    if normalized.chars().count() < 2 || is_stop_term(normalized) {
        return;
    }
    if !terms.iter().any(|existing| existing == normalized) {
        terms.push(normalized.to_string());
    }
}

fn promise_aliases(title: &str) -> Vec<&'static str> {
    let mut aliases = Vec::new();
    if title.contains("玉佩") {
        aliases.extend(["玉坠", "玉牌", "那枚玉", "那块玉"]);
    }
    if title.contains("密道") {
        aliases.extend(["暗道", "地道", "暗门"]);
    }
    if title.contains("钥匙") {
        aliases.extend(["钥匙串", "铜钥匙"]);
    }
    aliases
}

fn meaningful_terms(text: &str) -> Vec<String> {
    let mut terms = Vec::new();
    let chars: Vec<char> = text.chars().collect();
    for window in 2..=3 {
        if chars.len() < window {
            continue;
        }
        for slice in chars.windows(window) {
            let term: String = slice.iter().collect();
            if term.chars().all(is_term_char) && !is_stop_term(&term) {
                push_term(&mut terms, &term);
            }
        }
    }
    terms
}

fn is_term_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ('\u{4e00}'..='\u{9fff}').contains(&ch)
}

fn is_stop_term(term: &str) -> bool {
    const STOP_TERMS: &[&str] = &[
        "需要", "交代", "下落", "伏笔", "回收", "揭示", "说明", "后续", "预期", "Chapter", "章节",
        "当前", "引入", "拿走", "留下", "发现", "必须", "为何", "什么", "没有",
    ];
    STOP_TERMS.iter().any(|stop| term.contains(stop))
}

fn is_later_chapter(current: &str, introduced: &str) -> bool {
    match (chapter_number(current), chapter_number(introduced)) {
        (Some(current), Some(introduced)) => current > introduced,
        _ => false,
    }
}

fn is_stale_promise(current: &str, introduced: &str, expected_payoff: &str) -> bool {
    if let (Some(current_number), Some(payoff_number)) =
        (chapter_number(current), chapter_number(expected_payoff))
    {
        return current_number >= payoff_number;
    }

    match (chapter_number(current), chapter_number(introduced)) {
        (Some(current_number), Some(introduced_number)) => current_number - introduced_number >= 3,
        _ => false,
    }
}

fn promise_decision_operations(
    promise: &super::memory::PlotPromiseSummary,
    chapter_id: &str,
) -> Vec<WriterOperation> {
    vec![
        WriterOperation::PromiseResolve {
            promise_id: promise.id.to_string(),
            chapter: chapter_id.to_string(),
        },
        WriterOperation::PromiseDefer {
            promise_id: promise.id.to_string(),
            chapter: chapter_id.to_string(),
            expected_payoff: next_chapter_label(chapter_id),
        },
        WriterOperation::PromiseAbandon {
            promise_id: promise.id.to_string(),
            chapter: chapter_id.to_string(),
            reason: format!(
                "Author decided '{}' no longer needs payoff in the current story shape.",
                promise.title
            ),
        },
    ]
}

fn next_chapter_label(chapter_id: &str) -> String {
    chapter_number(chapter_id)
        .map(|number| format!("Chapter-{}", number + 1))
        .unwrap_or_else(|| "later chapter".to_string())
}

fn chapter_number(chapter: &str) -> Option<i64> {
    let mut numbers = Vec::new();
    let mut current = String::new();
    for ch in chapter.chars() {
        if ch.is_ascii_digit() {
            current.push(ch);
        } else if !current.is_empty() {
            numbers.push(current.parse::<i64>().ok()?);
            current.clear();
        }
    }
    if !current.is_empty() {
        numbers.push(current.parse::<i64>().ok()?);
    }
    numbers.last().copied()
}

impl Default for DiagnosticsEngine {
    fn default() -> Self {
        Self::new()
    }
}
