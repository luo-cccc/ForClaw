const OVERALL_WEIGHTS: &[(&str, f32)] = &[
    ("anchor_carry", 0.15),
    ("style_drift", 0.10),
    ("length_compliance", 0.10),
    ("dialogue_function", 0.15),
    ("exposition_ratio", 0.15),
    ("ending_hook", 0.15),
    ("scene_causality", 0.10),
    ("promise_progress", 0.10),
];

#[derive(Debug, Clone, Default)]
pub struct ChapterQualitySignals {
    pub anchor_keywords: Vec<String>,
    pub author_voice: Option<crate::writer_agent::author_voice::AuthorVoiceSnapshot>,
    pub required_anchors: Vec<crate::chapter_generation::StoryAnchor>,
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

    ChapterQualityReport {
        chapter_title: chapter_title.to_string(),
        overall_score,
        fatal_issues,
        major_issues,
        metric_results,
        top_revision_targets,
        no_fatal_issue,
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
            if sim > 0.4 && best_match.map_or(true, |(_, best_sim)| sim > best_sim) {
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
        assert!(report.overall_score <= 0.65);
        assert!(!report.metric_results.is_empty());
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
    fn all_eight_metrics_present() {
        let plan = SceneCraftPlan::default();
        let report = evaluate_chapter_quality("一些测试文本内容", "test-chapter", &plan, &[], 0, 500);
        assert_eq!(report.metric_results.len(), 8, "all 8 metrics should be present");
        let expected_metrics = [
            "anchor_carry", "style_drift", "length_compliance",
            "dialogue_function", "exposition_ratio", "ending_hook",
            "scene_causality", "promise_progress",
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
}
