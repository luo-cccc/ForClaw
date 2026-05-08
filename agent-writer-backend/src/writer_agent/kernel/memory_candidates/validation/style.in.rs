pub fn validate_style_preference(key: &str, value: &str) -> MemoryCandidateQuality {
    let key = key.trim();
    if key.chars().count() < 3 {
        return MemoryCandidateQuality::Vague {
            reason: "style key too short (min 3 chars)".to_string(),
        };
    }
    let value = value.trim();
    let slot = style_preference_slot(key, value);
    if is_vague_style_key(key) && slot.is_none() && !is_feedback_style_key(key) {
        return MemoryCandidateQuality::Vague {
            reason: format!("style key '{}' is too generic", key),
        };
    }

    let value_chars = value.chars().count();
    if value_chars < 6 {
        return MemoryCandidateQuality::Vague {
            reason: format!("style preference too short ({} chars, min 6)", value_chars),
        };
    }
    if is_vague_style_value(value) {
        return MemoryCandidateQuality::Vague {
            reason: format!("style preference '{}' is too generic", value),
        };
    }

    MemoryCandidateQuality::Acceptable
}

pub fn style_preference_taxonomy_slot(key: &str, value: &str) -> Option<String> {
    style_preference_slot(key.trim(), value.trim()).map(|slot| slot.label())
}

pub fn validate_style_preference_with_memory(
    key: &str,
    value: &str,
    memory: &WriterMemory,
) -> MemoryCandidateQuality {
    let quality = validate_style_preference(key, value);
    if quality != MemoryCandidateQuality::Acceptable {
        return quality;
    }

    let key = key.trim();
    let value = value.trim();
    let existing_preferences = memory.list_style_preferences(200).unwrap_or_default();
    if let Some(existing) = existing_preferences
        .iter()
        .find(|preference| preference.key.trim().eq_ignore_ascii_case(key))
    {
        return classify_existing_style_preference(existing, value);
    }

    let Some(candidate_slot) = comparable_style_preference_slot(key, value) else {
        return MemoryCandidateQuality::Acceptable;
    };
    if let Some(existing) = existing_preferences.iter().find(|preference| {
        comparable_style_preference_slot(&preference.key, &preference.value)
            .is_some_and(|existing_slot| existing_slot == candidate_slot)
    }) {
        return classify_existing_style_taxonomy_preference(existing, value, candidate_slot);
    }

    MemoryCandidateQuality::Acceptable
}

fn classify_existing_style_preference(
    existing: &StylePreferenceSummary,
    value: &str,
) -> MemoryCandidateQuality {
    if existing.value.trim() == value {
        MemoryCandidateQuality::Duplicate {
            existing_name: existing.key.clone(),
        }
    } else if style_preference_polarity(&existing.value) == style_preference_polarity(value) {
        MemoryCandidateQuality::Acceptable
    } else {
        MemoryCandidateQuality::Conflict {
            existing_name: existing.key.clone(),
            reason: format!(
                "style preference '{}' conflicts: existing={}, candidate={}",
                existing.key, existing.value, value
            ),
        }
    }
}

fn classify_existing_style_taxonomy_preference(
    existing: &StylePreferenceSummary,
    value: &str,
    slot: StylePreferenceSlot,
) -> MemoryCandidateQuality {
    if existing.value.trim() == value {
        MemoryCandidateQuality::Duplicate {
            existing_name: existing.key.clone(),
        }
    } else if style_preference_polarity(&existing.value) == style_preference_polarity(value) {
        MemoryCandidateQuality::Acceptable
    } else {
        MemoryCandidateQuality::Conflict {
            existing_name: existing.key.clone(),
            reason: format!(
                "style taxonomy slot '{}' already has a preference: existing={}, candidate={}",
                slot.label(),
                existing.value,
                value
            ),
        }
    }
}

pub fn style_preference_memory_key(key: &str, value: &str) -> String {
    comparable_style_preference_slot(key, value)
        .map(|slot| format!("style:{}", slot.label()))
        .unwrap_or_else(|| key.trim().to_string())
}

fn is_vague_style_key(key: &str) -> bool {
    let key = key.trim().to_ascii_lowercase();
    matches!(
        key.as_str(),
        "style" | "tone" | "voice" | "writing" | "preference" | "good" | "bad" | "风格" | "语气"
    )
}

fn is_feedback_style_key(key: &str) -> bool {
    let key = key.trim().to_ascii_lowercase();
    key.starts_with("accepted_")
        || key.starts_with("ignored_")
        || key.starts_with("rejected_")
        || key.starts_with("memory_extract:")
}

