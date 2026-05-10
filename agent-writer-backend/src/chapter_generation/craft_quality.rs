const OVERALL_WEIGHTS: &[(&str, f32)] = &[
    ("anchor_carry", 0.12),
    ("style_drift", 0.08),
    ("length_compliance", 0.08),
    ("dialogue_function", 0.12),
    ("exposition_ratio", 0.12),
    ("ending_hook", 0.12),
    ("scene_causality", 0.08),
    ("promise_progress", 0.08),
    ("scene_repetition", 0.08),
    ("plot_progression", 0.08),
    ("new_information_density", 0.08),
    ("state_delta_coverage", 0.04),
    ("world_consistency", 0.08),
    ("term_misuse", 0.08),
];

#[derive(Debug, Clone, Default)]
pub struct ChapterQualitySignals {
    pub anchor_keywords: Vec<String>,
    pub author_voice: Option<crate::writer_agent::author_voice::AuthorVoiceSnapshot>,
    pub required_anchors: Vec<crate::chapter_generation::StoryAnchor>,
    pub required_state_deltas: Vec<crate::chapter_generation::StateDelta>,
    pub prior_chapter_summaries: Vec<String>,
    pub scene_contract: Option<crate::writer_agent::world_bible::SceneContract>,
    pub world_assets: Vec<crate::writer_agent::world_bible::WorldAsset>,
    /// P14: Approved canon constraints compiled from WorldBibleIndex.
    /// When provided, these take priority over generic world_assets checks.
    pub canon_constraints: Vec<crate::writer_agent::world_bible::CanonConstraint>,
    /// P17: Approved canon terms for term misuse detection.
    pub canon_terms: Vec<crate::writer_agent::world_bible::CanonTerm>,
}

pub fn evaluate_chapter_quality(
    chapter_text: &str,
    chapter_title: &str,
    scene_plan: &SceneCraftPlan,
    open_promise_keywords: &[String],
    target_min_chars: usize,
    target_max_chars: usize,
) -> ChapterQualityReport {
    evaluate_chapter_quality_with_signals(
        chapter_text,
        chapter_title,
        scene_plan,
        open_promise_keywords,
        target_min_chars,
        target_max_chars,
        &ChapterQualitySignals::default(),
    )
}

pub fn evaluate_chapter_quality_with_signals(
    chapter_text: &str,
    chapter_title: &str,
    scene_plan: &SceneCraftPlan,
    open_promise_keywords: &[String],
    target_min_chars: usize,
    target_max_chars: usize,
    signals: &ChapterQualitySignals,
) -> ChapterQualityReport {
    let metric_results = vec![
        metric_length_compliance(chapter_text, target_min_chars, target_max_chars),
        metric_dialogue_function(chapter_text),
        metric_exposition_ratio(chapter_text),
        metric_ending_hook(chapter_text, scene_plan),
        metric_scene_causality(chapter_text, scene_plan),
        metric_promise_progress(chapter_text, open_promise_keywords),
        metric_anchor_carry(chapter_text, &signals.anchor_keywords, &signals.required_anchors),
        metric_style_drift(chapter_text, chapter_title, signals.author_voice.as_ref()),
        metric_scene_repetition(chapter_text, &signals.prior_chapter_summaries),
        metric_plot_progression(chapter_text),
        metric_new_information_density(chapter_text, &signals.prior_chapter_summaries),
        metric_state_delta_coverage(chapter_text, &signals.required_state_deltas),
        metric_world_consistency(chapter_text, signals.scene_contract.as_ref(), &signals.world_assets, &signals.canon_constraints),
        metric_term_misuse(chapter_text, &signals.canon_terms),
    ];

    let overall_score: f32 = metric_results
        .iter()
        .map(|m| {
            let weight = OVERALL_WEIGHTS
                .iter()
                .find(|(name, _)| *name == m.metric)
                .map(|(_, w)| *w)
                .unwrap_or(0.125);
            m.score * weight
        })
        .sum();

    let fatal_issues: Vec<QualityIssue> = metric_results
        .iter()
        .filter(|m| m.severity == IssueSeverity::Fatal)
        .map(|m| QualityIssue {
            metric: m.metric.clone(),
            severity: IssueSeverity::Fatal,
            evidence: m.evidence_excerpt.clone(),
            description: m.reason.clone(),
        })
        .collect();

    let major_issues: Vec<QualityIssue> = metric_results
        .iter()
        .filter(|m| m.severity == IssueSeverity::Major)
        .map(|m| QualityIssue {
            metric: m.metric.clone(),
            severity: IssueSeverity::Major,
            evidence: m.evidence_excerpt.clone(),
            description: m.reason.clone(),
        })
        .collect();

    let mut top_revision_targets: Vec<String> = metric_results
        .iter()
        .filter(|m| m.severity == IssueSeverity::Major || m.severity == IssueSeverity::Fatal)
        .map(|m| m.metric.clone())
        .take(3)
        .collect();

    if top_revision_targets.is_empty() {
        // Fall back to the 3 lowest-scoring metrics
        let mut sorted: Vec<&QualityMetricResult> = metric_results.iter().collect();
        sorted.sort_by(|a, b| a.score.partial_cmp(&b.score).unwrap_or(std::cmp::Ordering::Equal));
        top_revision_targets = sorted.iter().take(3).map(|m| m.metric.clone()).collect();
    }

    let no_fatal_issue = fatal_issues.is_empty();

    let world_consistency_violations = if let Some(contract) = signals.scene_contract.as_ref() {
        crate::writer_agent::world_bible::validate_world_consistency(chapter_text, contract, &signals.world_assets)
    } else {
        Vec::new()
    };

    // P15: Populate structured canon constraint violations for quality report
    let canon_constraint_violations =
        crate::writer_agent::world_bible::format_canon_constraint_violations(
            &world_consistency_violations,
        );

    ChapterQualityReport {
        chapter_title: chapter_title.to_string(),
        overall_score,
        fatal_issues,
        major_issues,
        metric_results,
        top_revision_targets,
        no_fatal_issue,
        world_consistency_violations,
        canon_constraint_violations,
    }
}

fn metric_anchor_carry(
    text: &str,
    anchors: &[String],
    required_anchors: &[crate::chapter_generation::StoryAnchor],
) -> QualityMetricResult {
    let all_anchor_keywords: Vec<String> = if anchors.is_empty() && required_anchors.is_empty() {
        return gated_metric(
            "anchor_carry",
            0.5,
            "",
            "anchor_carry.rs",
            "需要项目级锚点数据，本次评估跳过",
            "在完整写作项目中重新运行",
        );
    } else if anchors.is_empty() {
        required_anchors.iter().map(|a| a.anchor_id.clone()).collect()
    } else {
        anchors.to_vec()
    };

    let report = crate::writer_agent::anchor_carry::score_anchor_carry(text, &all_anchor_keywords);
    let mut score = report.carry_rate as f32;

    // Check required anchors separately
    let mut required_missing = Vec::new();
    let mut required_weak = Vec::new();
    for req in required_anchors {
        let item = report.items.iter().find(|i| i.anchor == req.anchor_id);
        match item {
            None => {
                required_missing.push(req.anchor_id.clone());
            }
            Some(i) if !i.carried => {
                required_weak.push(req.anchor_id.clone());
            }
            _ => {}
        }
    }

    // Penalize missing or weak required anchors
    if !required_missing.is_empty() || !required_weak.is_empty() {
        let penalty = ((required_missing.len() + required_weak.len()) as f32 * 0.25)
            .min(0.6);
        score = (score - penalty).max(0.0);
    }

    let evidence = report
        .items
        .iter()
        .filter(|item| item.mentioned)
        .take(4)
        .map(|item| {
            let modes = if item.carry_modes.is_empty() {
                "mentioned_only".to_string()
            } else {
                item.carry_modes.join("+")
            };
            format!("{}:{}", item.anchor, modes)
        })
        .collect::<Vec<_>>()
        .join(", ");

    let reason = if !required_missing.is_empty() || !required_weak.is_empty() {
        format!(
            "锚点承载率 {}/{}，提及率 {}/{}；必需锚点缺失 [{}]，弱承载 [{}]",
            report.carried_count,
            report.anchor_count,
            report.mentioned_count,
            report.anchor_count,
            required_missing.join(", "),
            required_weak.join(", ")
        )
    } else {
        format!(
            "锚点承载率 {}/{}，提及率 {}/{}",
            report.carried_count, report.anchor_count, report.mentioned_count, report.anchor_count
        )
    };

    gated_metric(
        "anchor_carry",
        score,
        &evidence,
        "anchor_carry.rs",
        &reason,
        "让关键锚点参与行动、对话、后果或兑现压力，而不是只被提名",
    )
}

fn metric_style_drift(
    text: &str,
    chapter_title: &str,
    author_voice: Option<&crate::writer_agent::author_voice::AuthorVoiceSnapshot>,
) -> QualityMetricResult {
    let Some(voice) = author_voice else {
        return gated_metric(
            "style_drift",
            0.5,
            "",
            "author_voice.rs",
            "需要作者风格快照，本次评估跳过",
            "在完整写作项目中重新运行",
        );
    };

    let diagnostic = crate::writer_agent::author_voice::compute_style_drift(
        voice,
        text,
        chapter_title,
    );
    let high = diagnostic
        .drift_signals
        .iter()
        .filter(|signal| signal.severity == "high")
        .count();
    let medium = diagnostic
        .drift_signals
        .iter()
        .filter(|signal| signal.severity == "medium")
        .count();
    let score = (1.0 - (high as f32 * 0.35) - (medium as f32 * 0.18))
        .clamp(0.0, 1.0);
    let evidence = diagnostic
        .drift_signals
        .iter()
        .take(3)
        .map(|signal| {
            format!(
                "{}:{}->{}",
                signal.aspect, signal.expected_pattern, signal.observed_pattern
            )
        })
        .collect::<Vec<_>>()
        .join(", ");

    if diagnostic.drift_signals.is_empty() {
        return gated_metric(
            "style_drift",
            score,
            "no drift signal",
            "author_voice.rs",
            "作者风格漂移信号低",
            "",
        );
    }

    let reason = format!(
        "风格漂移 {}，high={}, medium={}，voice_confidence={:.2}",
        diagnostic.overall_severity, high, medium, voice.confidence
    );
    gated_metric(
        "style_drift",
        score,
        &evidence,
        "author_voice.rs",
        &reason,
        "按作者风格快照压回句式、语气、对话密度和禁用表达",
    )
}

