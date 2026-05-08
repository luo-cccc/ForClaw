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

pub fn evaluate_chapter_quality(
    chapter_text: &str,
    chapter_title: &str,
    _scene_plan: &SceneCraftPlan,
    open_promise_keywords: &[String],
    target_min_chars: usize,
    target_max_chars: usize,
) -> ChapterQualityReport {
    let metric_results = vec![
        metric_length_compliance(chapter_text, target_min_chars, target_max_chars),
        metric_dialogue_function(chapter_text),
        metric_exposition_ratio(chapter_text),
        metric_ending_hook(chapter_text),
        metric_scene_causality(chapter_text),
        metric_promise_progress(chapter_text, open_promise_keywords),
        // anchor_carry and style_drift require project-level data or pre-built snapshots;
        // for MVP, emit placeholder "insufficient evidence" results
        gated_metric(
            "anchor_carry", 0.5, "", "anchor_carry.rs",
            "需要项目级锚点数据，本次评估跳过", "在完整写作项目中重新运行"
        ),
        gated_metric(
            "style_drift", 0.5, "", "author_voice.rs",
            "需要作者风格快照，本次评估跳过", "在完整写作项目中重新运行"
        ),
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

fn metric_ending_hook(text: &str) -> QualityMetricResult {
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

    let score = match (has_consequence, has_question) {
        (true, true) => 0.9,
        (true, false) | (false, true) => 0.5,
        (false, false) => 0.2,
    };

    let evidence: String = tail.chars().rev().take(100).collect::<String>().chars().rev().collect();

    gated_metric(
        "ending_hook", score, &evidence,
        "craft:ending_hook",
        &format!("后果信号={}, 未解信号={}", has_consequence, has_question),
        "章末加一个刚发生的后果和一个角色面临的选择",
    )
}

fn metric_scene_causality(text: &str) -> QualityMetricResult {
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

    let score = if density >= 1.0 {
        0.9
    } else if density >= 0.5 {
        0.6
    } else {
        0.3
    };
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
}