fn is_vague_style_value(value: &str) -> bool {
    let normalized = value.trim().to_ascii_lowercase();
    if matches!(
        normalized.as_str(),
        "good" | "bad" | "better" | "nice" | "ok" | "accepted" | "rejected" | "not my style"
    ) {
        return true;
    }
    matches!(
        value.trim(),
        "好" | "不好" | "更好" | "不错" | "可以" | "喜欢" | "不喜欢" | "有感觉" | "没感觉"
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StylePreferencePolarity {
    Prefer,
    Avoid,
    Neutral,
}

fn style_preference_polarity(value: &str) -> StylePreferencePolarity {
    let normalized = value.trim().to_ascii_lowercase();
    if contains_any(
        &normalized,
        &[
            "避免", "不要", "少", "减少", "降低", "拒绝", "别", "不再", "别再", "少用", "克制",
            "avoid", "less", "reduce", "reject", "without", "no ",
        ],
    ) {
        return StylePreferencePolarity::Avoid;
    }
    if contains_any(
        &normalized,
        &[
            "偏",
            "优先",
            "保留",
            "增加",
            "更多",
            "多",
            "强化",
            "强调",
            "喜欢",
            "倾向",
            "prefer",
            "more",
            "keep",
            "increase",
            "emphasize",
        ],
    ) {
        return StylePreferencePolarity::Prefer;
    }
    StylePreferencePolarity::Neutral
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct StylePreferenceSlot {
    dimension: &'static str,
    axis: &'static str,
    comparable: bool,
}

impl StylePreferenceSlot {
    fn label(self) -> String {
        format!("{}.{}", self.dimension, self.axis)
    }
}

fn comparable_style_preference_slot(key: &str, value: &str) -> Option<StylePreferenceSlot> {
    style_preference_slot(key, value).filter(|slot| slot.comparable)
}

fn style_preference_slot(key: &str, value: &str) -> Option<StylePreferenceSlot> {
    let key_lower = key.trim().to_ascii_lowercase();
    if is_feedback_style_key(&key_lower) {
        return Some(StylePreferenceSlot {
            dimension: "feedback",
            axis: "proposal",
            comparable: false,
        });
    }

    let value_lower = value.trim().to_ascii_lowercase();
    let combined = format!("{} {}", key_lower, value_lower);

    if contains_any(&combined, &["dialogue", "dialog", "对话", "台词", "对白"])
        && contains_any(
            &combined,
            &[
                "subtext",
                "潜台词",
                "留白",
                "解释情绪",
                "真实情绪",
                "情绪解释",
                "direct emotion",
                "explain emotion",
            ],
        )
    {
        return Some(StylePreferenceSlot {
            dimension: "dialogue",
            axis: "subtext",
            comparable: true,
        });
    }

    if contains_any(
        &combined,
        &[
            "sentence_length",
            "sentence length",
            "short sentence",
            "long sentence",
            "短句",
            "长句",
            "句长",
            "断句",
        ],
    ) {
        return Some(StylePreferenceSlot {
            dimension: "prose",
            axis: "sentence_length",
            comparable: true,
        });
    }

    if contains_any(
        &combined,
        &[
            "exposition",
            "info_dump",
            "infodump",
            "information density",
            "说明",
            "信息量",
            "背景交代",
            "解释设定",
        ],
    ) {
        return Some(StylePreferenceSlot {
            dimension: "exposition",
            axis: "density",
            comparable: true,
        });
    }

    if contains_any(
        &combined,
        &[
            "description",
            "sensory",
            "image",
            "描写",
            "感官",
            "气味",
            "触感",
            "画面",
        ],
    ) {
        return Some(StylePreferenceSlot {
            dimension: "description",
            axis: "sensory_detail",
            comparable: true,
        });
    }

    if contains_any(
        &combined,
        &[
            "pov",
            "point_of_view",
            "point of view",
            "视角",
            "内心",
            "旁白",
        ],
    ) {
        return Some(StylePreferenceSlot {
            dimension: "pov",
            axis: "distance",
            comparable: true,
        });
    }

    if contains_any(
        &combined,
        &["action", "combat", "fight", "动作", "打斗", "追逐", "交锋"],
    ) {
        return Some(StylePreferenceSlot {
            dimension: "action",
            axis: "clarity",
            comparable: true,
        });
    }

    if contains_any(
        &combined,
        &[
            "chapter_hook",
            "hook",
            "cliffhanger",
            "悬念",
            "钩子",
            "章尾",
            "转折",
        ],
    ) {
        return Some(StylePreferenceSlot {
            dimension: "structure",
            axis: "hook",
            comparable: true,
        });
    }

    if contains_any(
        &combined,
        &[
            "tone", "voice", "语气", "风格", "冷峻", "克制", "轻松", "幽默", "吐槽",
        ],
    ) {
        return Some(StylePreferenceSlot {
            dimension: "tone",
            axis: "voice",
            comparable: true,
        });
    }

    None
}

fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| haystack.contains(needle))
}