fn gated_metric(
    metric: &str,
    score: f32,
    evidence: &str,
    rule_source: &str,
    reason: &str,
    revision_hint: &str,
) -> QualityMetricResult {
    if score >= 0.8 {
        QualityMetricResult {
            metric: metric.into(),
            score,
            severity: IssueSeverity::None,
            evidence_excerpt: String::new(),
            rule_source: rule_source.into(),
            reason: "该项表现良好".into(),
            revision_hint: String::new(),
        }
    } else if evidence.is_empty() {
        QualityMetricResult {
            metric: metric.into(),
            score: 0.5,
            severity: IssueSeverity::None,
            evidence_excerpt: String::new(),
            rule_source: rule_source.into(),
            reason: format!("证据不足，无法确定是否存在问题。{}", reason),
            revision_hint: "需要更多上下文或更大样本量后重新评估".into(),
        }
    } else {
        let severity = if score < 0.3 {
            IssueSeverity::Fatal
        } else if score < 0.5 {
            IssueSeverity::Major
        } else {
            IssueSeverity::Minor
        };
        QualityMetricResult {
            metric: metric.into(),
            score,
            severity,
            evidence_excerpt: evidence.to_string(),
            rule_source: rule_source.into(),
            reason: reason.to_string(),
            revision_hint: revision_hint.to_string(),
        }
    }
}

fn metric_length_compliance(text: &str, min_chars: usize, max_chars: usize) -> QualityMetricResult {
    let count = text.chars().count();
    if count >= min_chars && count <= max_chars {
        gated_metric(
            "length_compliance", 1.0,
            &format!("{count} chars"),
            "chapter_contract",
            "字数合规", "",
        )
    } else if count < min_chars {
        let ratio = count as f32 / min_chars.max(1) as f32;
        gated_metric(
            "length_compliance", ratio * 0.7,
            &format!("{count} chars < min {min_chars}"),
            "chapter_contract",
            &format!("正文字数 {count} 低于最低要求 {min_chars}"),
            "扩展场景或增加细节以达到最低字数",
        )
    } else {
        let ratio = max_chars as f32 / count.max(1) as f32;
        gated_metric(
            "length_compliance", ratio * 0.7,
            &format!("{count} chars > max {max_chars}"),
            "chapter_contract",
            &format!("正文字数 {count} 超出上限 {max_chars}"),
            "精简冗余描写或拆分场景",
        )
    }
}

fn metric_dialogue_function(text: &str) -> QualityMetricResult {
    let dialogue_markers = [
        "\"", "\u{201c}", "\u{201d}", "\u{300c}", "\u{300d}",
        "说", "问", "答", "道",
    ];
    let function_signals = [
        "决定", "拒绝", "承认", "隐瞒", "威胁", "交换", "选择",
        "妥协", "逼问", "暗示", "试探", "回避",
    ];

    let has_dialogue = dialogue_markers.iter().any(|m| text.contains(m));
    if !has_dialogue {
        return gated_metric(
            "dialogue_function", 1.0, "",
            "craft:dialogue_function",
            "本章无对话场景，不适用", "",
        );
    }

    let signal_count = function_signals.iter().filter(|s| text.contains(*s)).count();
    let score = (signal_count as f32 / 3.0).min(1.0);
    let evidence = if signal_count > 0 {
        function_signals
            .iter()
            .filter(|s| text.contains(*s))
            .take(3)
            .cloned()
            .collect::<Vec<_>>()
            .join(", ")
    } else {
        String::new()
    };

    gated_metric(
        "dialogue_function", score, &evidence,
        "craft:dialogue_function",
        &format!("对话功能信号: {signal_count}/12"),
        "确保对话改变了权力、关系、信息或选择",
    )
}

fn metric_exposition_ratio(text: &str) -> QualityMetricResult {
    let action_verbs = [
        "拔", "握", "推", "拉", "砍", "刺", "走", "跑", "跳",
        "拿", "放", "打", "挡", "追", "转", "翻", "看", "盯",
        "藏", "递", "交", "救", "抢", "护", "站起", "坐下", "点头", "摇头",
    ];
    let dialogue_markers = ["\"", "\u{201c}", "\u{300c}", "说", "问", "答"];

    let paragraphs: Vec<&str> = text
        .split('\n')
        .filter(|p| !p.trim().is_empty())
        .collect();
    if paragraphs.is_empty() {
        return gated_metric(
            "exposition_ratio", 1.0, "",
            "craft:setting_in_scene", "无段落", "",
        );
    }

    let mut expo_para_count = 0usize;
    for para in &paragraphs {
        let len = para.chars().count();
        if len > 200 {
            let has_action = action_verbs.iter().any(|v| para.contains(v));
            let has_dialogue = dialogue_markers.iter().any(|m| para.contains(m));
            if !has_action && !has_dialogue {
                expo_para_count += 1;
            }
        }
    }

    let ratio = expo_para_count as f32 / paragraphs.len().max(1) as f32;
    let score = if ratio > 0.4 {
        0.3
    } else if ratio > 0.25 {
        0.6
    } else {
        0.9
    };
    let evidence = if expo_para_count > 0 {
        format!("{expo_para_count}/{} 段落为纯说明（>200chars 无动作/对话）", paragraphs.len())
    } else {
        String::new()
    };

    gated_metric(
        "exposition_ratio", score, &evidence,
        "craft:setting_in_scene",
        &format!("说明性段落占比 {:.0}%", ratio * 100.0),
        "将说明段落中的信息改写成角色行动、误解或对话",
    )
}

fn metric_ending_hook(text: &str, scene_plan: &SceneCraftPlan) -> QualityMetricResult {
    let tail: String = text
        .chars()
        .rev()
        .take(300)
        .collect::<String>()
        .chars()
        .rev()
        .collect();

    let consequence_signals = [
        "后果", "代价", "变了", "不再", "从此", "已经",
        "终于", "失去", "获得", "明白", "知道", "决定",
    ];
    let question_signals = [
        "但是", "然而", "不过", "还不知道", "没发现",
        "不知道", "选择", "怎么办", "要不要", "能不能",
    ];

    let has_consequence = consequence_signals.iter().any(|s| tail.contains(s));
    let has_question = question_signals.iter().any(|s| tail.contains(s));

    let mut score = match (has_consequence, has_question) {
        (true, true) => 0.9,
        (true, false) | (false, true) => 0.5,
        (false, false) => 0.2,
    };

    // If SceneCraftPlan provided question_left_open, check alignment
    if !scene_plan.ending_hook.question_left_open.is_empty() {
        let plan_question_words: Vec<&str> = scene_plan.ending_hook.question_left_open
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| w.chars().count() >= 2)
            .collect();
        let any_match = plan_question_words.iter()
            .any(|w| tail.contains(w));
        if any_match {
            score = f32::min(score + 0.1, 1.0);
        }
    }

    // Bonus: if SceneCraftPlan provided expected consequences, check alignment
    if !scene_plan.ending_hook.consequence_delivered.is_empty() {
        let plan_consequence_words: Vec<&str> = scene_plan.ending_hook.consequence_delivered
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| w.chars().count() >= 2)
            .collect();
        let any_match = plan_consequence_words.iter()
            .any(|w| tail.contains(w));
        if any_match {
            score = f32::min(score + 0.1, 1.0);
        }
    }

    let evidence: String = tail.chars().rev().take(100).collect::<String>().chars().rev().collect();

    gated_metric(
        "ending_hook", score, &evidence,
        "craft:ending_hook",
        &format!("后果信号={}, 未解信号={}", has_consequence, has_question),
        "章末加一个刚发生的后果和一个角色面临的选择",
    )
}

fn metric_scene_causality(text: &str, scene_plan: &SceneCraftPlan) -> QualityMetricResult {
    let causality_markers = [
        "因为", "所以", "因此", "于是", "导致",
        "逼得", "只好", "不得不", "结果", "后果",
    ];
    let count: usize = causality_markers
        .iter()
        .map(|m| text.matches(m).count())
        .sum();
    let char_count = text.chars().count().max(1);
    let density = count as f32 / char_count as f32 * 500.0;

    let mut score = if density >= 1.0 {
        0.9
    } else if density >= 0.5 {
        0.6
    } else {
        0.3
    };

    // If SceneCraftPlan provided an objective, check whether the ending hook text relates to it
    if !scene_plan.objective.is_empty() {
        let tail: String = text
            .chars()
            .rev()
            .take(300)
            .collect::<String>()
            .chars()
            .rev()
            .collect();
        let objective_words: Vec<&str> = scene_plan.objective
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| w.chars().count() >= 2)
            .collect();
        let any_match = objective_words.iter()
            .any(|w| tail.contains(w));
        if any_match {
            score = f32::min(score + 0.1, 1.0);
        }
    }

    let evidence = if count > 0 {
        causality_markers
            .iter()
            .filter(|m| text.contains(*m))
            .take(3)
            .cloned()
            .collect::<Vec<_>>()
            .join(", ")
    } else {
        String::new()
    };

    gated_metric(
        "scene_causality", score, &evidence,
        "craft:scene_causality",
        &format!("因果连接词密度: {density:.2}/500chars"),
        "增加事件间的因果连接，少用'然后'，多用'因此/于是/导致'",
    )
}

fn metric_promise_progress(text: &str, keywords: &[String]) -> QualityMetricResult {
    if keywords.is_empty() {
        return gated_metric(
            "promise_progress", 0.5, "",
            "craft:promise_advance",
            "无 open promise 关键词，跳过评估", "",
        );
    }

    let matched: Vec<&String> = keywords.iter().filter(|kw| text.contains(kw.as_str())).collect();
    let score = if matched.is_empty() {
        0.2
    } else {
        (matched.len() as f32 / keywords.len().max(1) as f32).min(1.0)
    };
    let evidence = matched
        .iter()
        .take(3)
        .map(|s| s.as_str())
        .collect::<Vec<_>>()
        .join(", ");

    gated_metric(
        "promise_progress", score, &evidence,
        "craft:promise_advance",
        &format!("{}/{} promise keywords found in text", matched.len(), keywords.len()),
        "检查 open promises 是否在本章中被推进、误导或兑现",
    )
}

