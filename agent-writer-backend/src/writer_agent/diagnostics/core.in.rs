
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticResult {
    pub id: String,
    pub severity: DiagnosticSeverity,
    pub category: DiagnosticCategory,
    pub message: String,
    pub entity_name: Option<String>,
    pub from: usize,
    pub to: usize,
    pub evidence: Vec<DiagnosticEvidence>,
    pub fix_suggestion: Option<String>,
    pub operations: Vec<WriterOperation>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum DiagnosticSeverity {
    Error,
    Warning,
    Info,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum DiagnosticCategory {
    CanonConflict,
    UnresolvedPromise,
    StoryContractViolation,
    ChapterMissionViolation,
    TimelineIssue,
    CharacterVoiceInconsistency,
    PacingNote,
    PayoffGap,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticEvidence {
    pub source: String,
    pub reference: String,
    pub snippet: String,
}

pub struct DiagnosticsEngine;

impl DiagnosticsEngine {
    pub fn new() -> Self {
        Self
    }

    /// Run all diagnostics on a paragraph within a chapter context.
    pub fn diagnose(
        &self,
        paragraph: &str,
        paragraph_offset: usize,
        chapter_id: &str,
        project_id: &str,
        memory: &WriterMemory,
    ) -> Vec<DiagnosticResult> {
        let mut results = Vec::new();
        let mut counter = 0u32;

        let mut next_id = || {
            counter += 1;
            format!("diag_{}_{}", chapter_id, counter)
        };

        // 1. Entity conflict + timeline state checks.
        let entities = extract_entities(paragraph, memory);

        // Story time context for timeline diagnostics.
        let time_slice_evidence: Option<DiagnosticEvidence> = memory
            .get_time_mapping_for_chapter(chapter_id)
            .ok()
            .and_then(|mappings| {
                mappings.first().and_then(|m| {
                    memory
                        .get_time_slice_by_id(m.time_slice_id)
                        .ok()
                        .flatten()
                        .map(|ts| DiagnosticEvidence {
                            source: "story_time".into(),
                            reference: format!("time_slice:{}", ts.id),
                            snippet: format!(
                                "{} (order: {}, mode: {})",
                                ts.label, ts.relative_order, m.narrative_mode
                            ),
                        })
                })
            });
        for entity in &entities {
            let canonical_entity = memory
                .resolve_canon_entity_name(entity)
                .ok()
                .flatten()
                .unwrap_or_else(|| entity.clone());
            if let Ok(facts) = memory.get_canon_facts_for_entity(entity) {
                for (key, canon_value) in &facts {
                    if let Some(mentioned_value) = detect_attribute_value(paragraph, entity, key) {
                        if !attribute_values_compatible(key, &mentioned_value, canon_value) {
                            let pos = paragraph
                                .find(&mentioned_value)
                                .map(|p| paragraph_offset + byte_to_char_index(paragraph, p))
                                .unwrap_or(paragraph_offset);
                            let to = pos + mentioned_value.chars().count();
                            results.push(DiagnosticResult {
                                id: next_id(),
                                severity: DiagnosticSeverity::Error,
                                category: DiagnosticCategory::CanonConflict,
                                message: format!(
                                    "{}: canon记录 {}={}，但文中出现 {}",
                                    canonical_entity, key, canon_value, mentioned_value
                                ),
                                entity_name: Some(canonical_entity.clone()),
                                from: pos,
                                to,
                                evidence: vec![DiagnosticEvidence {
                                    source: "canon".into(),
                                    reference: canonical_entity.clone(),
                                    snippet: format!("{} = {}", key, canon_value),
                                }],
                                fix_suggestion: Some(format!(
                                    "将 {} 改为 {}",
                                    mentioned_value, canon_value
                                )),
                                operations: canon_conflict_operations(
                                    chapter_id,
                                    pos,
                                    to,
                                    canon_value,
                                    &mentioned_value,
                                    &canonical_entity,
                                    key,
                                ),
                            });
                        }
                    }

                    if let Some(issue) = detect_timeline_issue(
                        paragraph,
                        paragraph_offset,
                        entity,
                        &canonical_entity,
                        key,
                        canon_value,
                    ) {
                        let mut evidence = vec![DiagnosticEvidence {
                            source: "canon".into(),
                            reference: canonical_entity.clone(),
                            snippet: format!("{} = {}", key, canon_value),
                        }];
                        if let Some(ref ts_evidence) = time_slice_evidence {
                            evidence.push(ts_evidence.clone());
                        }
                        results.push(DiagnosticResult {
                            id: next_id(),
                            severity: DiagnosticSeverity::Warning,
                            category: DiagnosticCategory::TimelineIssue,
                            message: issue.message,
                            entity_name: Some(canonical_entity.clone()),
                            from: issue.from,
                            to: issue.to,
                            evidence,
                            fix_suggestion: issue.fix_suggestion,
                            operations: Vec::new(),
                        });
                    }
                }
            }
        }

        // 1b. Hidden relationship exposure check.
        for entity in &entities {
            if let Ok(Some(character)) = memory.get_character_by_name(entity) {
                if let Ok(relationships) = memory.get_active_relationships(character.id, chapter_id) {
                    for rel in &relationships {
                        if rel.visibility == "hidden" {
                            if let (Ok(Some(a)), Ok(Some(b))) = (
                                memory.get_character_by_id(rel.character_a_id),
                                memory.get_character_by_id(rel.character_b_id),
                            ) {
                                if paragraph.contains(&a.name) && paragraph.contains(&b.name) {
                                    results.push(DiagnosticResult {
                                        id: next_id(),
                                        severity: DiagnosticSeverity::Warning,
                                        category: DiagnosticCategory::CanonConflict,
                                        message: format!(
                                            "隐藏角色关系可能被暴露: {} 与 {}",
                                            a.name, b.name
                                        ),
                                        entity_name: Some(entity.clone()),
                                        from: paragraph_offset,
                                        to: paragraph_offset + paragraph.chars().count(),
                                        evidence: vec![DiagnosticEvidence {
                                            source: "relationship".into(),
                                            reference: format!("rel:{}", rel.id),
                                            snippet: format!("{}:{} (hidden)", rel.relation_type, rel.visibility),
                                        }],
                                        fix_suggestion: Some("确认此处是否应当揭示隐藏关系，或调整叙述以避免暴露。".into()),
                                        operations: Vec::new(),
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }

        // 1c. Flashback identity consistency check
        if let Ok(mappings) = memory.get_time_mapping_for_chapter(chapter_id) {
            if mappings.iter().any(|m| m.narrative_mode == "flashback") {
                for entity in &entities {
                    if let Ok(Some(character)) = memory.get_character_by_name(entity) {
                        if let Ok(Some(identity)) = memory.get_active_identity(character.id, chapter_id) {
                            if identity.public_identity != identity.private_identity
                                && !identity.public_identity.is_empty()
                                && !identity.private_identity.is_empty()
                            {
                                results.push(DiagnosticResult {
                                    id: next_id(),
                                    severity: DiagnosticSeverity::Info,
                                    category: DiagnosticCategory::TimelineIssue,
                                    message: format!("闪回场景: {} 的公开身份({})在闪回时间点可能需要与当前身份({})一致", entity, identity.public_identity, identity.private_identity),
                                    entity_name: Some(entity.clone()),
                                    from: paragraph_offset,
                                    to: paragraph_offset + paragraph.chars().count(),
                                    evidence: vec![DiagnosticEvidence {
                                        source: "identity".into(), reference: entity.clone(),
                                        snippet: format!("flashback: public={} private={}", identity.public_identity, identity.private_identity),
                                    }],
                                    fix_suggestion: Some("确认闪回中角色的身份状态是否与故事时间一致".into()),
                                    operations: Vec::new(),
                                });
                            }
                        }
                    }
                }
            }
        }

        // 3. Knowledge visibility check: flag if characters act on knowledge they shouldn't have
        for entity in &entities {
            if let Ok(Some(character)) = memory.get_character_by_name(entity) {
                if let Ok(ownerships) = memory.get_knowledge_by_holder("character", character.id, chapter_id) {
                    for ownership in &ownerships {
                        if ownership.knowledge_mode == "misbelief" {
                            if paragraph.contains(&ownership.topic) {
                                results.push(DiagnosticResult {
                                    id: next_id(),
                                    severity: DiagnosticSeverity::Warning,
                                    category: DiagnosticCategory::CanonConflict,
                                    message: format!(
                                        "知识冲突: {} 仍处于误判状态，但段落内容涉及 {}",
                                        entity, ownership.topic
                                    ),
                                    entity_name: Some(entity.clone()),
                                    from: 0,
                                    to: paragraph.chars().count(),
                                    evidence: vec![DiagnosticEvidence {
                                        source: "knowledge".into(),
                                        reference: ownership.topic.clone(),
                                        snippet: format!("mode={}", ownership.knowledge_mode),
                                    }],
                                    fix_suggestion: Some("确认此角色此时是否应该知道这条信息".into()),
                                    operations: Vec::new(),
                                });
                            }
                        }
                    }
                }
            }
        }

        // 4. Identity-based canon drift: check public vs private identity
        for entity in &entities {
            if let Ok(Some(character)) = memory.get_character_by_name(entity) {
                if let Ok(Some(identity)) = memory.get_active_identity(character.id, chapter_id) {
                    if identity.public_identity != identity.private_identity
                        && !identity.public_identity.is_empty()
                        && !identity.private_identity.is_empty()
                    {
                        // Flag if both identities appear without reveal context
                        let has_public = paragraph.contains(&identity.public_identity);
                        let has_private = paragraph.contains(&identity.private_identity);
                        if has_public && has_private {
                            results.push(DiagnosticResult {
                                id: next_id(),
                                severity: DiagnosticSeverity::Warning,
                                category: DiagnosticCategory::CanonConflict,
                                message: format!(
                                    "身份冲突: {} 的公开身份({})与真实身份({})同时出现",
                                    entity, identity.public_identity, identity.private_identity
                                ),
                                entity_name: Some(entity.clone()),
                                from: 0,
                                to: paragraph.chars().count(),
                                evidence: vec![DiagnosticEvidence {
                                    source: "identity".into(),
                                    reference: entity.clone(),
                                    snippet: format!("public={} private={}", identity.public_identity, identity.private_identity),
                                }],
                                fix_suggestion: Some("确认此场景是否应该揭示真实身份".into()),
                                operations: Vec::new(),
                            });
                        }
                    }
                }
            }
        }

        // 2. Book-level contract checks.
        if let Ok(Some(contract)) = memory.get_story_contract(project_id) {
            for issue in detect_story_contract_violations(paragraph, paragraph_offset, &contract) {
                results.push(DiagnosticResult {
                    id: next_id(),
                    severity: issue.severity,
                    category: DiagnosticCategory::StoryContractViolation,
                    message: issue.message,
                    entity_name: None,
                    from: issue.from,
                    to: issue.to,
                    evidence: vec![DiagnosticEvidence {
                        source: "story_contract".into(),
                        reference: issue.reference,
                        snippet: issue.snippet,
                    }],
                    fix_suggestion: Some(issue.fix_suggestion),
                    operations: vec![WriterOperation::TextAnnotate {
                        chapter: chapter_id.to_string(),
                        from: issue.from,
                        to: issue.to,
                        message: issue.annotation,
                        severity: issue.annotation_severity,
                    }],
                });
            }
        }

        // 3. Chapter mission guard checks.
        if let Ok(Some(mission)) = memory.get_chapter_mission(project_id, chapter_id) {
            for issue in detect_chapter_mission_violations(paragraph, paragraph_offset, &mission) {
                results.push(DiagnosticResult {
                    id: next_id(),
                    severity: issue.severity,
                    category: DiagnosticCategory::ChapterMissionViolation,
                    message: issue.message,
                    entity_name: None,
                    from: issue.from,
                    to: issue.to,
                    evidence: vec![DiagnosticEvidence {
                        source: "chapter_mission".into(),
                        reference: issue.reference,
                        snippet: issue.snippet,
                    }],
                    fix_suggestion: Some(issue.fix_suggestion),
                    operations: vec![WriterOperation::TextAnnotate {
                        chapter: chapter_id.to_string(),
                        from: issue.from,
                        to: issue.to,
                        message: issue.annotation,
                        severity: issue.annotation_severity,
                    }],
                });
            }
        }

        // 4. Open promises for this chapter.
        if let Ok(promises) = memory.get_open_promise_summaries() {
            for promise in &promises {
                if !is_later_chapter(chapter_id, &promise.introduced_chapter) {
                    continue;
                }

                let mention = match_promise(paragraph, promise);
                if mention.is_match {
                    results.push(DiagnosticResult {
                        id: next_id(),
                        severity: DiagnosticSeverity::Info,
                        category: DiagnosticCategory::UnresolvedPromise,
                        message: format!(
                            "伏笔回收机会: {} ({}引入)",
                            promise.title, promise.introduced_chapter
                        ),
                        entity_name: None,
                        from: paragraph_offset + mention.from.unwrap_or(0),
                        to: paragraph_offset
                            + mention
                                .to
                                .unwrap_or_else(|| paragraph.chars().count())
                                .max(mention.from.unwrap_or(0) + 1),
                        evidence: vec![DiagnosticEvidence {
                            source: "promise".into(),
                            reference: promise.title.clone(),
                            snippet: promise.description.clone(),
                        }],
                        fix_suggestion: Some(format!(
                            "确认这里是否要回收伏笔：{}",
                            promise.expected_payoff
                        )),
                        operations: promise_decision_operations(promise, chapter_id),
                    });
                    continue;
                }

                if is_stale_promise(
                    chapter_id,
                    &promise.introduced_chapter,
                    &promise.expected_payoff,
                ) {
                    results.push(DiagnosticResult {
                        id: next_id(),
                        severity: DiagnosticSeverity::Warning,
                        category: DiagnosticCategory::UnresolvedPromise,
                        message: format!(
                            "伏笔仍未回收: {} ({}引入，预期{})",
                            promise.title,
                            promise.introduced_chapter,
                            if promise.expected_payoff.trim().is_empty() {
                                "后续回收"
                            } else {
                                promise.expected_payoff.as_str()
                            }
                        ),
                        entity_name: None,
                        from: paragraph_offset,
                        to: paragraph_offset + paragraph.chars().count().min(40),
                        evidence: vec![DiagnosticEvidence {
                            source: "promise".into(),
                            reference: promise.title.clone(),
                            snippet: promise.description.clone(),
                        }],
                        fix_suggestion: Some("决定回收、延后，或标记为废弃。".into()),
                        operations: promise_decision_operations(promise, chapter_id),
                    });
                }
            }
        }

        // 5. Pacing check (paragraph length).
        if paragraph.chars().count() > 2000 {
            results.push(DiagnosticResult {
                id: next_id(),
                severity: DiagnosticSeverity::Warning,
                category: DiagnosticCategory::PacingNote,
                message: "段落较长(>2000字)，考虑拆分或检查节奏".into(),
                entity_name: None,
                from: paragraph_offset,
                to: paragraph_offset + 10,
                evidence: vec![],
                fix_suggestion: Some("在对话或动作处拆分段落".into()),
                operations: Vec::new(),
            });
        }

        // 5. Emotional debt pressure check.
        let debt_keywords = ["愤怒", "悲伤", "背叛", "恐惧", "悔恨"];
        let mut has_pressure = false;
        for kw in &debt_keywords {
            if paragraph.contains(kw) {
                has_pressure = true;
                break;
            }
        }
        if has_pressure {
            // Check if there's a payoff or resolution in the same paragraph
            let has_resolution = paragraph.contains("放下")
                || paragraph.contains("释怀")
                || paragraph.contains("原谅");
            if !has_resolution {
                results.push(DiagnosticResult {
                    id: next_id(),
                    severity: DiagnosticSeverity::Info,
                    category: DiagnosticCategory::TimelineIssue,
                    message: "情绪压力场景未发现释放/解决信号".to_string(),
                    entity_name: None,
                    from: paragraph_offset,
                    to: paragraph_offset + paragraph.chars().count(),
                    evidence: vec![DiagnosticEvidence {
                        source: "emotional_debt".into(),
                        reference: "chapter".into(),
                        snippet: "pressure without payoff".to_string(),
                    }],
                    fix_suggestion: Some("考虑在后续场景中添加情绪释放或解决".into()),
                    operations: Vec::new(),
                });
            }
        }

        // Voice drift check for protagonist characters
        if let Ok(chars) = memory.list_characters(Some("protagonist")) {
            for c in &chars {
                if paragraph.contains(&c.name) {
                    let sentences: Vec<&str> = paragraph.split('。').filter(|s| !s.trim().is_empty()).collect();
                    if !sentences.is_empty() {
                        let avg_len = sentences.iter().map(|s| s.chars().count()).sum::<usize>() / sentences.len();
                        if avg_len > 80 && (c.current_state_summary.contains("寡言") || c.current_state_summary.contains("沉默")) {
                            results.push(DiagnosticResult {
                                id: next_id(),
                                severity: DiagnosticSeverity::Info,
                                category: DiagnosticCategory::CanonConflict,
                                message: format!("角色声音漂移: {} 以长句为主(avg {}字/句)，与设定可能不一致", c.name, avg_len),
                                entity_name: Some(c.name.clone()),
                                from: paragraph_offset,
                                to: paragraph_offset + paragraph.chars().count(),
                                evidence: vec![DiagnosticEvidence { source: "voice".into(), reference: c.name.clone(), snippet: format!("avg_sentence_len={}", avg_len) }],
                                fix_suggestion: Some("检查角色对话/叙述风格是否与设定一致".into()),
                                operations: Vec::new(),
                            });
                            break; // one voice drift per paragraph is enough
                        }
                    }
                }
            }
        }

        // Pacing monotony check
        if let Ok(recent) = memory.list_recent_chapter_results(project_id, 5) {
            if recent.len() >= 4 {
                let action_chapters = recent.iter().filter(|r| r.summary.contains("冲突") || r.summary.contains("战斗")).count();
                if action_chapters >= 4 {
                    results.push(DiagnosticResult {
                        id: next_id(),
                        severity: DiagnosticSeverity::Info,
                        category: DiagnosticCategory::PacingNote,
                        message: format!("节奏单调: 最近{}章中有{}章以动作/冲突为主，建议插入过渡或情绪章节", recent.len(), action_chapters),
                        entity_name: None,
                        from: paragraph_offset, to: paragraph_offset + 1,
                        evidence: vec![DiagnosticEvidence { source: "pacing".into(), reference: "recent_chapters".into(), snippet: format!("action_ratio={}/{}", action_chapters, recent.len()) }],
                        fix_suggestion: Some("考虑插入过渡章节或情绪场景以调节节奏".into()),
                        operations: Vec::new(),
                    });
                }
            }
        }

        // Scene density check
        if let Ok(scenes) = memory.list_scenes_by_chapter(chapter_id) {
            if scenes.len() > 6 {
                results.push(DiagnosticResult {
                    id: next_id(),
                    severity: DiagnosticSeverity::Info,
                    category: DiagnosticCategory::PacingNote,
                    message: format!("场景密度较高: 本章有{}个场景，建议不超过6个", scenes.len()),
                    entity_name: None,
                    from: paragraph_offset, to: paragraph_offset + 1,
                    evidence: vec![DiagnosticEvidence { source: "scene".into(), reference: chapter_id.into(), snippet: format!("scene_count={}", scenes.len()) }],
                    fix_suggestion: Some("考虑合并部分场景或移至下一章".into()),
                    operations: Vec::new(),
                });
            }
        }

        // 6. Adjust severity based on author ignore patterns.
        for result in &mut results {
            let category_str = diagnostic_category_str(&result.category);
            let ignore_rate = author_ignore_rate(category_str, memory);
            if ignore_rate > 0.6 {
                match &result.category {
                    DiagnosticCategory::CanonConflict => {
                        if result.severity == DiagnosticSeverity::Warning {
                            result.severity = DiagnosticSeverity::Info;
                        }
                    }
                    DiagnosticCategory::StoryContractViolation
                    | DiagnosticCategory::ChapterMissionViolation => {
                        if result.severity == DiagnosticSeverity::Error {
                            result.severity = DiagnosticSeverity::Warning;
                        }
                    }
                    _ => {}
                }
            }
        }

        results
    }
}

fn diagnostic_category_str(category: &DiagnosticCategory) -> &'static str {
    match category {
        DiagnosticCategory::CanonConflict => "canon_conflict",
        DiagnosticCategory::UnresolvedPromise => "unresolved_promise",
        DiagnosticCategory::StoryContractViolation => "story_contract_violation",
        DiagnosticCategory::ChapterMissionViolation => "chapter_mission_violation",
        DiagnosticCategory::TimelineIssue => "timeline_issue",
        DiagnosticCategory::CharacterVoiceInconsistency => "character_voice_inconsistency",
        DiagnosticCategory::PacingNote => "pacing_note",
        DiagnosticCategory::PayoffGap => "payoff_gap",
    }
}

/// Returns the fraction of recent feedback where the author ignored proposals
/// of a given diagnostic category. Returns 0.0 when insufficient data exists.
pub fn author_ignore_rate(category: &str, memory: &WriterMemory) -> f64 {
    let audits = match memory.list_memory_audit(30) {
        Ok(list) => list,
        Err(_) => return 0.0,
    };
    let mut seen = 0usize;
    let mut ignored = 0usize;
    for entry in &audits {
        if entry.kind == category || entry.kind.contains(category) {
            seen += 1;
            if entry.action.contains("ignored") || entry.action.contains("rejected") || entry.action.contains("snoozed") {
                ignored += 1;
            }
        }
    }
    if seen < 5 {
        return 0.0;
    }
    ignored as f64 / seen as f64
}

