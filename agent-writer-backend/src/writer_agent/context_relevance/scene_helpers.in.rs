fn add_scene_type_scores(
    score: &mut RelevanceScore,
    focus_scene_types: &[WritingSceneType],
    candidate_scene_types: &[WritingSceneType],
    points: i32,
    prefix: &str,
) {
    let mut matching_scene_types = focus_scene_types
        .iter()
        .copied()
        .filter(|scene_type| candidate_scene_types.contains(scene_type))
        .collect::<Vec<_>>();
    matching_scene_types.sort_by_key(|scene_type| scene_type_relevance_priority(*scene_type));
    for scene_type in matching_scene_types.into_iter().take(2) {
        score.add(
            points,
            format!("{} scene type {}", prefix, scene_type.label()),
        );
    }
}

fn scene_type_relevance_priority(scene_type: WritingSceneType) -> u8 {
    match scene_type {
        WritingSceneType::SetupPayoff => 0,
        WritingSceneType::Reveal => 1,
        WritingSceneType::ConflictEscalation => 2,
        WritingSceneType::EmotionalBeat => 3,
        WritingSceneType::Dialogue => 4,
        WritingSceneType::Action => 5,
        WritingSceneType::Exposition => 6,
        WritingSceneType::Description => 7,
        WritingSceneType::Transition => 8,
    }
}

fn promise_kind_scene_types(kind: &str) -> Vec<WritingSceneType> {
    match kind {
        "mystery_clue" => vec![WritingSceneType::Reveal, WritingSceneType::SetupPayoff],
        "object_whereabouts" => vec![WritingSceneType::SetupPayoff],
        "emotional_debt" => vec![WritingSceneType::EmotionalBeat],
        "character_commitment" => vec![WritingSceneType::Dialogue, WritingSceneType::SetupPayoff],
        "relationship_tension" => {
            vec![WritingSceneType::Dialogue, WritingSceneType::EmotionalBeat]
        }
        _ => Vec::new(),
    }
}

fn relevance_reason_text(reasons: &[String]) -> String {
    if reasons.is_empty() {
        "ledger priority".to_string()
    } else {
        reasons
            .iter()
            .take(5)
            .cloned()
            .collect::<Vec<_>>()
            .join("; ")
    }
}

fn canon_attributes_text(entity: &CanonEntitySummary) -> String {
    entity
        .attributes
        .as_object()
        .map(|map| {
            map.iter()
                .map(|(key, value)| format!("{}={}", key, value))
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default()
}

fn canon_entity_text(entity: &CanonEntitySummary) -> String {
    format!(
        "{}\n{}\n{}\n{}",
        entity.name,
        entity.kind,
        entity.summary,
        canon_attributes_text(entity)
    )
}

fn promise_text(promise: &PlotPromiseSummary) -> String {
    format!(
        "{}\n{}\n{}\n{}\n{}\n{}",
        promise.title,
        promise.kind,
        promise.description,
        promise.introduced_chapter,
        promise.last_seen_chapter,
        promise.expected_payoff
    )
}

fn infer_scene_types(text: &str) -> Vec<WritingSceneType> {
    let mut scene_types = Vec::new();
    add_scene_type_if(
        &mut scene_types,
        text,
        WritingSceneType::Dialogue,
        &[
            "「", "」", "\"", "说", "问", "回答", "低声", "低语", "喃喃", "喊",
        ],
    );
    add_scene_type_if(
        &mut scene_types,
        text,
        WritingSceneType::Action,
        &[
            "拔", "挥", "冲", "扑", "闪", "避", "刺", "劈", "砍", "追", "打斗", "交锋",
        ],
    );
    add_scene_type_if(
        &mut scene_types,
        text,
        WritingSceneType::Description,
        &[
            "雾", "风", "月光", "烛", "气味", "潮湿", "冷意", "雪", "雨", "颜色", "影子",
        ],
    );
    add_scene_type_if(
        &mut scene_types,
        text,
        WritingSceneType::EmotionalBeat,
        &[
            "沉默", "心跳", "愤怒", "恐惧", "悲伤", "颤抖", "握紧", "犹豫", "后悔",
        ],
    );
    add_scene_type_if(
        &mut scene_types,
        text,
        WritingSceneType::ConflictEscalation,
        &[
            "突然",
            "然而",
            "不料",
            "没想到",
            "更糟",
            "危机",
            "追杀",
            "背叛",
            "阻止",
        ],
    );
    add_scene_type_if(
        &mut scene_types,
        text,
        WritingSceneType::Reveal,
        &[
            "真相", "揭开", "揭露", "发现", "原来", "秘密", "身份", "来源", "线索",
        ],
    );
    add_scene_type_if(
        &mut scene_types,
        text,
        WritingSceneType::SetupPayoff,
        &[
            "伏笔", "回收", "兑现", "下落", "承诺", "誓言", "代价", "结果", "收束",
        ],
    );
    add_scene_type_if(
        &mut scene_types,
        text,
        WritingSceneType::Exposition,
        &[
            "解释", "说明", "背景", "来历", "规则", "设定", "宗门", "朝堂", "历史",
        ],
    );
    add_scene_type_if(
        &mut scene_types,
        text,
        WritingSceneType::Transition,
        &[
            "翌日",
            "后来",
            "与此同时",
            "转眼",
            "回到",
            "离开",
            "抵达",
            "路上",
        ],
    );
    scene_types
}