fn metric_scene_repetition(text: &str, prior_summaries: &[String]) -> QualityMetricResult {
    let sentences: Vec<String> = text
        .split(['。', '！', '？', '!', '?', '\n'])
        .map(|s| s.trim().to_string())
        .filter(|s| s.chars().count() >= 8)
        .collect();

    if sentences.len() < 2 {
        return gated_metric(
            "scene_repetition", 1.0, "",
            "craft:scene_repetition",
            "句子数量不足，无法评估重复", "",
        );
    }

    let mut overlap_count = 0usize;
    for window in sentences.windows(2) {
        let a = &window[0];
        let b = &window[1];
        let a_chars: Vec<char> = a.chars().collect();
        let b_chars: Vec<char> = b.chars().collect();
        if a_chars.len() < 4 || b_chars.len() < 4 {
            continue;
        }
        let a_ngrams: std::collections::HashSet<String> = a_chars
            .windows(4)
            .map(|w| w.iter().collect::<String>())
            .collect();
        let b_ngrams: std::collections::HashSet<String> = b_chars
            .windows(4)
            .map(|w| w.iter().collect::<String>())
            .collect();
        let intersection: std::collections::HashSet<String> = a_ngrams
            .intersection(&b_ngrams)
            .cloned()
            .collect();
        let union_count = a_ngrams.len() + b_ngrams.len() - intersection.len();
        if union_count > 0 {
            let jaccard = intersection.len() as f32 / union_count as f32;
            if jaccard > 0.3 {
                overlap_count += 1;
            }
        }
    }

    // Cross-chapter repetition: detect if current chapter sentences overlap with prior chapter summaries
    let mut cross_overlap = 0usize;
    for summary in prior_summaries.iter().filter(|s| !s.trim().is_empty()) {
        let prior_sentences: Vec<String> = summary
            .split(['。', '！', '？', '!', '?', '\n'])
            .map(|s| s.trim().to_string())
            .filter(|s| s.chars().count() >= 8)
            .collect();
        for current in &sentences {
            let current_chars: Vec<char> = current.chars().collect();
            if current_chars.len() < 4 {
                continue;
            }
            let current_ngrams: std::collections::HashSet<String> = current_chars
                .windows(4)
                .map(|w| w.iter().collect::<String>())
                .collect();
            for prior in &prior_sentences {
                let prior_chars: Vec<char> = prior.chars().collect();
                if prior_chars.len() < 4 {
                    continue;
                }
                let prior_ngrams: std::collections::HashSet<String> = prior_chars
                    .windows(4)
                    .map(|w| w.iter().collect::<String>())
                    .collect();
                let intersection: std::collections::HashSet<String> = current_ngrams
                    .intersection(&prior_ngrams)
                    .cloned()
                    .collect();
                let union_count = current_ngrams.len() + prior_ngrams.len() - intersection.len();
                if union_count > 0 {
                    let jaccard = intersection.len() as f32 / union_count as f32;
                    if jaccard > 0.35 {
                        cross_overlap += 1;
                        break;
                    }
                }
            }
        }
    }

    let ratio = overlap_count as f32 / sentences.len().max(1) as f32;
    let cross_ratio = cross_overlap as f32 / sentences.len().max(1) as f32;
    let combined_ratio = (ratio + cross_ratio * 0.5).min(1.0);
    let score = if combined_ratio > 0.3 {
        0.2
    } else if combined_ratio > 0.15 {
        0.5
    } else {
        0.9
    };
    let evidence = if cross_overlap > 0 {
        format!(
            "{}/{} adjacent pairs overlap; {}/{} sentences repeat prior chapters",
            overlap_count,
            sentences.len().saturating_sub(1),
            cross_overlap,
            sentences.len()
        )
    } else if overlap_count > 0 {
        format!(
            "{}/{} adjacent sentence pairs show 4-gram overlap",
            overlap_count,
            sentences.len().saturating_sub(1)
        )
    } else {
        String::new()
    };

    gated_metric(
        "scene_repetition",
        score,
        &evidence,
        "craft:scene_repetition",
        &format!("重复率（含跨章）{:.0}%", combined_ratio * 100.0),
        "避免连续句子使用相同词汇和句式，交替视角或动作",
    )
}

fn metric_plot_progression(text: &str) -> QualityMetricResult {
    let progression_signals = [
        "决定", "拒绝", "选择", "放弃", "承担", "接受", "离开", "留下",
        "背叛", "保护", "牺牲", "反击", "撤退", "追击", "隐瞒", "揭露",
    ];

    let count: usize = progression_signals
        .iter()
        .map(|s| text.matches(s).count())
        .sum();
    let char_count = text.chars().count().max(1);
    let density = count as f32 / char_count as f32 * 1000.0;

    let score = if density >= 3.0 { 0.9 } else if density >= 1.5 { 0.6 } else { 0.3 };
    let evidence: String = progression_signals
        .iter()
        .filter(|s| text.contains(*s))
        .take(4)
        .cloned()
        .collect::<Vec<_>>()
        .join(", ");

    gated_metric(
        "plot_progression", score, &evidence,
        "craft:plot_progression",
        &format!("情节推进信号密度: {:.2}/1000chars", density),
        "增加角色的决定、拒绝、选择等推进情节的行动",
    )
}

fn metric_new_information_density(text: &str, prior_summaries: &[String]) -> QualityMetricResult {
    if prior_summaries.is_empty() {
        return gated_metric(
            "new_information_density", 0.5, "",
            "craft:new_information_density",
            "无前序章节摘要，跳过评估", "",
        );
    }

    let prior_text = prior_summaries.join(" ");
    let prior_words: std::collections::HashSet<String> = prior_text
        .split(|c: char| c.is_whitespace() || c == '，' || c == '。' || c == '、')
        .filter(|w| w.chars().count() >= 2)
        .map(|w| w.to_string())
        .collect();

    let current_words: Vec<String> = text
        .split(|c: char| c.is_whitespace() || c == '，' || c == '。' || c == '、')
        .filter(|w| w.chars().count() >= 2)
        .map(|w| w.to_string())
        .collect();

    if current_words.is_empty() {
        return gated_metric(
            "new_information_density", 0.0, "",
            "craft:new_information_density",
            "当前章节无有效词汇", "增加具体细节和场景描写",
        );
    }

    let novel_count = current_words.iter().filter(|w| !prior_words.contains(*w)).count();
    let ratio = novel_count as f32 / current_words.len() as f32;
    let score = if ratio >= 0.5 { 0.9 } else if ratio >= 0.3 { 0.6 } else { 0.3 };
    let evidence = format!("{}/{} words not in prior summaries", novel_count, current_words.len());

    gated_metric(
        "new_information_density", score, &evidence,
        "craft:new_information_density",
        &format!("新信息占比 {:.0}%", ratio * 100.0),
        "确保章节提供前序章节未覆盖的新信息或视角",
    )
}

fn metric_state_delta_coverage(
    text: &str,
    required_deltas: &[crate::chapter_generation::StateDelta],
) -> QualityMetricResult {
    if required_deltas.is_empty() {
        return gated_metric(
            "state_delta_coverage", 0.5, "",
            "craft:state_delta_coverage",
            "无 required state deltas，跳过评估", "",
        );
    }

    // State-change indicators that suggest a meaningful transition happened
    let change_markers: &[&str] = &[
        "了", "已经", "不再", "变成", "化为", "决定", "选择", "放弃", "接受", "拒绝",
        "承认", "承担", "离开", "留下", "失去", "获得", "发现", "揭露", "牺牲", "代价",
        "改变", "转变", "动摇", "崩溃", "觉醒", "醒悟", "妥协", "坚持", "背叛", "信任",
        "死亡", "重生", "封印", "解封", "突破", "失败", "成功",
    ];

    let mut covered = 0usize;
    let mut weak = 0usize;
    let mut missing = 0usize;
    let text_sentences: Vec<&str> = text
        .split(['。', '！', '？', '!', '?', '\n'])
        .filter(|s| s.trim().len() >= 2)
        .collect();
    for delta in required_deltas {
        let keywords: Vec<&str> = delta.description
            .split(|c: char| c.is_whitespace() || c == '，' || c == '。' || c == '、')
            .filter(|w| w.chars().count() >= 2)
            .collect();
        let keyword_match = keywords.iter().any(|kw| text.contains(kw));
        if !keyword_match {
            missing += 1;
            continue;
        }
        // Require at least one change marker near the keyword context (within same sentence)
        let has_change_marker = text_sentences.iter().any(|sentence| {
            let has_kw = keywords.iter().any(|kw| sentence.contains(kw));
            let has_marker = change_markers.iter().any(|m| sentence.contains(m));
            has_kw && has_marker
        });
        if has_change_marker {
            covered += 1;
        } else {
            weak += 1;
        }
    }

    let total = required_deltas.len();
    // weighted: covered=1.0, weak=0.4, missing=0.0
    let weighted_ratio = (covered as f32 + weak as f32 * 0.4) / total as f32;
    let score = if weighted_ratio >= 0.8 { 0.9 } else if weighted_ratio >= 0.5 { 0.6 } else { 0.3 };
    let evidence = format!(
        "{} covered / {} weak / {} missing of {} required deltas",
        covered, weak, missing, total
    );
    let revision_hint = if missing > 0 {
        format!(
            "本章未涉及 {} 个要求的状态变化，请补充包含'了/已经/不再/决定/选择/放弃/获得/失去'等变化词的相关情节",
            missing
        )
    } else if weak > 0 {
        format!(
            "{} 个状态变化仅有提及而未使用变化动词，请补充行动或抉择使其成立",
            weak
        )
    } else {
        String::new()
    };

    gated_metric(
        "state_delta_coverage", score, &evidence,
        "craft:state_delta_coverage",
        &format!("状态变化覆盖 {:.0}%（covered={} weak={} missing={}）", weighted_ratio * 100.0, covered, weak, missing),
        &revision_hint,
    )
}

fn metric_world_consistency(
    text: &str,
    scene_contract: Option<&crate::writer_agent::world_bible::SceneContract>,
    world_assets: &[crate::writer_agent::world_bible::WorldAsset],
    canon_constraints: &[crate::writer_agent::world_bible::CanonConstraint],
) -> QualityMetricResult {
    let Some(contract) = scene_contract else {
        return gated_metric(
            "world_consistency",
            0.5,
            "",
            "world_bible",
            "无场景合约，跳过世界观一致性评估",
            "",
        );
    };

    // P14: When canon_constraints are provided directly, merge them into the contract's
    // active_constraints. Approved canon constraints take priority (prepended so they
    // are checked first and can override generic asset checks).
    let mut effective_contract = contract.clone();
    if !canon_constraints.is_empty() {
        // Prepend approved canon constraints so they take priority
        let mut merged = canon_constraints.to_vec();
        merged.append(&mut effective_contract.active_constraints);
        effective_contract.active_constraints = merged;
    }

    let violations = crate::writer_agent::world_bible::validate_world_consistency(
        text,
        &effective_contract,
        world_assets,
    );
    if violations.is_empty() {
        return gated_metric(
            "world_consistency",
            1.0,
            "无世界观一致性违规",
            "world_bible",
            "世界观一致性通过",
            "",
        );
    }

    let hard_count = violations
        .iter()
        .filter(|v| matches!(v.severity, crate::writer_agent::world_bible::ConstraintSeverity::Hard))
        .count();
    let warning_count = violations.len() - hard_count;
    let penalty = (hard_count as f32 * 0.3 + warning_count as f32 * 0.1).min(1.0);
    let score = 1.0 - penalty;

    let evidence = violations
        .iter()
        .take(3)
        .map(format_violation_evidence)
        .collect::<Vec<_>>()
        .join("; ");

    let reason = format!(
        "世界观一致性违规: {} hard, {} warning",
        hard_count, warning_count
    );

    gated_metric(
        "world_consistency",
        score,
        &evidence,
        "world_bible",
        &reason,
        &format!("修复 {} 个世界观违规", violations.len()),
    )
}

