fn extract_entities(paragraph: &str, memory: &WriterMemory) -> Vec<String> {
    let mut entities = Vec::new();
    if let Ok(known_names) = memory.get_canon_entity_names() {
        for name in known_names {
            if !name.trim().is_empty() && paragraph.contains(&name) && !entities.contains(&name) {
                entities.push(name);
            }
        }
    }

    // Find 2-3 char sequences that look like names (Chinese names are typically 2-3 chars)
    let chars: Vec<char> = paragraph.chars().collect();
    let mut i = 0;
    while i + 1 < chars.len() {
        // Look for patterns like "XX拔出" or "XX的"
        if i + 2 < chars.len() {
            let slice: String = chars[i..i + 2].iter().collect();
            // Check if followed by action verb or particle
            if i + 2 < chars.len() {
                let next = chars[i + 2];
                if matches!(next, '拔' | '握' | '拿' | '举' | '的' | '说' | '走' | '看')
                    && !entities.contains(&slice) {
                        entities.push(slice);
                    }
            }
        }
        i += 1;
    }
    entities
}

fn byte_to_char_index(text: &str, byte_index: usize) -> usize {
    text[..byte_index.min(text.len())].chars().count()
}

/// Detect a specific attribute value mentioned near an entity.
fn detect_attribute_value(paragraph: &str, entity: &str, attribute: &str) -> Option<String> {
    match attribute {
        "weapon" => {
            let weapons = [
                "匕首",
                "长剑",
                "短剑",
                "寒影刀",
                "剑",
                "刀",
                "枪",
                "弓",
                "棍",
                "鞭",
                "斧",
                "戟",
                "锤",
            ];
            if let Some(pos) = paragraph.find(entity) {
                let after: String = paragraph[pos + entity.len()..].chars().take(30).collect();
                for w in &weapons {
                    if after.contains(w) {
                        return Some(w.to_string());
                    }
                }
            }
            None
        }
        "location" => {
            let locations = ["破庙", "宫殿", "山洞", "客栈", "城", "山林", "河边"];
            for loc in &locations {
                if paragraph.contains(loc) {
                    return Some(loc.to_string());
                }
            }
            None
        }
        _ => None,
    }
}

fn attribute_values_compatible(attribute: &str, mentioned_value: &str, canon_value: &str) -> bool {
    let mentioned = mentioned_value.trim();
    let canon = canon_value.trim();
    if mentioned == canon {
        return true;
    }

    match attribute {
        "weapon" => {
            canon.contains(mentioned)
                || mentioned.contains(canon)
                || weapon_family(canon) == weapon_family(mentioned)
        }
        _ => false,
    }
}

fn canon_conflict_operations(
    chapter_id: &str,
    from: usize,
    to: usize,
    canon_value: &str,
    mentioned_value: &str,
    entity: &str,
    attribute: &str,
) -> Vec<WriterOperation> {
    let replacement = canon_value.trim();
    if replacement.is_empty() {
        return vec![WriterOperation::TextAnnotate {
            chapter: chapter_id.to_string(),
            from,
            to,
            message: format!(
                "{} 的 {} 与 canon 冲突：文中 {}",
                entity, attribute, mentioned_value
            ),
            severity: AnnotationSeverity::Error,
        }];
    }

    vec![
        WriterOperation::TextReplace {
            chapter: chapter_id.to_string(),
            from,
            to,
            text: replacement.to_string(),
            revision: "missing".to_string(),
        },
        WriterOperation::CanonUpdateAttribute {
            entity: entity.to_string(),
            attribute: attribute.to_string(),
            value: mentioned_value.to_string(),
            confidence: 0.82,
        },
    ]
}

fn weapon_family(value: &str) -> &str {
    for family in ["剑", "刀", "枪", "弓", "匕首", "棍", "鞭", "斧", "戟", "锤"] {
        if value.contains(family) {
            return family;
        }
    }
    value
}

struct TimelineIssue {
    message: String,
    from: usize,
    to: usize,
    fix_suggestion: Option<String>,
}

fn detect_timeline_issue(
    paragraph: &str,
    paragraph_offset: usize,
    entity_mention: &str,
    canonical_entity: &str,
    attribute: &str,
    canon_value: &str,
) -> Option<TimelineIssue> {
    if !is_state_attribute(attribute) || !paragraph.contains(entity_mention) {
        return None;
    }
    if looks_like_flashback_or_nonliteral(paragraph) {
        return None;
    }

    let span = entity_context_after(paragraph, entity_mention, 36);
    let entity_pos = paragraph.find(entity_mention).unwrap_or(0);
    let from = paragraph_offset + byte_to_char_index(paragraph, entity_pos);
    let to = from + entity_mention.chars().count();

    if value_contains_any(canon_value, DEAD_STATE_CUES)
        && text_contains_any(&span, LIVING_ACTION_CUES)
    {
        return Some(TimelineIssue {
            message: format!(
                "时间线疑点: canon记录{}已死亡，但当前段落让其执行行动或说话",
                canonical_entity
            ),
            from,
            to,
            fix_suggestion: Some("确认这是回忆、幻象、尸体描写，还是需要调整出场人物。".into()),
        });
    }

    if value_contains_any(canon_value, UNAVAILABLE_STATE_CUES)
        && text_contains_any(&span, PRESENT_ACTION_CUES)
    {
        return Some(TimelineIssue {
            message: format!(
                "时间线疑点: canon记录{}当前不在场，但当前段落让其出现在现场",
                canonical_entity
            ),
            from,
            to,
            fix_suggestion: Some("补一笔其返回原因，或替换为当前在场角色。".into()),
        });
    }

    None
}

fn is_state_attribute(attribute: &str) -> bool {
    matches!(
        attribute,
        "status" | "state" | "life_state" | "current_state" | "availability"
    )
}

const DEAD_STATE_CUES: &[&str] = &["死亡", "已死", "死了", "阵亡", "dead", "deceased"];
const UNAVAILABLE_STATE_CUES: &[&str] = &["离开", "离队", "远在", "不在", "失踪", "被关", "囚禁"];
const LIVING_ACTION_CUES: &[&str] = &[
    "说道", "说", "笑", "哭", "走", "跑", "站", "看", "呼吸", "握", "拔", "推门", "点头", "摇头",
    "伸手", "醒来",
];
const PRESENT_ACTION_CUES: &[&str] = &[
    "走进",
    "推门而入",
    "出现",
    "站在",
    "回到",
    "坐在",
    "就在",
    "等在",
    "开口",
    "说道",
];
