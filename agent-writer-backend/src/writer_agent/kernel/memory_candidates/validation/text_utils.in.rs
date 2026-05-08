pub(crate) fn split_sentences(text: &str) -> Vec<String> {
    let mut sentences = Vec::new();
    let mut current = String::new();
    for ch in text.chars() {
        current.push(ch);
        if matches!(ch, '。' | '！' | '？' | '.' | '!' | '?' | '\n') {
            let trimmed = current.trim();
            if trimmed.chars().count() >= 6 {
                sentences.push(trimmed.to_string());
            }
            current.clear();
        }
    }
    let trimmed = current.trim();
    if trimmed.chars().count() >= 6 {
        sentences.push(trimmed.to_string());
    }
    sentences
}

fn extract_name_after(sentence: &str, cue: &str) -> Option<String> {
    let cue_byte = sentence.find(cue)?;
    let after = &sentence[cue_byte + cue.len()..];
    let name: String = after
        .chars()
        .skip_while(|c| c.is_whitespace() || matches!(c, '“' | '"' | '\'' | '：' | ':'))
        .take_while(|c| c.is_alphanumeric() || ('\u{4e00}'..='\u{9fff}').contains(c))
        .take(6)
        .collect();
    let count = name.chars().count();
    if (2..=6).contains(&count) {
        Some(name)
    } else {
        None
    }
}

fn should_keep_entity(name: &str, known: &[String], existing: &[CanonEntityOp]) -> bool {
    let name = name.trim();
    !name.is_empty()
        && !known.iter().any(|item| item == name)
        && !existing.iter().any(|item| item.name == name)
}

fn extract_related_entities(sentence: &str) -> Vec<String> {
    let mut entities = Vec::new();
    for marker in [
        "林墨",
        "张三",
        "玉佩",
        "长刀",
        "戒指",
        "密信",
        "钥匙",
        "令牌",
        "黑衣人",
        "旧门",
    ] {
        if sentence.contains(marker) {
            entities.push(marker.to_string());
        }
    }
    if entities.is_empty() {
        entities.push("unknown".to_string());
    }
    entities.truncate(3);
    entities
}

fn contains_promise_cue(sentence: &str) -> bool {
    [
        "还没",
        "尚未",
        "迟早",
        "总有一天",
        "秘密",
        "谜",
        "真相",
        "下落",
        "没有说出口",
        "没有告诉",
        "约定",
        "承诺",
        "发誓",
        "一定会",
        "等着",
        "留给",
        "交给",
        "带走",
        "失踪",
        "不见",
        "消失",
        "藏",
        "隐瞒",
    ]
    .iter()
    .any(|cue| sentence.contains(cue))
}

fn promise_kind_from_cues(sentence: &str) -> PromiseKind {
    let s = sentence;
    if s.contains("下落") || s.contains("不见") || s.contains("消失") || s.contains("带走")
    {
        PromiseKind::ObjectWhereabouts
    } else if s.contains("秘密") || s.contains("谜") || s.contains("真相") || s.contains("隐瞒")
    {
        PromiseKind::MysteryClue
    } else if s.contains("约定") || s.contains("承诺") || s.contains("发誓") || s.contains("等着")
    {
        PromiseKind::CharacterCommitment
    } else if s.contains("没有说出口") || s.contains("没有告诉") || s.contains("藏") {
        PromiseKind::EmotionalDebt
    } else {
        PromiseKind::PlotPromise
    }
}

fn promise_title(sentence: &str) -> String {
    for marker in [
        "玉佩", "密信", "钥匙", "令牌", "真相", "秘密", "下落", "戒指", "剑", "刀", "信物", "地图",
        "药", "毒",
    ] {
        if sentence.contains(marker) {
            return marker.to_string();
        }
    }
    sentence
        .chars()
        .filter(|c| !c.is_whitespace())
        .take(12)
        .collect()
}

pub(crate) fn sentence_snippet(sentence: &str, limit: usize) -> String {
    sentence
        .trim_matches(|c: char| c.is_whitespace())
        .chars()
        .take(limit)
        .collect()
}

#[derive(Debug, PartialEq)]
pub enum MemoryCandidateQuality {
    Acceptable,
    Vague {
        reason: String,
    },
    Duplicate {
        existing_name: String,
    },
    MergeableAttributes {
        existing_name: String,
        attributes: Vec<(String, String)>,
    },
    Conflict {
        existing_name: String,
        reason: String,
    },
}