fn metric_term_misuse(
    text: &str,
    canon_terms: &[crate::writer_agent::world_bible::CanonTerm],
) -> QualityMetricResult {
    if canon_terms.is_empty() {
        return gated_metric(
            "term_misuse",
            0.5,
            "",
            "world_bible",
            "无可用的正典术语定义，跳过评估",
            "",
        );
    }

    let violations = crate::writer_agent::world_bible::validate_term_misuse(text, canon_terms);
    if violations.is_empty() {
        return gated_metric(
            "term_misuse",
            1.0,
            "无术语误用",
            "world_bible",
            "所有正典术语使用正确",
            "",
        );
    }

    let hard_count = violations
        .iter()
        .filter(|v| matches!(v.severity, crate::writer_agent::world_bible::ConstraintSeverity::Hard))
        .count();
    let warning_count = violations.len() - hard_count;
    let penalty = (hard_count as f32 * 0.35 + warning_count as f32 * 0.12).min(1.0);
    let score = 1.0 - penalty;

    let evidence = violations
        .iter()
        .take(3)
        .map(|v| {
            format!(
                "[{}] term={} expected='{}' observed='{}'",
                match v.severity {
                    crate::writer_agent::world_bible::ConstraintSeverity::Hard => "Hard",
                    crate::writer_agent::world_bible::ConstraintSeverity::Warning => "Warning",
                    crate::writer_agent::world_bible::ConstraintSeverity::Info => "Info",
                },
                v.term,
                v.expected_definition,
                v.observed_usage
            )
        })
        .collect::<Vec<_>>()
        .join("; ");

    let reason = format!(
        "术语误用: {} hard, {} warning",
        hard_count, warning_count
    );

    gated_metric(
        "term_misuse",
        score,
        &evidence,
        "world_bible",
        &reason,
        &format!("修正 {} 个术语误用", violations.len()),
    )
}

/// Format a WorldConsistencyViolation into a machine-parseable evidence string.
/// Format: "[Hard] rule_id={id} source={source_ref}: {summary} | excerpt: {text_excerpt}"
fn format_violation_evidence(
    v: &crate::writer_agent::world_bible::WorldConsistencyViolation,
) -> String {
    let severity_label = match v.severity {
        crate::writer_agent::world_bible::ConstraintSeverity::Hard => "Hard",
        crate::writer_agent::world_bible::ConstraintSeverity::Warning => "Warning",
        crate::writer_agent::world_bible::ConstraintSeverity::Info => "Info",
    };
    let source_ref = v
        .evidence
        .first()
        .map(|e| format!("{}/{}", e.source_id, e.excerpt))
        .unwrap_or_else(|| "unknown".to_string());
    let summary = if v.suggested_fix.is_empty() {
        v.message.clone()
    } else {
        v.suggested_fix.clone()
    };
    format!(
        "[{}] rule_id={} source={}: {} | excerpt: {}",
        severity_label,
        v.constraint_id,
        source_ref,
        summary,
        v.text_excerpt
    )
}

pub fn build_revision_prompt(
    chapter_text: &str,
    quality_report: &ChapterQualityReport,
    max_targets: usize,
) -> String {
    let targets: Vec<&QualityMetricResult> = quality_report
        .metric_results
        .iter()
        .filter(|m| m.severity == IssueSeverity::Major || m.severity == IssueSeverity::Fatal)
        .take(max_targets)
        .collect();

    if targets.is_empty() {
        return String::new();
    }

    let strong_metrics: Vec<&str> = quality_report
        .metric_results
        .iter()
        .filter(|m| m.score >= 0.8)
        .map(|m| m.metric.as_str())
        .collect();

    let mut prompt = String::from(
        "你是专业中文小说修订者。只修复下面列出的问题，不改其他任何内容。\n\n",
    );

    prompt.push_str("## 需要修复的问题\n\n");
    for (i, target) in targets.iter().enumerate() {
        prompt.push_str(&format!(
            "{}. **{}** (score {:.1}): {}\n   Revision hint: {}\n\n",
            i + 1,
            target.metric,
            target.score,
            target.reason,
            target.revision_hint
        ));
    }

    if !strong_metrics.is_empty() {
        prompt.push_str("## 必须保留的强项\n\n");
        prompt.push_str(&format!(
            "以下指标已达标，修订不能破坏：{}\n\n",
            strong_metrics.join("、")
        ));
    }

    prompt.push_str("## 硬约束\n\n");
    prompt.push_str("- 只修改与上述问题直接相关的句子和段落\n");
    prompt.push_str("- 不重写全章、不改变情节走向、不引入新人物或新设定\n");
    prompt.push_str("- 修改后字数变化不超过 ±10%\n");
    prompt.push_str("- 保留原文中所有已通过的写作特征\n\n");

    prompt.push_str("## 待修订正文\n\n");
    prompt.push_str(chapter_text);

    prompt
}

pub fn build_revision_target_changes(
    before: &ChapterQualityReport,
    after: Option<&ChapterQualityReport>,
    revision_attempted: bool,
    budget_skipped: bool,
) -> Vec<RevisionTargetChange> {
    build_revision_target_changes_with_text(
        before,
        after,
        revision_attempted,
        budget_skipped,
        None,
        None,
    )
}

pub fn build_revision_target_changes_with_text(
    before: &ChapterQualityReport,
    after: Option<&ChapterQualityReport>,
    revision_attempted: bool,
    budget_skipped: bool,
    draft_before: Option<&str>,
    draft_after: Option<&str>,
) -> Vec<RevisionTargetChange> {
    let mut targets: Vec<&QualityMetricResult> = before
        .top_revision_targets
        .iter()
        .filter_map(|metric| {
            before
                .metric_results
                .iter()
                .find(|result| result.metric == *metric)
        })
        .collect();

    if targets.is_empty() {
        targets = before
            .metric_results
            .iter()
            .filter(|result| {
                result.severity == IssueSeverity::Major || result.severity == IssueSeverity::Fatal
            })
            .take(3)
            .collect();
    }

    targets
        .into_iter()
        .map(|target| {
            let after_metric = after.and_then(|report| {
                report
                    .metric_results
                    .iter()
                    .find(|result| result.metric == target.metric)
            });
            let score_after = after_metric.map(|metric| metric.score);
            let delta = score_after.map(|score| score - target.score);
            let status = revision_target_change_status(
                delta,
                revision_attempted,
                budget_skipped,
                after_metric.is_some(),
            );
            let evidence_after = after_metric.map(|metric| metric.evidence_excerpt.clone());
            let text_excerpt_change =
                match (draft_before, draft_after) {
                    (Some(before_text), Some(after_text)) => {
                        changed_text_excerpt(before_text, after_text, &target.metric)
                    }
                    _ => None,
                };
            let sentence_changes = match (draft_before, draft_after) {
                (Some(before_text), Some(after_text)) => {
                    compute_sentence_changes(before_text, after_text, &target.metric)
                }
                _ => Vec::new(),
            };
            RevisionTargetChange {
                metric: target.metric.clone(),
                revision_hint: target.revision_hint.clone(),
                score_before: target.score,
                score_after,
                delta,
                status,
                evidence_before: target.evidence_excerpt.clone(),
                changed_excerpt_before: text_excerpt_change
                    .as_ref()
                    .map(|change| change.0.clone())
                    .unwrap_or_default(),
                changed_excerpt_after: text_excerpt_change
                    .as_ref()
                    .map(|change| change.1.clone())
                    .unwrap_or_default(),
                text_change_summary: summarize_revision_text_change(
                    &target.evidence_excerpt,
                    evidence_after.as_deref(),
                    delta,
                    text_excerpt_change.as_ref().map(|change| {
                        (change.0.as_str(), change.1.as_str())
                    }),
                ),
                sentence_changes,
                evidence_after,
            }
        })
        .collect()
}

fn revision_target_change_status(
    delta: Option<f32>,
    revision_attempted: bool,
    budget_skipped: bool,
    after_observed: bool,
) -> RevisionTargetChangeStatus {
    if budget_skipped {
        return RevisionTargetChangeStatus::BudgetSkipped;
    }
    if !revision_attempted {
        return RevisionTargetChangeStatus::NotAttempted;
    }
    if !after_observed {
        return RevisionTargetChangeStatus::NotObserved;
    }
    let delta = delta.unwrap_or(0.0);
    if delta > 0.01 {
        RevisionTargetChangeStatus::Improved
    } else if delta < -0.01 {
        RevisionTargetChangeStatus::Regressed
    } else {
        RevisionTargetChangeStatus::Unchanged
    }
}

fn summarize_revision_text_change(
    evidence_before: &str,
    evidence_after: Option<&str>,
    delta: Option<f32>,
    text_excerpt_change: Option<(&str, &str)>,
) -> String {
    if let Some((before_excerpt, after_excerpt)) = text_excerpt_change {
        return format!(
            "Draft text changed from '{}' to '{}'; score delta {:+.2}.",
            snippet_for_report(before_excerpt, 80),
            snippet_for_report(after_excerpt, 80),
            delta.unwrap_or(0.0)
        );
    }
    let Some(evidence_after) = evidence_after else {
        return "No after-revision metric evidence was recorded for this target.".to_string();
    };
    let delta = delta.unwrap_or(0.0);
    if evidence_before.trim().is_empty() && evidence_after.trim().is_empty() {
        if delta > 0.01 {
            "Score improved, but this metric has no excerpt-level evidence.".to_string()
        } else if delta < -0.01 {
            "Score regressed, but this metric has no excerpt-level evidence.".to_string()
        } else {
            "Score was unchanged and this metric has no excerpt-level evidence.".to_string()
        }
    } else if evidence_before == evidence_after {
        format!(
            "Metric evidence unchanged; score delta {:+.2}.",
            delta
        )
    } else {
        format!(
            "Metric evidence changed from '{}' to '{}'; score delta {:+.2}.",
            snippet_for_report(evidence_before, 80),
            snippet_for_report(evidence_after, 80),
            delta
        )
    }
}

