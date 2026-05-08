fn add_scene_type_if(
    scene_types: &mut Vec<WritingSceneType>,
    text: &str,
    scene_type: WritingSceneType,
    cues: &[&str],
) {
    if cues.iter().any(|cue| text.contains(cue)) && !scene_types.contains(&scene_type) {
        scene_types.push(scene_type);
    }
}

fn relevance_terms(text: &str) -> Vec<String> {
    let mut terms = Vec::new();
    let mut seen = HashSet::new();
    for marker in [
        "寒玉戒指",
        "寒影刀",
        "黑衣人",
        "北境林墨",
        "南境林墨",
        "玉佩",
        "密信",
        "旧门钥匙",
        "钥匙",
        "令牌",
        "真相",
        "秘密",
        "下落",
        "来源",
        "林墨",
        "张三",
        "北境",
        "南境",
        "宗门",
        "朝堂",
        "旧门",
        "戒指",
        "长剑",
        "信任",
        "怀疑",
        "关系",
        "承诺",
        "誓言",
        "冲突",
        "危机",
    ] {
        if text.contains(marker) && seen.insert(marker.to_string()) {
            terms.push(marker.to_string());
        }
    }

    for keyword in extract_keywords(text) {
        push_relevance_term(&mut terms, &mut seen, &keyword);
    }

    add_phrase_relevance_terms(&mut terms, &mut seen, text);

    let mut current = String::new();
    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() || ('\u{4e00}'..='\u{9fff}').contains(&ch) {
            current.push(ch);
        } else {
            push_relevance_term(&mut terms, &mut seen, &current);
            current.clear();
        }
    }
    push_relevance_term(&mut terms, &mut seen, &current);
    terms
}

fn add_phrase_relevance_terms(terms: &mut Vec<String>, seen: &mut HashSet<String>, text: &str) {
    let mut phrase_text = text.to_string();
    for boundary in RELEVANCE_PHRASE_BOUNDARIES {
        phrase_text = phrase_text.replace(boundary, "\n");
    }
    for part in phrase_text.split(|ch| {
        matches!(
            ch,
            '\n' | '\r' | '。' | '；' | ';' | '.' | '，' | ',' | '、' | ':' | '：' | '？' | '?'
        )
    }) {
        push_relevance_term(terms, seen, part);
    }
}

const RELEVANCE_PHRASE_BOUNDARIES: &[&str] = &[
    "必须", "需要", "继续", "追查", "查清", "寻找", "找到", "确认", "揭开", "揭露", "发现", "回收",
    "收束", "指向", "围绕", "藏进", "带走", "带往", "不要", "不得", "禁止", "避免", "不能", "别再",
    "别让", "之间", "以及", "或者", "并且", "下落", "来源", "真相", "和", "与", "或", "被", "把",
    "将", "让", "以", "并", "但", "而",
];

fn is_blocked_by_negative_terms(term: &str, negative_terms: &[String]) -> bool {
    negative_terms.iter().any(|negative| {
        negative == term
            || (term.chars().count() >= 2 && negative.contains(term))
            || (negative.chars().count() >= 2 && term.contains(negative))
    })
}

fn negative_relevance_terms(text: &str) -> Vec<String> {
    let mut terms = Vec::new();
    let mut seen = HashSet::new();
    for segment in text.split(['\n', '。', '；', ';', '.']) {
        for cue in NEGATIVE_CUES {
            if let Some(cue_start) = segment.find(cue) {
                let cue_tail = &segment[cue_start + cue.len()..];
                for term in negative_phrase_terms(cue_tail) {
                    if seen.insert(term.clone()) {
                        terms.push(term);
                    }
                }
            }
        }
    }
    terms
}

const NEGATIVE_CUES: &[&str] = &["不要", "不得", "禁止", "避免", "不能", "别再", "别让", "别"];
const NEGATIVE_BOUNDARIES: &[&str] = &[
    "稀释", "干扰", "掩盖", "拖慢", "分散", "偏离", "覆盖", "盖过", "取代", "代替", "替代", "抢走",
];

fn negative_phrase_terms(text: &str) -> Vec<String> {
    let before_boundary = NEGATIVE_BOUNDARIES
        .iter()
        .filter_map(|boundary| text.find(boundary))
        .min()
        .map(|idx| &text[..idx])
        .unwrap_or(text);
    before_boundary
        .split(['或', '和', '、', '，', ',', '/', '／'])
        .filter_map(|part| {
            let term = strip_negative_phrase_filler(part);
            let count = term.chars().count();
            if (2..=10).contains(&count) && !is_relevance_stopword(term) {
                Some(term.to_string())
            } else {
                None
            }
        })
        .collect()
}

fn strip_negative_phrase_filler(raw: &str) -> &str {
    let mut text = raw.trim();
    loop {
        let trimmed = text
            .trim_start_matches('被')
            .trim_start_matches('把')
            .trim_start_matches('将')
            .trim_start_matches('让')
            .trim_start_matches('用')
            .trim_start_matches('以')
            .trim_start_matches("继续")
            .trim_start_matches("只是")
            .trim_start_matches('只')
            .trim_start_matches('再')
            .trim();
        if trimmed == text {
            return trimmed;
        }
        text = trimmed;
    }
}

fn push_relevance_term(terms: &mut Vec<String>, seen: &mut HashSet<String>, raw: &str) {
    let term = raw.trim().trim_end_matches('的');
    let count = term.chars().count();
    if !(2..=10).contains(&count) || is_relevance_stopword(term) {
        return;
    }
    if seen.insert(term.to_string()) {
        terms.push(term.to_string());
    }
}

fn is_relevance_stopword(term: &str) -> bool {
    let normalized = term.trim().to_ascii_lowercase();
    if normalized.starts_with("chapter-")
        || normalized.starts_with("chapter_")
        || normalized.starts_with("rev-")
        || normalized.starts_with("rev_")
    {
        return true;
    }
    let stopwords = [
        "章节", "本章", "任务", "目标", "当前", "需要", "继续", "处理", "保持", "推进", "不要",
        "不得", "禁止", "避免", "不能", "必须", "追查", "确认", "后续", "解释", "结果", "摘要",
        "状态", "变化", "新的", "明确", "作者", "哪里", "什么", "accepted", "active", "chapter",
        "mission", "result", "feedback", "decision", "eval", "rev",
    ];
    stopwords
        .iter()
        .any(|stopword| term == *stopword || normalized == *stopword)
}
