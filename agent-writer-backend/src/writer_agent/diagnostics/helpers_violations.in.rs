fn looks_like_flashback_or_nonliteral(text: &str) -> bool {
    text_contains_any(
        text,
        &[
            "回忆", "想起", "梦", "梦里", "幻觉", "幻象", "尸体", "遗体", "墓", "画像", "信里",
            "传闻", "灵魂",
        ],
    )
}

fn entity_context_after(text: &str, entity: &str, max_chars: usize) -> String {
    let Some(pos) = text.find(entity) else {
        return String::new();
    };
    text[pos + entity.len()..].chars().take(max_chars).collect()
}

fn value_contains_any(value: &str, needles: &[&str]) -> bool {
    let lower = value.to_lowercase();
    needles
        .iter()
        .any(|needle| lower.contains(&needle.to_lowercase()))
}

fn text_contains_any(value: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| value.contains(needle))
}

struct StoryContractIssue {
    severity: DiagnosticSeverity,
    message: String,
    from: usize,
    to: usize,
    reference: String,
    snippet: String,
    fix_suggestion: String,
    annotation: String,
    annotation_severity: AnnotationSeverity,
}

struct ChapterMissionIssue {
    severity: DiagnosticSeverity,
    message: String,
    from: usize,
    to: usize,
    reference: String,
    snippet: String,
    fix_suggestion: String,
    annotation: String,
    annotation_severity: AnnotationSeverity,
}

fn detect_story_contract_violations(
    paragraph: &str,
    paragraph_offset: usize,
    contract: &StoryContractSummary,
) -> Vec<StoryContractIssue> {
    let mut issues = Vec::new();

    if let Some(issue) = detect_structural_boundary_violation(
        paragraph,
        paragraph_offset,
        &contract.structural_boundary,
    ) {
        issues.push(issue);
    }

    issues
}

fn detect_chapter_mission_violations(
    paragraph: &str,
    paragraph_offset: usize,
    mission: &ChapterMissionSummary,
) -> Vec<ChapterMissionIssue> {
    let mut issues = Vec::new();

    if let Some(issue) = detect_mission_must_not_violation(
        paragraph,
        paragraph_offset,
        &mission.chapter_title,
        &mission.must_not,
    ) {
        issues.push(issue);
    }

    issues
}

fn detect_mission_must_not_violation(
    paragraph: &str,
    paragraph_offset: usize,
    chapter_title: &str,
    must_not: &str,
) -> Option<ChapterMissionIssue> {
    let must_not = must_not.trim();
    if must_not.is_empty() {
        return None;
    }

    let terms = mission_guard_terms(must_not);
    let matched = terms
        .iter()
        .find_map(|term| match_mission_guard_term(paragraph, term))?;
    if looks_negated_or_deferred_before(paragraph, matched.0) {
        return None;
    }
    let from = paragraph_offset + matched.0;
    let to = paragraph_offset + matched.1.max(matched.0 + 1);

    Some(ChapterMissionIssue {
        severity: DiagnosticSeverity::Error,
        message: format!(
            "章节任务违例: {} 的禁止事项被触碰「{}」",
            chapter_title, must_not
        ),
        from,
        to,
        reference: format!("{}:must_not", chapter_title),
        snippet: must_not.to_string(),
        fix_suggestion: "保留悬念、换成误导或旁证，或先更新本章任务再继续。".to_string(),
        annotation: format!("疑似违反本章禁止事项：{}", must_not),
        annotation_severity: AnnotationSeverity::Error,
    })
}

fn mission_guard_terms(must_not: &str) -> Vec<String> {
    let mut terms = Vec::new();
    for term in meaningful_terms(must_not) {
        if is_mission_guard_stop_term(&term) {
            continue;
        }
        push_term(&mut terms, &term);
    }

    let chars = must_not.chars().collect::<Vec<_>>();
    for pair in chars.windows(2) {
        let term = pair.iter().collect::<String>();
        if term.chars().all(is_term_char)
            && !is_mission_guard_stop_term(&term)
            && !is_stop_term(&term)
        {
            push_term(&mut terms, &term);
        }
    }

    terms
}

fn match_mission_guard_term(paragraph: &str, term: &str) -> Option<(usize, usize)> {
    if let Some(byte_pos) = paragraph.find(term) {
        let from = byte_to_char_index(paragraph, byte_pos);
        return Some((from, from + term.chars().count()));
    }

    if term.chars().count() == 2 {
        let chars = term.chars().collect::<Vec<_>>();
        if chars.len() == 2 && paragraph.contains(chars[0]) && paragraph.contains(chars[1]) {
            let first = paragraph
                .find(chars[0])
                .map(|pos| byte_to_char_index(paragraph, pos))?;
            let second = paragraph
                .find(chars[1])
                .map(|pos| byte_to_char_index(paragraph, pos))?;
            let from = first.min(second);
            let to = first.max(second) + 1;
            return Some((from, to));
        }
    }

    None
}