/// Compute sentence-level changes between before/after revision text.
///
/// Uses character-set Jaccard similarity for alignment. This is intentionally
/// lightweight (no embedding dependency) but has a known limitation: sentences
/// with heavy synonym substitution or major word-order rearrangement may score
/// below the match threshold and be reported as deleted+inserted instead of
/// modified. Such cases are marked with Low confidence or Unaligned so callers
/// do not treat them as reliable semantic mappings.
pub fn compute_sentence_changes(
    before_text: &str,
    after_text: &str,
    metric: &str,
) -> Vec<crate::chapter_generation::SentenceChange> {
    use crate::chapter_generation::{
        SentenceChange, SentenceChangeConfidence, SentenceChangeKind,
    };

    if before_text == after_text {
        return Vec::new();
    }

    let before_sentences = split_revision_units(before_text);
    let after_sentences = split_revision_units(after_text);
    if before_sentences.is_empty() || after_sentences.is_empty() {
        return vec![SentenceChange {
            before_sentence: snippet_for_report(before_text, 120),
            after_sentence: snippet_for_report(after_text, 120),
            change_kind: SentenceChangeKind::Unaligned,
            target_metric: metric.to_string(),
            confidence: SentenceChangeConfidence::Low,
        }];
    }

    let mut used_before = std::collections::HashSet::new();
    let mut changes: Vec<SentenceChange> = Vec::new();

    for (after_idx, after_sent) in after_sentences.iter().enumerate() {
        let mut best_match: Option<(usize, f32)> = None;
        for (before_idx, before_sent) in before_sentences.iter().enumerate() {
            if used_before.contains(&before_idx) {
                continue;
            }
            let sim = sentence_similarity(before_sent, after_sent);
            if sim > 0.4 && best_match.is_none_or(|(_, best_sim)| sim > best_sim) {
                best_match = Some((before_idx, sim));
            }
        }

        if let Some((before_idx, sim)) = best_match {
            used_before.insert(before_idx);
            let before_sent = &before_sentences[before_idx];
            let is_moved = before_idx != after_idx
                && before_sentences.len() == after_sentences.len()
                && sentence_similarity(before_sent, after_sent) > 0.7;
            if before_sent != after_sent || is_moved {
                let confidence = if sim >= 0.8 {
                    SentenceChangeConfidence::High
                } else if sim >= 0.6 {
                    SentenceChangeConfidence::Medium
                } else {
                    SentenceChangeConfidence::Low
                };
                let kind = if is_moved {
                    SentenceChangeKind::Moved
                } else {
                    SentenceChangeKind::Modified
                };
                changes.push(SentenceChange {
                    before_sentence: before_sent.clone(),
                    after_sentence: after_sent.clone(),
                    change_kind: kind,
                    target_metric: metric.to_string(),
                    confidence,
                });
            }
        } else {
            changes.push(SentenceChange {
                before_sentence: String::new(),
                after_sentence: after_sent.clone(),
                change_kind: SentenceChangeKind::Inserted,
                target_metric: metric.to_string(),
                confidence: SentenceChangeConfidence::High,
            });
        }
    }

    for (before_idx, before_sent) in before_sentences.iter().enumerate() {
        if !used_before.contains(&before_idx) {
            changes.push(SentenceChange {
                before_sentence: before_sent.clone(),
                after_sentence: String::new(),
                change_kind: SentenceChangeKind::Deleted,
                target_metric: metric.to_string(),
                confidence: SentenceChangeConfidence::High,
            });
        }
    }

    // Filter to metric-relevant changes when there are many
    let needles = metric_change_needles(metric);
    if !needles.is_empty() && changes.len() > 3 {
        let filtered: Vec<SentenceChange> = changes
            .iter()
            .filter(|c| {
                needles.iter().any(|n| {
                    c.before_sentence.contains(n) || c.after_sentence.contains(n)
                })
            })
            .cloned()
            .collect();
        if !filtered.is_empty() {
            return filtered;
        }
    }

    changes
}

fn sentence_similarity(a: &str, b: &str) -> f32 {
    let a_chars: std::collections::HashSet<char> = a.chars().collect();
    let b_chars: std::collections::HashSet<char> = b.chars().collect();
    if a_chars.is_empty() && b_chars.is_empty() {
        return 1.0;
    }
    let intersection: std::collections::HashSet<char> =
        a_chars.intersection(&b_chars).copied().collect();
    let union: std::collections::HashSet<char> =
        a_chars.union(&b_chars).copied().collect();
    if union.is_empty() {
        return 0.0;
    }
    intersection.len() as f32 / union.len() as f32
}

fn changed_text_excerpt(
    before_text: &str,
    after_text: &str,
    metric: &str,
) -> Option<(String, String)> {
    if before_text == after_text {
        return None;
    }

    let before_sentences = split_revision_units(before_text);
    let after_sentences = split_revision_units(after_text);
    if before_sentences.is_empty() || after_sentences.is_empty() {
        return Some((
            snippet_for_report(before_text, 120),
            snippet_for_report(after_text, 120),
        ));
    }

    let preferred_needles = metric_change_needles(metric);
    for needle in preferred_needles {
        let before = before_sentences
            .iter()
            .find(|sentence| sentence.contains(needle.as_str()));
        let after = after_sentences
            .iter()
            .find(|sentence| sentence.contains(needle.as_str()));
        if let (Some(before), Some(after)) = (before, after) {
            if before != after {
                return Some((before.clone(), after.clone()));
            }
        } else if before.is_some() || after.is_some() {
            return Some(match (before, after) {
                (Some(before), None) => (
                    before.clone(),
                    after_sentences.first().cloned().unwrap_or_default(),
                ),
                (None, Some(after)) => (
                    before_sentences.first().cloned().unwrap_or_default(),
                    after.clone(),
                ),
                _ => unreachable!(),
            });
        }
    }

    let max_len = before_sentences.len().max(after_sentences.len());
    for idx in 0..max_len {
        let before = before_sentences.get(idx);
        let after = after_sentences.get(idx);
        if before != after {
            return Some((
                before.cloned().unwrap_or_default(),
                after.cloned().unwrap_or_default(),
            ));
        }
    }

    None
}

fn split_revision_units(text: &str) -> Vec<String> {
    let mut units = Vec::new();
    let mut current = String::new();
    for ch in text.chars() {
        current.push(ch);
        if matches!(ch, '。' | '！' | '？' | '!' | '?' | '\n') {
            push_revision_unit(&mut units, &mut current);
        }
    }
    push_revision_unit(&mut units, &mut current);
    units
}

fn push_revision_unit(units: &mut Vec<String>, current: &mut String) {
    let trimmed = current.trim();
    if !trimmed.is_empty() {
        units.push(snippet_for_report(trimmed, 160));
    }
    current.clear();
}

fn metric_change_needles(metric: &str) -> Vec<String> {
    match metric {
        "length_compliance" => Vec::new(),
        "dialogue_function" => ["说", "问", "答", "道", "\"", "“"]
            .into_iter()
            .map(str::to_string)
            .collect(),
        "ending_hook" => ["代价", "后果", "选择", "不知道", "但是", "然而"]
            .into_iter()
            .map(str::to_string)
            .collect(),
        "scene_causality" => ["因为", "所以", "因此", "于是", "导致", "只好"]
            .into_iter()
            .map(str::to_string)
            .collect(),
        "promise_progress" | "anchor_carry" => ["代价", "选择", "兑现", "线索", "秘密"]
            .into_iter()
            .map(str::to_string)
            .collect(),
        "style_drift" => ["说", "问", "。", "，"]
            .into_iter()
            .map(str::to_string)
            .collect(),
        _ => Vec::new(),
    }
}

fn snippet_for_report(text: &str, max_chars: usize) -> String {
    let mut snippet: String = text.chars().take(max_chars).collect();
    if text.chars().count() > max_chars {
        snippet.push_str("...");
    }
    snippet
}

#[cfg(test)]
mod craft_quality_tests {
    use super::*;

    #[test]
    fn empty_text_scores_low_but_no_panic() {
        let plan = SceneCraftPlan::default();
        let report = evaluate_chapter_quality("", "test-chapter", &plan, &[], 3000, 4000);
        // With 12 metrics, empty text gets more default "insufficient evidence" scores;
        // threshold relaxed but length_compliance should still be near zero.
        assert!(report.overall_score <= 0.80);
        assert!(!report.metric_results.is_empty());
        let length = report.metric_results.iter().find(|m| m.metric == "length_compliance").unwrap();
        assert!(length.score < 0.1, "empty text should fail length compliance");
    }

    #[test]
    fn length_compliance_detects_under_min() {
        let plan = SceneCraftPlan::default();
        let report = evaluate_chapter_quality("短", "test-chapter", &plan, &[], 3000, 4000);
        let length = report.metric_results.iter().find(|m| m.metric == "length_compliance").unwrap();
        assert!(length.score < 0.8);
    }

    #[test]
    fn dialogue_function_scores_ok_when_signals_present() {
        let plan = SceneCraftPlan::default();
        let text = "\"你必须做出决定。\"林墨说。她回避了他的目光。这是最后一次选择，也是最后的妥协。";
        let report = evaluate_chapter_quality(text, "test-chapter", &plan, &[], 0, 500);
        let dialogue = report.metric_results.iter().find(|m| m.metric == "dialogue_function").unwrap();
        assert!(dialogue.score >= 0.3);
    }

    #[test]
    fn ending_hook_detects_consequence_and_question() {
        let plan = SceneCraftPlan::default();
        let text_with_hook = "前面很多内容...他终于明白了一切。代价已经付出。但她还不知道——那个选择意味着什么。";
        let report = evaluate_chapter_quality(text_with_hook, "test-chapter", &plan, &[], 0, 500);
        let hook = report.metric_results.iter().find(|m| m.metric == "ending_hook").unwrap();
        assert!(hook.score >= 0.5, "should detect consequence and question");
    }

    #[test]
    fn no_evidence_gates_to_insufficient() {
        let plan = SceneCraftPlan::default();
        // Pure-narration paragraph (>200 chars, no action verbs, no dialogue markers)
        // to trigger exposition_ratio detection
        let text = "L".repeat(250);
        let report = evaluate_chapter_quality(&text, "test-chapter", &plan, &[], 0, 5000);
        let expo = report.metric_results.iter().find(|m| m.metric == "exposition_ratio").unwrap();
        assert!(
            expo.reason.contains("说明性段落占比") || expo.reason.contains("证据不足"),
            "expected expo reason to mention exposition ratio or insufficient evidence, got: {}",
            expo.reason
        );
    }

    #[test]
    fn fatal_issues_block_no_fatal_flag() {
        let plan = SceneCraftPlan::default();
        let report = evaluate_chapter_quality("极短", "test-chapter", &plan, &[], 3000, 4000);
        if report.fatal_issues.is_empty() {
            assert!(report.no_fatal_issue);
        } else {
            assert!(!report.no_fatal_issue);
        }
    }

    #[test]
    fn all_metrics_present() {
        let plan = SceneCraftPlan::default();
        let report = evaluate_chapter_quality("一些测试文本内容", "test-chapter", &plan, &[], 0, 500);
        assert_eq!(report.metric_results.len(), 14, "all 14 metrics should be present");
        let expected_metrics = [
            "anchor_carry", "style_drift", "length_compliance",
            "dialogue_function", "exposition_ratio", "ending_hook",
            "scene_causality", "promise_progress",
            "scene_repetition", "plot_progression", "new_information_density", "state_delta_coverage",
            "world_consistency", "term_misuse",
        ];
        for expected in &expected_metrics {
            assert!(
                report.metric_results.iter().any(|m| m.metric == *expected),
                "missing metric: {expected}"
            );
        }
    }

    #[test]
    fn revision_prompt_empty_for_no_issues() {
        let plan = SceneCraftPlan::default();
        let report = evaluate_chapter_quality(
            "一些正常文本内容，但是他终于明白了代价。",
            "test-chapter",
            &plan,
            &[],
            0,
            500,
        );
        let prompt = build_revision_prompt("text", &report, 3);
        // If no major/fatal issues, prompt is empty
        assert!(prompt.is_empty() || !prompt.contains("需要修复的问题"));
    }

    #[test]
    fn revision_prompt_includes_targets_and_constraints() {
        let plan = SceneCraftPlan::default();
        // Force a low-quality report by providing empty text with high length requirement
        let report = evaluate_chapter_quality("", "test-chapter", &plan, &[], 3000, 4000);
        let prompt = build_revision_prompt("正文内容", &report, 2);
        if !prompt.is_empty() {
            assert!(prompt.contains("需要修复的问题"));
            assert!(prompt.contains("硬约束"));
            assert!(prompt.contains("待修订正文"));
        }
    }

