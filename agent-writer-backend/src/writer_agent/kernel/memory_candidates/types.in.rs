
pub(crate) enum CandidateSource {
    Local,
    Llm(String),
}

impl CandidateSource {
    fn label(&self) -> String {
        match self {
            CandidateSource::Local => "本地记忆抽取".to_string(),
            CandidateSource::Llm(model) => format!("LLM增强记忆抽取: {}.", model),
        }
    }

    fn canon_metadata(&self) -> (String, f64, Vec<String>) {
        match self {
            CandidateSource::Local => (
                "章节保存后发现可复用人物/物件设定，建议写入长期 canon。".to_string(),
                0.62,
                vec!["自动抽取可能误把普通名词当设定，请确认后接受。".to_string()],
            ),
            CandidateSource::Llm(model) => (
                format!("LLM增强记忆抽取: {}. 建议写入长期 canon。", model),
                0.78,
                vec!["LLM 抽取仍需人工确认，避免把临场描述误记成长期设定。".to_string()],
            ),
        }
    }

    fn promise_metadata(&self) -> (String, f64, Vec<String>) {
        match self {
            CandidateSource::Local => (
                "章节保存后发现未回收信息，建议加入伏笔 ledger 以便后续提醒。".to_string(),
                0.66,
                vec!["请确认这是真伏笔，而不是只在当前场景内解决的信息。".to_string()],
            ),
            CandidateSource::Llm(model) => (
                format!("LLM增强记忆抽取: {}. 建议加入伏笔 ledger。", model),
                0.8,
                vec!["请确认这是真伏笔，而不是 LLM 过度解读。".to_string()],
            ),
        }
    }
}

pub(crate) fn extract_new_canon_entities(text: &str, known: &[String]) -> Vec<CanonEntityOp> {
    let mut entities = Vec::new();
    for sentence in split_sentences(text) {
        for cue in ["名叫", "叫做", "名为", "代号"] {
            if let Some(name) = extract_name_after(&sentence, cue) {
                if should_keep_entity(&name, known, &entities) {
                    entities.push(CanonEntityOp {
                        kind: "character".to_string(),
                        name: name.clone(),
                        aliases: vec![],
                        summary: sentence_snippet(&sentence, 120),
                        attributes: serde_json::json!({}),
                        confidence: 0.62,
                    });
                }
            }
        }

        for marker in ["寒影刀", "玉佩", "密信", "钥匙", "令牌"] {
            if sentence.contains(marker) && should_keep_entity(marker, known, &entities) {
                entities.push(CanonEntityOp {
                    kind: "object".to_string(),
                    name: marker.to_string(),
                    aliases: vec![],
                    summary: sentence_snippet(&sentence, 120),
                    attributes: serde_json::json!({ "category": "story_object" }),
                    confidence: 0.58,
                });
            }
        }
    }
    entities
}

pub fn extract_plot_promises(text: &str, observation: &WriterObservation) -> Vec<PlotPromiseOp> {
    let mut promises = Vec::new();
    for sentence in split_sentences(text) {
        if !contains_promise_cue(&sentence) {
            continue;
        }
        let title = promise_title(&sentence);
        if title.is_empty() || promises.iter().any(|p: &PlotPromiseOp| p.title == title) {
            continue;
        }
        let kind = promise_kind_from_cues(&sentence);
        let priority = match kind {
            PromiseKind::ObjectWhereabouts | PromiseKind::MysteryClue => 5,
            PromiseKind::CharacterCommitment | PromiseKind::EmotionalDebt => 4,
            _ => 3,
        };
        let related = extract_related_entities(&sentence);
        promises.push(PlotPromiseOp {
            kind: kind.as_kind_str().to_string(),
            title,
            description: sentence_snippet(&sentence, 140),
            introduced_chapter: observation
                .chapter_title
                .clone()
                .unwrap_or_else(|| "current chapter".to_string()),
            expected_payoff: "后续章节回收或解释".to_string(),
            priority,
            related_entities: related,
        });
    }
    promises
}

pub(crate) fn llm_memory_candidates_from_value(
    value: serde_json::Value,
    observation: &WriterObservation,
    _model: &str,
) -> Vec<MemoryCandidate> {
    let mut candidates = Vec::new();

    if let Some(canon) = value.get("canon").and_then(|v| v.as_array()) {
        for item in canon.iter().take(5) {
            let name = item
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim();
            if name.chars().count() < 2 || name.chars().count() > 16 {
                continue;
            }
            let summary = item
                .get("summary")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim();
            if summary.chars().count() < 6 {
                continue;
            }
            let kind = item
                .get("kind")
                .and_then(|v| v.as_str())
                .unwrap_or("entity")
                .trim();
            let confidence = item
                .get("confidence")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.75)
                .clamp(0.0, 1.0);
            if confidence < 0.55 {
                continue;
            }
            let aliases = item
                .get("aliases")
                .and_then(|v| v.as_array())
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|alias| alias.as_str())
                        .map(str::trim)
                        .filter(|alias| !alias.is_empty())
                        .take(6)
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let attributes = item
                .get("attributes")
                .cloned()
                .filter(|value| value.is_object())
                .unwrap_or_else(|| serde_json::json!({}));
            candidates.push(MemoryCandidate::Canon(CanonEntityOp {
                kind: if kind.is_empty() {
                    "entity".to_string()
                } else {
                    kind.to_string()
                },
                name: name.to_string(),
                aliases,
                summary: sentence_snippet(summary, 180),
                attributes,
                confidence,
            }));
        }
    }

    if let Some(promises) = value.get("promises").and_then(|v| v.as_array()) {
        for item in promises.iter().take(5) {
            let title = item
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim();
            let description = item
                .get("description")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim();
            if title.chars().count() < 2 || description.chars().count() < 6 {
                continue;
            }
            let confidence = item
                .get("confidence")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.75)
                .clamp(0.0, 1.0);
            if confidence < 0.55 {
                continue;
            }
            candidates.push(MemoryCandidate::Promise(PlotPromiseOp {
                kind: item
                    .get("kind")
                    .and_then(|v| v.as_str())
                    .unwrap_or("open_question")
                    .trim()
                    .to_string(),
                title: sentence_snippet(title, 40),
                description: sentence_snippet(description, 180),
                introduced_chapter: item
                    .get("introducedChapter")
                    .or_else(|| item.get("introduced_chapter"))
                    .and_then(|v| v.as_str())
                    .unwrap_or_else(|| {
                        observation
                            .chapter_title
                            .as_deref()
                            .unwrap_or("current chapter")
                    })
                    .trim()
                    .to_string(),
                expected_payoff: item
                    .get("expectedPayoff")
                    .or_else(|| item.get("expected_payoff"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("后续章节回收或解释")
                    .trim()
                    .to_string(),
                priority: item
                    .get("priority")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(3)
                    .clamp(0, 10) as i32,
                related_entities: vec![],
            }));
        }
    }

    dedupe_memory_candidates(candidates)
}

fn dedupe_memory_candidates(candidates: Vec<MemoryCandidate>) -> Vec<MemoryCandidate> {
    let mut seen = HashSet::new();
    let mut deduped = Vec::new();
    for candidate in candidates {
        let key = match &candidate {
            MemoryCandidate::Canon(entity) => format!("canon:{}", entity.name),
            MemoryCandidate::Promise(promise) => format!("promise:{}", promise.title),
        };
        if seen.insert(key) {
            deduped.push(candidate);
        }
    }
    deduped
}