fn is_mission_guard_stop_term(term: &str) -> bool {
    const STOP_TERMS: &[&str] = &[
        "不得", "不要", "不能", "禁止", "不许", "避免", "提前", "泄露", "揭露", "揭示", "揭开",
        "本章", "事项",
    ];
    STOP_TERMS.iter().any(|stop| term.contains(stop))
}

fn detect_structural_boundary_violation(
    paragraph: &str,
    paragraph_offset: usize,
    boundary: &str,
) -> Option<StoryContractIssue> {
    let boundary = boundary.trim();
    if boundary.is_empty() || !text_contains_any(boundary, CONTRACT_FORBID_CUES) {
        return None;
    }
    if !text_contains_any(paragraph, CONTRACT_REVEAL_CUES) {
        return None;
    }

    let terms = contract_boundary_terms(boundary);
    let matched = terms
        .iter()
        .find_map(|term| match_contract_term(paragraph, term))?;
    if looks_negated_or_deferred_before(paragraph, matched.0) {
        return None;
    }
    let from = paragraph_offset + matched.0;
    let to = paragraph_offset + matched.1.max(matched.0 + 1);

    Some(StoryContractIssue {
        severity: DiagnosticSeverity::Error,
        message: format!("书级合同违例: 当前段落疑似触碰禁区「{}」", boundary),
        from,
        to,
        reference: "structural_boundary".to_string(),
        snippet: boundary.to_string(),
        fix_suggestion: "延后揭示、改成误导线索，或先更新书级合同后再写正文。".to_string(),
        annotation: format!("疑似违反书级禁区：{}", boundary),
        annotation_severity: AnnotationSeverity::Error,
    })
}

const CONTRACT_FORBID_CUES: &[&str] = &["不得", "不要", "不能", "禁止", "不许", "避免", "禁"];
const CONTRACT_REVEAL_CUES: &[&str] = &[
    "真相", "来源", "身份", "秘密", "揭开", "揭露", "揭示", "说出", "坦白", "承认", "原来", "其实",
    "就是", "来自",
];

fn contract_boundary_terms(boundary: &str) -> Vec<String> {
    let mut terms = Vec::new();
    for term in meaningful_terms(boundary) {
        if is_contract_boundary_stop_term(&term) {
            continue;
        }
        push_term(&mut terms, &term);
    }

    let chars = boundary.chars().collect::<Vec<_>>();
    for pair in chars.windows(2) {
        let term = pair.iter().collect::<String>();
        if term.chars().all(is_term_char)
            && !is_contract_boundary_stop_term(&term)
            && !is_stop_term(&term)
        {
            push_term(&mut terms, &term);
        }
    }

    terms
}

fn match_contract_term(paragraph: &str, term: &str) -> Option<(usize, usize)> {
    if let Some(byte_pos) = paragraph.find(term) {
        let from = byte_to_char_index(paragraph, byte_pos);
        return Some((from, from + term.chars().count()));
    }

    if term.chars().count() == 2 {
        let chars = term.chars().collect::<Vec<_>>();
        if chars.len() == 2 && paragraph.contains(chars[0]) && paragraph.contains(chars[1]) {
            let first = paragraph
                .find(chars[0])
                .map(|pos| byte_to_char_index(paragraph, pos))?;
            let second = paragraph
                .find(chars[1])
                .map(|pos| byte_to_char_index(paragraph, pos))?;
            let from = first.min(second);
            let to = first.max(second) + 1;
            return Some((from, to));
        }
    }

    None
}

fn is_contract_boundary_stop_term(term: &str) -> bool {
    const STOP_TERMS: &[&str] = &[
        "不得", "不要", "不能", "禁止", "不许", "避免", "提前", "泄露", "揭露", "揭示", "揭开",
        "真相", "来源", "身份", "秘密",
    ];
    STOP_TERMS.iter().any(|stop| term.contains(stop))
}

fn looks_negated_or_deferred_before(text: &str, match_from: usize) -> bool {
    let chars = text.chars().collect::<Vec<_>>();
    let start = match_from.saturating_sub(8);
    let context = chars[start..match_from.min(chars.len())]
        .iter()
        .collect::<String>();
    text_contains_any(
        &context,
        &[
            "没有",
            "并未",
            "未曾",
            "尚未",
            "还没",
            "不会",
            "不能",
            "不该",
            "不肯",
            "拒绝",
            "暂不",
            "避免",
            "没有真正",
            "并没有",
        ],
    )
}