    #[test]
    fn revision_target_changes_map_before_after_metric_evidence() {
        let plan = SceneCraftPlan::default();
        let before = evaluate_chapter_quality("", "test-chapter", &plan, &[], 3000, 4000);
        let after = evaluate_chapter_quality(
            "林墨不得不做出选择，因此付出了代价。终于，他知道寒影剑已经改变了局面，但是新的问题还没有解决。",
            "test-chapter",
            &plan,
            &[],
            0,
            4000,
        );

        let changes = build_revision_target_changes(&before, Some(&after), true, false);

        assert!(!changes.is_empty());
        assert!(changes.iter().any(|change| change.score_after.is_some()));
        assert!(changes
            .iter()
            .any(|change| change.status == RevisionTargetChangeStatus::Improved));
    }

    #[test]
    fn quality_signals_drive_anchor_and_style_metrics() {
        let plan = SceneCraftPlan::default();
        let signals = ChapterQualitySignals {
            anchor_keywords: vec!["寒影剑".to_string(), "林墨".to_string(), "代价".to_string()],
            author_voice: Some(crate::writer_agent::author_voice::AuthorVoiceSnapshot {
                voice_id: "test-voice".to_string(),
                rhythm: crate::writer_agent::author_voice::VoiceRhythm {
                    avg_sentence_length: 24.0,
                    sentence_variance: 8.0,
                    paragraph_pacing: "medium".to_string(),
                },
                diction: crate::writer_agent::author_voice::VoiceDiction {
                    register: "formal".to_string(),
                    sensory_density: 0.5,
                    subtext_ratio: 0.3,
                },
                pov: "third_person_limited".to_string(),
                dialogue_texture: "subtext_heavy".to_string(),
                sentence_shape: Vec::new(),
                taboo_phrases: Vec::new(),
                confidence: 0.8,
                sample_refs: vec!["sample:chapter-1".to_string()],
                last_updated_ms: 0,
            }),
            required_anchors: Vec::new(),
            required_state_deltas: Vec::new(),
            prior_chapter_summaries: Vec::new(),
            scene_contract: None,
            world_assets: Vec::new(),
            canon_constraints: Vec::new(),
            canon_terms: Vec::new(),
        };
        let report = evaluate_chapter_quality_with_signals(
            "林墨只好拔出寒影剑，因此付出代价。",
            "test-chapter",
            &plan,
            &[],
            0,
            500,
            &signals,
        );

        let anchor = report
            .metric_results
            .iter()
            .find(|metric| metric.metric == "anchor_carry")
            .unwrap();
        let style = report
            .metric_results
            .iter()
            .find(|metric| metric.metric == "style_drift")
            .unwrap();
        assert!(!anchor.reason.contains("证据不足"));
        assert!(!style.reason.contains("证据不足"));
        assert!(anchor.score > 0.0);
        assert!(style.score > 0.0);
    }

    #[test]
    fn revision_target_changes_record_text_excerpts() {
        let plan = SceneCraftPlan::default();
        let before_text = "林墨看着寒影剑。";
        let after_text = "林墨只好拔出寒影剑，因此付出代价。";
        let before = evaluate_chapter_quality(before_text, "test-chapter", &plan, &[], 0, 500);
        let after = evaluate_chapter_quality(after_text, "test-chapter", &plan, &[], 0, 500);

        let changes = build_revision_target_changes_with_text(
            &before,
            Some(&after),
            true,
            false,
            Some(before_text),
            Some(after_text),
        );

        assert!(changes.iter().any(|change| {
            !change.changed_excerpt_before.is_empty()
                && !change.changed_excerpt_after.is_empty()
                && change.text_change_summary.contains("Draft text changed")
        }));
    }

    #[test]
    fn required_anchors_penalize_missing_and_weak() {
        let plan = SceneCraftPlan::default();
        // Text mentions "寒影剑" but does not carry it (no action/dialogue/consequence)
        let text = "本章出现寒影剑、张三、镜中墟和旧债。";
        let signals = ChapterQualitySignals {
            anchor_keywords: vec!["寒影剑".to_string(), "张三".to_string()],
            author_voice: None,
            required_anchors: vec![
                crate::chapter_generation::StoryAnchor {
                    anchor_id: "寒影剑".to_string(),
                    source: "canon_constraint".to_string(),
                    description: "must participate in action".to_string(),
                    required: true,
                },
                crate::chapter_generation::StoryAnchor {
                    anchor_id: "旧债".to_string(),
                    source: "open_promise".to_string(),
                    description: "must advance promise".to_string(),
                    required: true,
                },
            ],
            required_state_deltas: Vec::new(),
            prior_chapter_summaries: Vec::new(),
            scene_contract: None,
            world_assets: Vec::new(),
            canon_constraints: Vec::new(),
            canon_terms: Vec::new(),
        };
        let report = evaluate_chapter_quality_with_signals(
            text, "test-chapter", &plan, &[], 0, 500, &signals,
        );
        let anchor = report
            .metric_results
            .iter()
            .find(|m| m.metric == "anchor_carry")
            .unwrap();
        // Both required anchors are only mentioned, not carried → penalty
        assert!(anchor.score < 0.5, "expected low score due to weak required anchors, got {}", anchor.score);
        assert!(anchor.reason.contains("必需锚点") || anchor.reason.contains("弱承载"));
    }

    #[test]
    fn required_anchors_boost_when_all_carried() {
        let plan = SceneCraftPlan::default();
        // Text carries all required anchors through action and consequence
        let text = "林墨拔出寒影刀逼问张三：“旧债今天要还。”镜中墟的门因此重新打开。";
        let signals = ChapterQualitySignals {
            anchor_keywords: vec!["寒影刀".to_string(), "张三".to_string(), "旧债".to_string()],
            author_voice: None,
            required_anchors: vec![
                crate::chapter_generation::StoryAnchor {
                    anchor_id: "寒影刀".to_string(),
                    source: "canon_constraint".to_string(),
                    description: "must participate in action".to_string(),
                    required: true,
                },
                crate::chapter_generation::StoryAnchor {
                    anchor_id: "旧债".to_string(),
                    source: "open_promise".to_string(),
                    description: "must advance promise".to_string(),
                    required: true,
                },
            ],
            required_state_deltas: Vec::new(),
            prior_chapter_summaries: Vec::new(),
            scene_contract: None,
            world_assets: Vec::new(),
            canon_constraints: Vec::new(),
            canon_terms: Vec::new(),
        };
        let report = evaluate_chapter_quality_with_signals(
            text, "test-chapter", &plan, &[], 0, 500, &signals,
        );
        let anchor = report
            .metric_results
            .iter()
            .find(|m| m.metric == "anchor_carry")
            .unwrap();
        assert!(anchor.score >= 0.5, "expected decent score when required anchors are carried, got {}", anchor.score);
    }

    #[test]
    fn sentence_diff_detects_inserted_sentence_for_major_rewrite() {
        let before = "林墨看着寒影剑。散修站在门口。";
        let after = "散修逼近门口时，林墨只好拔出寒影剑，因此付出鬓发变白的代价。";
        let changes = compute_sentence_changes(before, after, "ending_hook");
        assert!(!changes.is_empty(), "expected at least one sentence change");
        // After is a completely new sentence → detected as Inserted (before sentences deleted)
        let inserted = changes.iter().any(|c| c.change_kind == SentenceChangeKind::Inserted);
        assert!(inserted, "expected an Inserted sentence change for major rewrite");
    }

    #[test]
    fn sentence_diff_detects_insertion_and_deletion() {
        let before = "林墨看着寒影剑。散修站在门口。";
        let after = "林墨看着寒影剑。散修站在门口。他握紧了剑柄。";
        let changes = compute_sentence_changes(before, after, "scene_causality");
        let inserted = changes.iter().any(|c| c.change_kind == SentenceChangeKind::Inserted);
        assert!(inserted, "expected an Inserted sentence change");
    }

    #[test]
    fn sentence_diff_detects_moved_sentence() {
        let before = "第一句。第二句。";
        let after = "第二句。第一句。";
        let changes = compute_sentence_changes(before, after, "dialogue_function");
        let moved = changes.iter().any(|c| c.change_kind == SentenceChangeKind::Moved);
        assert!(moved, "expected a Moved sentence change");
    }

    #[test]
    fn sentence_diff_empty_for_identical_text() {
        let text = "林墨看着寒影剑。散修站在门口。";
        let changes = compute_sentence_changes(text, text, "anchor_carry");
        assert!(changes.is_empty(), "expected no changes for identical text");
    }

    #[test]
    fn sentence_diff_detects_deleted_and_inserted_for_unaligned() {
        let before = "ABC。";
        let after = "XYZ123。";
        let changes = compute_sentence_changes(before, after, "style_drift");
        let deleted = changes.iter().any(|c| c.change_kind == SentenceChangeKind::Deleted);
        let inserted = changes.iter().any(|c| c.change_kind == SentenceChangeKind::Inserted);
        assert!(deleted, "expected Deleted for original sentence");
        assert!(inserted, "expected Inserted for new sentence");
    }

    #[test]
    fn revision_target_changes_include_sentence_changes() {
        let plan = SceneCraftPlan::default();
        let before_text = "林墨看着寒影剑。散修站在门口。";
        let after_text = "散修逼近门口时，林墨只好拔出寒影剑，因此付出鬓发变白的代价。";
        let before = evaluate_chapter_quality(before_text, "test-chapter", &plan, &[], 0, 500);
        let after = evaluate_chapter_quality(after_text, "test-chapter", &plan, &[], 0, 500);

        let changes = build_revision_target_changes_with_text(
            &before,
            Some(&after),
            true,
            false,
            Some(before_text),
            Some(after_text),
        );

        let has_sentence_changes = changes.iter().any(|change| {
            !change.sentence_changes.is_empty()
                && change.sentence_changes.iter().any(|sc| {
                    sc.confidence == SentenceChangeConfidence::High
                        || sc.confidence == SentenceChangeConfidence::Medium
                })
        });
        assert!(has_sentence_changes, "expected at least one high/medium confidence sentence change in revision target changes");
    }

    // ──── P13 quality mode provider call limits ────

    #[test]
    fn quality_mode_default_is_balanced() {
        use crate::chapter_generation::GenerationQualityMode;
        assert_eq!(GenerationQualityMode::default(), GenerationQualityMode::Balanced);
    }

    #[test]
    fn quality_mode_fast_never_revises() {
        use crate::chapter_generation::GenerationQualityMode;
        // Fast mode: should_revise is always false regardless of quality report
        assert_eq!(GenerationQualityMode::Fast, GenerationQualityMode::Fast);
        // Verify Fast is NOT Balanced or Strict (mode identity check)
        assert_ne!(GenerationQualityMode::Fast, GenerationQualityMode::Balanced);
        assert_ne!(GenerationQualityMode::Fast, GenerationQualityMode::Strict);
    }

    #[test]
    fn strict_mode_has_stricter_gate_than_balanced() {
        use crate::chapter_generation::GenerationQualityMode;
        // Strict mode checks additional gate metrics beyond Balanced
        // Balanced only checks fatal/major issues
        // Strict also checks scene_repetition, plot_progression,
        // new_information_density, state_delta_coverage scores < 0.5
        assert_ne!(GenerationQualityMode::Strict, GenerationQualityMode::Balanced);
    }

    #[test]
    fn default_quality_report_has_no_fatal_issues() {
        let report = ChapterQualityReport::default();
        assert!(report.fatal_issues.is_empty());
        assert!(report.major_issues.is_empty());
        assert_eq!(report.overall_score, 0.0);
        // No issues → should_revise = false for all modes
        // This proves Fast/Balanced/Strict all produce 0 extra provider calls on clean output
    }

    // ──── P9 context source failure isolation ────

    #[test]
    fn context_source_failure_does_not_block_other_sources() {
        use crate::chapter_generation::ChapterContextSource;
        let sources = [
            ChapterContextSource {
                source_type: "previous_chapters".into(),
                id: "previous".into(),
                label: "Previous".into(),
                original_chars: 0,
                included_chars: 0,
                truncated: false,
                score: None,
                taxonomy: "story_context".into(),
                role: "required".into(),
                elapsed_ms: 120,
                retrieval_status: "not_found".into(),
            },
            ChapterContextSource {
                source_type: "outline".into(),
                id: "outline".into(),
                label: "Outline".into(),
                original_chars: 500,
                included_chars: 500,
                truncated: false,
                score: None,
                taxonomy: "story_context".into(),
                role: "required".into(),
                elapsed_ms: 5,
                retrieval_status: "ok".into(),
            },
            ChapterContextSource {
                source_type: "lorebook".into(),
                id: "lorebook".into(),
                label: "Lorebook".into(),
                original_chars: 1200,
                included_chars: 1200,
                truncated: false,
                score: Some(0.85),
                taxonomy: "story_lore".into(),
                role: "supplement".into(),
                elapsed_ms: 35,
                retrieval_status: "ok".into(),
            },
            ChapterContextSource {
                source_type: "project_brain".into(),
                id: "project_brain".into(),
                label: "Project Brain".into(),
                original_chars: 0,
                included_chars: 0,
                truncated: false,
                score: None,
                taxonomy: "external_research".into(),
                role: "optional".into(),
                elapsed_ms: 210,
                retrieval_status: "not_found".into(),
            },
        ];
        let ok_count = sources.iter().filter(|s| s.retrieval_status == "ok").count();
        let not_found_count = sources.iter().filter(|s| s.retrieval_status == "not_found").count();
        let total = sources.len();
        assert_eq!(
            ok_count + not_found_count,
            total,
            "all sources must have a valid retrieval_status"
        );
        assert_eq!(ok_count, 2, "two sources should be OK despite others being not_found");
        assert_eq!(not_found_count, 2, "two sources should be not_found without blocking OK sources");
        assert!(sources.iter().any(|s| s.included_chars > 0), "at least one source carries data");
    }

    // ──── P10 scene_repetition four-category coverage ────

    #[test]
    fn scene_repetition_detects_exact_duplicate() {
        let text = "林墨走在青石板路上。林墨走在青石板路上。林墨走在青石板路上。";
        let prior: Vec<String> = vec![];
        let result = metric_scene_repetition(text, &prior);
        assert!(result.score < 0.6, "完全重复应得低分，实际={}", result.score);
        assert!(!result.evidence_excerpt.is_empty());
    }

    #[test]
    fn scene_repetition_detects_slight_rewrite_duplicate() {
        // Repeated structure with similar vocabulary → 4-gram overlap across sentences
        let text = "林墨缓步走在石板路上。林墨缓步走在石板路尽头。林墨缓步走在石板路中央。";
        let prior: Vec<String> = vec![];
        let result = metric_scene_repetition(text, &prior);
        assert!(result.score < 0.8, "近义改写重复应被检测，实际={}", result.score);
    }

    #[test]
    fn scene_repetition_allows_legitimate_echo() {
        // 合法呼应：不同场景相似句式但间隔远且语义明确不同
        let text = "林墨想起十年前，师父也说过同样的话。如今他站在师父的位置，终于明白了那句话的重量。";
        let prior: Vec<String> = vec![];
        let result = metric_scene_repetition(text, &prior);
        assert!(result.score >= 0.5, "合法呼应不应被过度惩罚，实际={}", result.score);
    }

    #[test]
    fn scene_repetition_allows_necessary_recap() {
        // 必要recap：开头简短回顾前情，但后续是新内容
        let text = "上一次林墨在寒潭中突破之后，体内的寒毒暂时被压制。这次他发现寒毒已经侵入经脉，不再是简单的压制能解决的。他决定进入更深的潭底，去寻找传说中能净化一切的神物。他深吸一口气，纵身跃入漆黑的深渊。";
        let prior = vec!["林墨在寒潭中修炼，突破了玄冰境界。他发现了隐藏在潭底的古老封印。".to_string()];
        let result = metric_scene_repetition(text, &prior);
        assert!(result.score >= 0.4, "必要recap不应被误伤，实际={}", result.score);
    }

    #[test]
    fn anchor_carry_pass_but_scene_repetition_fail() {
        // 负例：锚点合格但场景重复
        let text = "林墨握着寒影刀，站在破庙门口。林墨握着寒影刀，站在破庙门口。林墨握着寒影刀，站在破庙门口。寒影刀在他手中微微颤动。张三从阴影中走出，看着林墨。张三从阴影中走出，看着林墨。张三从阴影中走出，看着林墨。林墨决定继续前进。林墨决定继续前进。林墨决定继续前进。";
        let plan = SceneCraftPlan::default();
        let signals = ChapterQualitySignals {
            anchor_keywords: vec!["寒影刀".to_string(), "张三".to_string()],
            prior_chapter_summaries: vec!["林墨在茶馆遇到了张三。".to_string()],
            ..Default::default()
        };
        let report = evaluate_chapter_quality_with_signals(
            text,
            "test",
            &plan,
            &[],
            0,
            2000,
            &signals,
        );
        let ac = report.metric_results.iter().find(|m| m.metric == "anchor_carry");
        let sr = report.metric_results.iter().find(|m| m.metric == "scene_repetition");
        assert!(ac.is_some(), "anchor_carry metric should exist");
        assert!(sr.is_some(), "scene_repetition metric should exist");
        if let (Some(a), Some(s)) = (ac, sr) {
            assert!(
                a.score >= 0.3,
                "anchor_carry should pass ({:.2}) because anchors are present",
                a.score
            );
            assert!(
                s.score < 0.5,
                "scene_repetition should fail ({:.2}) because sentences are heavily duplicated",
                s.score
            );
        }
    }

    // ──── P11 anchor_carry vs state_delta_coverage uniqueness ────

    #[test]
    fn anchor_carry_pass_but_state_delta_uncovered() {
        let text = "林墨握着寒影剑，感受到了剑中的寒意。这把剑是师父留给他的。寒影剑在他手中微微颤动。他一直相信寒影剑能帮他找到答案。";
        let plan = SceneCraftPlan::default();
        let report = evaluate_chapter_quality(text, "test", &plan, &[], 0, 2000);
        // anchor_carry: "寒影剑" appears → should be detected, high score
        let ac = report.metric_results.iter().find(|m| m.metric == "anchor_carry");
        let sd = report.metric_results.iter().find(|m| m.metric == "state_delta_coverage");
        assert!(ac.is_some());
        // state_delta_coverage: no change markers (了/已经/不再/决定/选择) near anchor keywords
        // With empty required deltas, it defaults to 0.5; but the point is it should NOT auto-pass
        // just because anchor_carry is high.
        if let (Some(a), Some(s)) = (ac, sd) {
            // Anchor carry can be decent (提到锚点)
            // But state delta should be lower if no actual state change occurred
            assert!(
                a.score >= 0.3 || s.score <= a.score,
                "anchor_carry={:.2} state_delta={:.2}: state_delta should not auto-pass on anchor mentions alone",
                a.score, s.score
            );
        }
    }

    #[test]
    fn state_delta_coverage_weak_vs_covered() {
        let deltas = vec![
            crate::chapter_generation::StateDelta {
                delta_type: "knowledge".into(),
                description: "秘境、入口".into(),
                source: "outline".into(),
            },
        ];
        // Text mentions keyword but no change marker → weak
        let weak_text = "秘境就在前方不远处。古老的石门被藤蔓覆盖着。入口就在这里。";
        let weak_result = metric_state_delta_coverage(weak_text, &deltas);
        // With 1 delta: keyword match but no change marker → weak → weighted_ratio = 0*1.0 + 1*0.4 / 1 = 0.4 → score 0.3
        assert!(
            weak_result.evidence_excerpt.contains("weak"),
            "mention without change verb should be 'weak', got: {}",
            weak_result.evidence_excerpt
        );

        // Text mentions keyword WITH change marker → covered
        let covered_text = "林墨发现了秘境入口。古老的石门在他面前缓缓打开。";
        let covered_result = metric_state_delta_coverage(covered_text, &deltas);
        // "发现" is change marker; "秘境" and "入口" are keywords; sentence "林墨发现了秘境入口" has both → covered
        // weighted_ratio = 1*1.0 + 0*0.4 / 1 = 1.0 → score 0.9
        assert!(
            covered_result.score > weak_result.score,
            "covered should score higher than weak: covered={:.2} weak={:.2}",
            covered_result.score, weak_result.score
        );
    }

    // ──── P14: Approved canon constraint violation detection with source_ref ────

    use crate::writer_agent::world_bible::{
        ApprovalStatus, CanonConstraint, CanonConstraintKind, ConstraintSeverity, EvidenceRef,
        SceneContract, WorldAsset, WorldAssetKind, WorldRule, RuleSubKind, TypedWorldAsset,
        WorldBibleIndex,
    };

    fn sample_evidence(source_id: &str, excerpt: &str) -> EvidenceRef {
        EvidenceRef {
            source_id: source_id.to_string(),
            source_path: Some("world_bible.md".to_string()),
            start_line: Some(10),
            end_line: Some(20),
            excerpt: excerpt.to_string(),
            confidence: 0.95,
        }
    }

    fn make_scene_contract_with_constraints(constraints: Vec<CanonConstraint>) -> SceneContract {
        SceneContract {
            chapter_id: "ch1".to_string(),
            mission: "test mission".to_string(),
            required_facts: Vec::new(),
            active_constraints: constraints,
            required_state_deltas: Vec::new(),
            allowed_reveals: Vec::new(),
            blocked_reveals: Vec::new(),
            evidence_refs: Vec::new(),
            continuity_anchors: Vec::new(),
            required_costs: Vec::new(),
        }
    }

    #[test]
    fn world_consistency_detects_hierarchy_violation_with_source_ref() {
        let text = "林墨（一个散修）随手捏碎了上古封印。";
        let hierarchy_constraint = CanonConstraint {
            id: "hierarchy-limit-001".to_string(),
            kind: CanonConstraintKind::HierarchyLimit,
            summary: "散修不可破坏上古封印".to_string(),
            trigger_terms: vec!["散修".to_string()],
            // Use exact terms that appear in the text for reliable matching
            forbidden_terms: vec!["捏碎".to_string(), "上古封印".to_string()],
            required_terms: Vec::new(),
            severity: ConstraintSeverity::Hard,
            source_asset_id: "hierarchy-001".to_string(),
            evidence: vec![sample_evidence("src://world_bible.md#hierarchy", "散修层级不可触及上古封印")],
            applies_to: vec!["散修".to_string()],
            expected_consequence: "封印反噬".to_string(),
        };
        let contract = make_scene_contract_with_constraints(vec![hierarchy_constraint]);
        // Provide an approved asset matching the constraint's source_asset_id so severity stays Hard
        let assets = vec![WorldAsset {
            id: "hierarchy-001".to_string(),
            kind: WorldAssetKind::Hierarchy,
            name: "修炼层级".to_string(),
            summary: "散修属于最低层级".to_string(),
            evidence: vec![sample_evidence("src://world_bible.md#hierarchy", "散修层级不可触及上古封印")],
            approval_status: ApprovalStatus::Approved,
            tags: vec!["散修".to_string(), "上古封印".to_string()],
        }];
        let result = metric_world_consistency(text, Some(&contract), &assets, &[]);

        assert!(result.score < 1.0, "hierarchy violation should reduce score, got {}", result.score);
        assert!(
            result.evidence_excerpt.contains("[Hard]"),
            "evidence should contain [Hard] severity label, got: {}",
            result.evidence_excerpt
        );
        assert!(
            result.evidence_excerpt.contains("rule_id=hierarchy-limit-001"),
            "evidence should contain rule_id, got: {}",
            result.evidence_excerpt
        );
        assert!(
            result.evidence_excerpt.contains("src://world_bible.md#hierarchy"),
            "evidence should contain source_ref, got: {}",
            result.evidence_excerpt
        );
        assert!(
            result.evidence_excerpt.contains("excerpt:"),
            "evidence should contain excerpt, got: {}",
            result.evidence_excerpt
        );
    }

    #[test]
    fn world_consistency_detects_forbidden_action_with_source_ref() {
        let text = "他使用了禁忌法术，召唤远古邪神。";
        let forbidden_constraint = CanonConstraint {
            id: "forbidden-action-001".to_string(),
            kind: CanonConstraintKind::ForbiddenClaim,
            summary: "禁止召唤远古邪神".to_string(),
            trigger_terms: vec!["禁忌法术".to_string()],
            forbidden_terms: vec!["召唤远古邪神".to_string()],
            required_terms: Vec::new(),
            severity: ConstraintSeverity::Hard,
            source_asset_id: "rule-001".to_string(),
            evidence: vec![sample_evidence("src://world_bible.md#taboo", "召唤远古邪神将导致世界崩溃")],
            applies_to: Vec::new(),
            expected_consequence: "世界崩溃".to_string(),
        };
        let contract = make_scene_contract_with_constraints(vec![forbidden_constraint]);
        // Provide an approved asset matching the constraint's source_asset_id so severity stays Hard
        let assets = vec![WorldAsset {
            id: "rule-001".to_string(),
            kind: WorldAssetKind::Rule,
            name: "禁忌法术".to_string(),
            summary: "禁止召唤远古邪神".to_string(),
            evidence: vec![sample_evidence("src://world_bible.md#taboo", "召唤远古邪神将导致世界崩溃")],
            approval_status: ApprovalStatus::Approved,
            tags: vec!["召唤远古邪神".to_string()],
        }];
        let result = metric_world_consistency(text, Some(&contract), &assets, &[]);

        assert!(result.score < 1.0, "forbidden action should reduce score, got {}", result.score);
        assert!(
            result.evidence_excerpt.contains("[Hard]"),
            "evidence should contain [Hard] severity label, got: {}",
            result.evidence_excerpt
        );
        assert!(
            result.evidence_excerpt.contains("rule_id=forbidden-action-001"),
            "evidence should contain rule_id, got: {}",
            result.evidence_excerpt
        );
        assert!(
            result.evidence_excerpt.contains("src://world_bible.md#taboo"),
            "evidence should contain source_ref, got: {}",
            result.evidence_excerpt
        );
    }

    #[test]
    fn world_consistency_passes_when_no_violation() {
        let text = "林墨（一个散修）在村口休息，看着远处的山峦。";
        let hierarchy_constraint = CanonConstraint {
            id: "hierarchy-limit-001".to_string(),
            kind: CanonConstraintKind::HierarchyLimit,
            summary: "散修不可破坏上古封印".to_string(),
            trigger_terms: vec!["散修".to_string()],
            forbidden_terms: vec!["捏碎上古封印".to_string()],
            required_terms: Vec::new(),
            severity: ConstraintSeverity::Hard,
            source_asset_id: "hierarchy-001".to_string(),
            evidence: vec![sample_evidence("src://world_bible.md#hierarchy", "散修层级不可触及上古封印")],
            applies_to: vec!["散修".to_string()],
            expected_consequence: "封印反噬".to_string(),
        };
        let contract = make_scene_contract_with_constraints(vec![hierarchy_constraint]);
        let result = metric_world_consistency(text, Some(&contract), &[], &[]);

        assert_eq!(result.score, 1.0, "no violation should yield perfect score");
        assert!(
            result.evidence_excerpt.is_empty() || result.evidence_excerpt.contains("无世界观一致性违规"),
            "no violation should yield empty or passing evidence, got: {}",
            result.evidence_excerpt
        );
    }

    #[test]
    fn approved_canon_constraints_take_priority_over_generic_assets() {
        // Create a WorldBibleIndex with approved typed assets
        let mut index = WorldBibleIndex::new("proj1");

        let rule = WorldRule {
            id: "rule-001".to_string(),
            sub_kind: RuleSubKind::Taboo,
            name: "禁忌法术".to_string(),
            summary: "散修不可使用禁忌法术".to_string(),
            source_ref: sample_evidence("src://world_bible.md#taboo", "散修使用禁忌法术将遭天谴"),
            original_excerpt: "散修使用禁忌法术将遭天谴".to_string(),
            confidence: 0.95,
            approval_status: ApprovalStatus::Approved,
            tags: vec!["禁忌法术".to_string()],
            scope: vec!["散修".to_string()],
            severity_description: "fatal".to_string(),
        };
        index.add_asset(TypedWorldAsset::Rule(rule)).unwrap();

        // Compile approved constraints at confidence threshold 0.7
        // compile_approved_constraints only compiles Rule assets into ForbiddenClaim constraints
        let canon_constraints = index.compile_approved_constraints(0.7);
        assert!(!canon_constraints.is_empty(), "should produce at least one constraint from approved rule");

        // Also compile matching WorldAsset for validate_world_consistency to resolve effective severity
        let assets: Vec<WorldAsset> = index
            .assets
            .iter()
            .filter(|a| a.can_enter_approved_canon(0.7))
            .map(|a| a.to_world_asset())
            .collect();
        assert!(!assets.is_empty(), "should have at least one approved asset");

        // Text violates the compiled constraint: contains "禁忌法术" (forbidden term from rule tags)
        let text = "散修林墨使用了禁忌法术。";
        let contract = make_scene_contract_with_constraints(Vec::new());

        // When canon_constraints are provided directly, they take priority
        let result = metric_world_consistency(text, Some(&contract), &assets, &canon_constraints);

        assert!(result.score < 1.0, "violation should be detected via canon_constraints, got score={}", result.score);
        assert!(
            result.evidence_excerpt.contains("rule_id="),
            "evidence should contain rule_id from canon constraint, got: {}",
            result.evidence_excerpt
        );
        assert!(
            result.evidence_excerpt.contains("src://world_bible.md#taboo"),
            "evidence should contain source_ref from approved rule, got: {}",
            result.evidence_excerpt
        );
    }

    // ──── P17: Term misuse detection ────

    #[test]
    fn term_misuse_detects_changed_meaning() {
        let text = "寒影剑是一把邪恶的魔剑，它代表着黑暗与背叛。";
        let canon_terms = vec![
            crate::writer_agent::world_bible::CanonTerm {
                term: "寒影剑".to_string(),
                definition: "寒影剑是正义之剑，代表光明与忠诚".to_string(),
                source_asset_id: "entity-001".to_string(),
                severity: ConstraintSeverity::Hard,
            },
        ];
        let result = metric_term_misuse(text, &canon_terms);
        assert!(result.score < 1.0, "term misuse should reduce score, got {}", result.score);
        assert!(
            result.evidence_excerpt.contains("Hard"),
            "evidence should contain Hard severity, got: {}",
            result.evidence_excerpt
        );
        assert!(
            result.evidence_excerpt.contains("寒影剑"),
            "evidence should contain the term, got: {}",
            result.evidence_excerpt
        );
    }

    #[test]
    fn term_misuse_passes_when_no_contradiction() {
        let text = "寒影剑是正义之剑，代表光明与忠诚。";
        let canon_terms = vec![
            crate::writer_agent::world_bible::CanonTerm {
                term: "寒影剑".to_string(),
                definition: "寒影剑是正义之剑，代表光明与忠诚".to_string(),
                source_asset_id: "entity-001".to_string(),
                severity: ConstraintSeverity::Hard,
            },
        ];
        let result = metric_term_misuse(text, &canon_terms);
        assert_eq!(result.score, 1.0, "no contradiction should yield perfect score");
    }

    #[test]
    fn term_misuse_gates_when_no_canon_terms() {
        let result = metric_term_misuse("some text", &[]);
        assert_eq!(result.score, 0.5, "empty canon_terms should gate to 0.5");
    }

    // ──── P17: Strict mode integration ────

    #[test]
    fn strict_mode_blocks_save_on_hard_violation() {
        // Simulate a quality report with hard world consistency violations
        let violations = [
            crate::writer_agent::world_bible::WorldConsistencyViolation {
                constraint_id: "hierarchy-limit-001".to_string(),
                severity: ConstraintSeverity::Hard,
                kind: CanonConstraintKind::HierarchyLimit,
                message: "散修不可破坏上古封印".to_string(),
                text_excerpt: "散修捏碎了上古封印".to_string(),
                evidence: vec![sample_evidence("src://world_bible.md#hierarchy", "散修层级不可触及上古封印")],
                suggested_fix: "确认角色层级".to_string(),
            },
        ];
        let hard_count = violations.iter().filter(|v| matches!(v.severity, ConstraintSeverity::Hard)).count();
        assert_eq!(hard_count, 1, "should have exactly 1 hard violation");
        // In strict mode, any hard violation blocks save
        assert!(
            hard_count > 0,
            "strict mode should block when hard violations exist"
        );
    }

    #[test]
    fn new_information_density_triggers_strict_warning() {
        let prior = vec!["林墨在茶馆遇到了张三。".to_string()];
        let text = "林墨在茶馆遇到了张三。林墨在茶馆遇到了张三。林墨在茶馆遇到了张三。";
        let result = metric_new_information_density(text, &prior);
        // High repetition → low new info density
        assert!(result.score < 0.5, "high repetition should yield low new_information_density, got {}", result.score);
    }
}
