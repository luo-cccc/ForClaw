use agent_writer_lib::chapter_generation::{
    build_revision_target_changes, build_revision_target_changes_with_text, build_scene_craft_plan,
    compile_empowerment_prompt, compile_empowerment_prompt_with_memory,
    evaluate_chapter_quality_with_signals, format_craft_prompt_section, ChapterQualitySignals,
    CraftMemoryPromptBadPattern, CraftMemoryPromptExample, CraftMemoryPromptSamples,
    ManualCraftEditFeedbackRequest, SceneCraftPlan,
};
use agent_writer_lib::writer_agent::author_voice::{
    AuthorVoiceSnapshot, VoiceDiction, VoiceRhythm,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

const TREND_REGRESSION_THRESHOLD: f32 = 0.05;

#[derive(Debug, Deserialize)]
struct EvalTask {
    task: String,
    chapter: String,
    instruction: Option<String>,
    check: Option<String>,
    metrics: Option<Vec<String>>,
    expected: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EvalResult {
    task: String,
    chapter: String,
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    before: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    after: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    delta: Option<serde_json::Value>,
    message: String,
}

#[derive(Debug)]
struct PreviousEvalRun {
    timestamp: String,
    results: Vec<EvalResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EvalRunTrend {
    timestamp: String,
    task_count: usize,
    pass: usize,
    fail: usize,
    average_after_score: Option<f32>,
    average_score_delta: Option<f32>,
    metric_after_average: BTreeMap<String, f32>,
    craft_rule_trends: BTreeMap<String, CraftRuleEvalTrend>,
    task_status: BTreeMap<String, String>,
    task_after_score: BTreeMap<String, f32>,
    failing_tasks: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct CraftRuleEvalTrend {
    rule_id: String,
    accepted_updates: usize,
    rejected_updates: usize,
    stored_examples: usize,
    stored_bad_patterns: usize,
    prompt_examples: usize,
    prompt_bad_patterns: usize,
    score_delta_sample_count: usize,
    average_score_delta: Option<f32>,
    matched_metrics: BTreeMap<String, usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EvalTrendDelta {
    pass_delta: isize,
    fail_delta: isize,
    average_after_score_delta: Option<f32>,
    average_score_delta_delta: Option<f32>,
    metric_after_average_delta: BTreeMap<String, f32>,
    craft_rule_trend_delta: BTreeMap<String, CraftRuleEvalTrendDelta>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CraftRuleEvalTrendDelta {
    accepted_updates_delta: isize,
    rejected_updates_delta: isize,
    stored_examples_delta: isize,
    stored_bad_patterns_delta: isize,
    prompt_examples_delta: isize,
    prompt_bad_patterns_delta: isize,
    average_score_delta_delta: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EvalTrendRegression {
    kind: String,
    subject: String,
    previous: serde_json::Value,
    current: serde_json::Value,
    message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EvalTrendReport {
    current: EvalRunTrend,
    previous: Option<EvalRunTrend>,
    delta: Option<EvalTrendDelta>,
    regressions: Vec<EvalTrendRegression>,
}

fn fixture_dir() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest.join("..").join("fixtures").join("writing_eval")
}

fn load_fixture() -> serde_json::Value {
    let path = fixture_dir().join("project.json");
    let text = std::fs::read_to_string(&path).expect("read project.json");
    serde_json::from_str(&text).expect("parse project.json")
}

fn load_tasks() -> Vec<EvalTask> {
    let path = fixture_dir().join("eval_tasks.jsonl");
    let text = std::fs::read_to_string(&path).expect("read eval_tasks.jsonl");
    text.lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty() && !l.starts_with("//"))
        .map(|l| serde_json::from_str(l).expect("parse eval task"))
        .collect()
}

fn run_chapter_generation_eval(task: &EvalTask, fixture: &serde_json::Value) -> EvalResult {
    let chapter_text = fixture["chapters"][&task.chapter].as_str().unwrap_or("");
    let plan = SceneCraftPlan::default();

    // Before: evaluate raw fixture chapter
    let quality_signals = quality_signals_from_fixture(fixture);
    let before_report = evaluate_chapter_quality_with_signals(
        chapter_text,
        &task.chapter,
        &plan,
        &[],
        500,
        2000,
        &quality_signals,
    );

    // After: compile empowerment prompt for the requested generation contract, then
    // re-evaluate the fixture chapter against the selected craft targets.
    let outline = fixture["outline"].as_array().unwrap();
    let summary = outline
        .iter()
        .find(|n| n["chapterTitle"].as_str() == Some(&task.chapter))
        .and_then(|n| n["summary"].as_str())
        .unwrap_or("");
    let instruction = task.instruction.as_deref().unwrap_or("");
    let objective = format!("{} {}", summary, instruction);

    let packet =
        compile_empowerment_prompt(&objective, "关键选择", 1, false, Some(5), Some(600), None);

    // Use craft plan from packet for after-evaluation
    let craft_plan = SceneCraftPlan {
        chapter_title: task.chapter.clone(),
        selected_craft_rules: packet
            .craft_rules
            .iter()
            .map(|r| r.rule_id.clone())
            .collect(),
        ..SceneCraftPlan::default()
    };

    let after_report = evaluate_chapter_quality_with_signals(
        chapter_text,
        &task.chapter,
        &craft_plan,
        &[],
        500,
        2000,
        &quality_signals,
    );

    let before_score = before_report.overall_score;
    let after_score = after_report.overall_score;
    let score_delta = after_score - before_score;

    let expected = &task.expected;
    let min_chars = expected["min_chars"].as_u64().unwrap_or(0) as usize;
    let max_chars = expected["max_chars"].as_u64().unwrap_or(usize::MAX as u64) as usize;
    let contract_valid = min_chars > 0 && min_chars <= max_chars;
    let outline_text = fixture["outline"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|node| node["summary"].as_str())
        .collect::<Vec<_>>()
        .join(" ");
    let lore_text = fixture["lorebook"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|entry| [entry["keyword"].as_str(), entry["content"].as_str()])
        .flatten()
        .collect::<Vec<_>>()
        .join(" ");
    let evidence_text =
        format!("{chapter_text} {summary} {instruction} {outline_text} {lore_text}");
    let must_contain: Vec<String> = expected["must_contain"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|v| v.as_str().map(str::to_string))
        .collect();
    let missing_terms: Vec<String> = must_contain
        .iter()
        .filter(|term| !evidence_text.contains(term.as_str()))
        .cloned()
        .collect();
    let mission_hit =
        !expected["mission_hit"].as_bool().unwrap_or(false) || missing_terms.is_empty();

    let status = if contract_valid && mission_hit && !packet.craft_rules.is_empty() {
        "pass"
    } else {
        "fail"
    };

    let message = format!(
        "fixture_chars={}, contract={}-{}, selected_rules={}, missing_terms={:?}, before_score={:.2}, after_score={:.2}, delta={:.2}",
        chapter_text.chars().count(),
        min_chars,
        max_chars,
        packet.craft_rules.len(),
        missing_terms,
        before_score,
        after_score,
        score_delta
    );

    EvalResult {
        task: task.task.clone(),
        chapter: task.chapter.clone(),
        status: status.to_string(),
        before: Some(serde_json::json!({
            "overall_score": before_score,
            "fatal_issues": before_report.fatal_issues.len(),
            "major_issues": before_report.major_issues.len(),
        })),
        after: Some(serde_json::json!({
            "overall_score": after_score,
            "fatal_issues": after_report.fatal_issues.len(),
            "major_issues": after_report.major_issues.len(),
        })),
        delta: Some(serde_json::json!({
            "overall_score": score_delta,
            "fatal_issues": after_report.fatal_issues.len() as i64 - before_report.fatal_issues.len() as i64,
            "major_issues": after_report.major_issues.len() as i64 - before_report.major_issues.len() as i64,
        })),
        message,
    }
}

fn run_quality_evaluation_eval(task: &EvalTask, fixture: &serde_json::Value) -> EvalResult {
    let chapter_text = fixture["chapters"][&task.chapter].as_str().unwrap_or("");
    let plan = SceneCraftPlan::default();

    // Before: evaluate with default plan
    let quality_signals = quality_signals_from_fixture(fixture);
    let before_report = evaluate_chapter_quality_with_signals(
        chapter_text,
        &task.chapter,
        &plan,
        &[],
        500,
        2000,
        &quality_signals,
    );

    // After: evaluate with craft-aware plan
    let outline = fixture["outline"].as_array().unwrap();
    let summary = outline
        .iter()
        .find(|n| n["chapterTitle"].as_str() == Some(&task.chapter))
        .and_then(|n| n["summary"].as_str())
        .unwrap_or("");

    let packet =
        compile_empowerment_prompt(summary, "关键选择", 1, false, Some(5), Some(600), None);

    let craft_plan = SceneCraftPlan {
        chapter_title: task.chapter.clone(),
        selected_craft_rules: packet
            .craft_rules
            .iter()
            .map(|r| r.rule_id.clone())
            .collect(),
        ..SceneCraftPlan::default()
    };

    let after_report = evaluate_chapter_quality_with_signals(
        chapter_text,
        &task.chapter,
        &craft_plan,
        &[],
        500,
        2000,
        &quality_signals,
    );

    let before_score = before_report.overall_score;
    let after_score = after_report.overall_score;
    let min_score = task.expected["overall_score_min"].as_f64().unwrap_or(0.0) as f32;
    let metric_min = task.expected["metric_min"].as_object();
    let metric_failures: Vec<String> = metric_min
        .into_iter()
        .flat_map(|map| map.iter())
        .filter_map(|(metric, min)| {
            let min = min.as_f64().unwrap_or(0.0) as f32;
            let actual = after_report
                .metric_results
                .iter()
                .find(|result| result.metric == *metric)
                .map(|result| result.score)
                .unwrap_or(0.0);
            if actual < min {
                Some(format!("{metric} {:.2} < {:.2}", actual, min))
            } else {
                None
            }
        })
        .collect();
    let target_changes =
        build_revision_target_changes(&before_report, Some(&after_report), true, false);

    let status = if after_score >= min_score && metric_failures.is_empty() {
        "pass"
    } else {
        "fail"
    };

    let message = format!(
        "metrics={:?}, before_score={:.2}, after_score={:.2}, expected_min={:.2}, metric_failures={:?}",
        task.metrics, before_score, after_score, min_score, metric_failures
    );

    EvalResult {
        task: task.task.clone(),
        chapter: task.chapter.clone(),
        status: status.to_string(),
        before: Some(serde_json::json!({
            "overall_score": before_score,
            "metric_results": before_report.metric_results.iter().map(|m| (m.metric.clone(), m.score)).collect::<std::collections::HashMap<_,_>>(),
        })),
        after: Some(serde_json::json!({
            "overall_score": after_score,
            "metric_results": after_report.metric_results.iter().map(|m| (m.metric.clone(), m.score)).collect::<std::collections::HashMap<_,_>>(),
        })),
        delta: Some(serde_json::json!({
            "overall_score": after_score - before_score,
            "metric_results": metric_delta_map(&before_report, &after_report),
            "revision_target_changes": target_changes,
        })),
        message,
    }
}

fn run_quality_signal_eval(task: &EvalTask, fixture: &serde_json::Value) -> EvalResult {
    let chapter_text = fixture["chapters"][&task.chapter].as_str().unwrap_or("");
    let plan = SceneCraftPlan::default();
    let quality_signals = quality_signals_from_fixture(fixture);
    let report = evaluate_chapter_quality_with_signals(
        chapter_text,
        &task.chapter,
        &plan,
        &[],
        500,
        2000,
        &quality_signals,
    );
    let metric_scores = report
        .metric_results
        .iter()
        .map(|metric| (metric.metric.clone(), metric.score))
        .collect::<std::collections::HashMap<_, _>>();
    let metric_reasons = report
        .metric_results
        .iter()
        .map(|metric| (metric.metric.clone(), metric.reason.clone()))
        .collect::<std::collections::HashMap<_, _>>();
    let metric_min = task.expected["metric_min"].as_object();
    let metric_failures: Vec<String> = metric_min
        .into_iter()
        .flat_map(|map| map.iter())
        .filter_map(|(metric, min)| {
            let min = min.as_f64().unwrap_or(0.0) as f32;
            let actual = metric_scores.get(metric).copied().unwrap_or(0.0);
            if actual < min {
                Some(format!("{metric} {:.2} < {:.2}", actual, min))
            } else {
                None
            }
        })
        .collect();
    let checked_metrics = task
        .metrics
        .clone()
        .unwrap_or_else(|| vec!["anchor_carry".to_string(), "style_drift".to_string()]);
    let evidence_failures: Vec<String> = task
        .expected
        .get("must_not_contain_reason")
        .and_then(|value| value.as_array())
        .into_iter()
        .flatten()
        .filter_map(|value| value.as_str())
        .filter_map(|needle| {
            let found = checked_metrics.iter().any(|metric| {
                metric_reasons
                    .get(metric)
                    .is_some_and(|reason| reason.contains(needle))
            });
            if found {
                Some(format!("unexpected placeholder reason: {needle}"))
            } else {
                None
            }
        })
        .collect();
    let status = if metric_failures.is_empty() && evidence_failures.is_empty() {
        "pass"
    } else {
        "fail"
    };

    EvalResult {
        task: task.task.clone(),
        chapter: task.chapter.clone(),
        status: status.to_string(),
        before: None,
        after: Some(serde_json::json!({
            "overall_score": report.overall_score,
            "metric_results": metric_scores,
            "metric_reasons": metric_reasons,
            "anchor_keywords": quality_signals.anchor_keywords,
            "author_voice": quality_signals.author_voice,
        })),
        delta: None,
        message: format!(
            "quality_signals metric_failures={:?}, evidence_failures={:?}",
            metric_failures, evidence_failures
        ),
    }
}

fn run_targeted_revision_eval(task: &EvalTask, fixture: &serde_json::Value) -> EvalResult {
    let before_text = task
        .expected
        .get("before_text")
        .and_then(|value| value.as_str())
        .unwrap_or("林墨看着寒影剑。");
    let after_text = task
        .expected
        .get("after_text")
        .and_then(|value| value.as_str())
        .unwrap_or("林墨只好拔出寒影剑，因此付出代价。");
    let plan = SceneCraftPlan::default();
    let quality_signals = quality_signals_from_fixture(fixture);
    let before_report = evaluate_chapter_quality_with_signals(
        before_text,
        &task.chapter,
        &plan,
        &[],
        0,
        2000,
        &quality_signals,
    );
    let after_report = evaluate_chapter_quality_with_signals(
        after_text,
        &task.chapter,
        &plan,
        &[],
        0,
        2000,
        &quality_signals,
    );
    let changes = build_revision_target_changes_with_text(
        &before_report,
        Some(&after_report),
        true,
        false,
        Some(before_text),
        Some(after_text),
    );
    let has_excerpt_mapping = changes.iter().any(|change| {
        !change.changed_excerpt_before.is_empty()
            && !change.changed_excerpt_after.is_empty()
            && change.text_change_summary.contains("Draft text changed")
    });
    let status = if has_excerpt_mapping { "pass" } else { "fail" };
    EvalResult {
        task: task.task.clone(),
        chapter: task.chapter.clone(),
        status: status.to_string(),
        before: Some(serde_json::json!({
            "overall_score": before_report.overall_score,
            "metric_results": before_report.metric_results.iter().map(|m| (m.metric.clone(), m.score)).collect::<std::collections::HashMap<_,_>>(),
        })),
        after: Some(serde_json::json!({
            "overall_score": after_report.overall_score,
            "metric_results": after_report.metric_results.iter().map(|m| (m.metric.clone(), m.score)).collect::<std::collections::HashMap<_,_>>(),
        })),
        delta: Some(serde_json::json!({
            "revision_target_changes": changes,
            "has_excerpt_mapping": has_excerpt_mapping,
        })),
        message: format!("targeted_revision excerpt_mapping={}", has_excerpt_mapping),
    }
}

fn run_craft_memory_eval(task: &EvalTask, _fixture: &serde_json::Value) -> EvalResult {
    let conn = rusqlite::Connection::open_in_memory().expect("open in-memory db");
    agent_writer_lib::writer_agent::memory::ensure_craft_tables(&conn)
        .expect("ensure craft tables");
    let example = agent_writer_lib::writer_agent::memory::CraftExampleMemory {
        id: "eval-dialogue-example".to_string(),
        rule_id: "dialogue_function".to_string(),
        scope: task.chapter.clone(),
        excerpt_ref: "eval:revision_report:dialogue_function".to_string(),
        excerpt: "他说：你现在必须选择。".to_string(),
        reason: "dialogue_function improved".to_string(),
        pattern: "dialogue_function".to_string(),
        scene_types: vec!["chapter_targeted_revision".to_string()],
        score_delta: 0.42,
        created_at: 1,
    };
    let bad = agent_writer_lib::writer_agent::memory::CraftBadPatternMemory {
        id: "eval-dialogue-bad-pattern".to_string(),
        rule_id: "dialogue_function".to_string(),
        scope: task.chapter.clone(),
        pattern: "dialogue_function".to_string(),
        evidence_ref: "eval:revision_report:dialogue_function".to_string(),
        evidence_excerpt: "他说了一段背景，局面没有变化。".to_string(),
        correction: "让对话改变权力、关系、信息或选择。".to_string(),
        rejected_count: 1,
        created_at: 2,
        updated_at: 2,
    };
    agent_writer_lib::writer_agent::memory::record_craft_example(&conn, &example)
        .expect("record craft example");
    agent_writer_lib::writer_agent::memory::record_craft_bad_pattern(&conn, &bad)
        .expect("record bad pattern");
    agent_writer_lib::writer_agent::memory::record_craft_bad_pattern(&conn, &bad)
        .expect("increment bad pattern");

    let examples =
        agent_writer_lib::writer_agent::memory::list_craft_examples(&conn, "dialogue_function", 10)
            .expect("list examples");
    let bad_patterns = agent_writer_lib::writer_agent::memory::list_craft_bad_patterns(
        &conn,
        "dialogue_function",
        10,
    )
    .expect("list bad patterns");
    let min_examples = task.expected["min_examples"].as_u64().unwrap_or(1) as usize;
    let min_bad_patterns = task.expected["min_bad_patterns"].as_u64().unwrap_or(1) as usize;
    let status = if examples.len() >= min_examples
        && bad_patterns.len() >= min_bad_patterns
        && bad_patterns
            .first()
            .is_some_and(|pattern| pattern.rejected_count >= 2)
    {
        "pass"
    } else {
        "fail"
    };

    EvalResult {
        task: task.task.clone(),
        chapter: task.chapter.clone(),
        status: status.to_string(),
        before: None,
        after: Some(serde_json::json!({
            "examples": examples,
            "bad_patterns": bad_patterns,
        })),
        delta: None,
        message: format!(
            "craft_memory examples={}, bad_patterns={}",
            examples.len(),
            bad_patterns.len()
        ),
    }
}

fn run_manual_craft_edit_eval(task: &EvalTask, fixture: &serde_json::Value) -> EvalResult {
    let conn = rusqlite::Connection::open_in_memory().expect("open in-memory db");
    agent_writer_lib::writer_agent::memory::ensure_craft_tables(&conn)
        .expect("ensure craft tables");
    let before_text = task
        .expected
        .get("before_text")
        .and_then(|value| value.as_str())
        .unwrap_or("林墨说：这是古剑。散修听完，没有变化。");
    let after_text = task
        .expected
        .get("after_text")
        .and_then(|value| value.as_str())
        .unwrap_or("林墨握紧寒影剑，低声说：现在你必须选择。散修因此停在门口。");
    let request = ManualCraftEditFeedbackRequest {
        chapter_title: task.chapter.clone(),
        before_text: before_text.to_string(),
        after_text: after_text.to_string(),
        metrics: task.metrics.clone().unwrap_or_default(),
        anchor_keywords: ["寒影剑", "林墨", "代价", "选择"]
            .into_iter()
            .map(str::to_string)
            .collect(),
        open_promise_keywords: vec!["寒影剑".to_string(), "代价".to_string()],
        author_voice: quality_signals_from_fixture(fixture).author_voice,
        target_min_chars: Some(0),
        target_max_chars: Some(2000),
        source_ref: Some("eval:manual_author_edit".to_string()),
        author_approved: true,
    };
    let result =
        agent_writer_lib::chapter_generation::record_manual_craft_edit_feedback(&conn, request)
            .expect("record manual craft edit feedback");
    let min_examples = task.expected["min_examples"].as_u64().unwrap_or(1) as usize;
    let min_bad_patterns = task.expected["min_bad_patterns"].as_u64().unwrap_or(1) as usize;
    let requires_mapping = task.expected["requires_excerpt_mapping"]
        .as_bool()
        .unwrap_or(true);
    let has_mapping = result.target_changes.iter().any(|change| {
        !change.changed_excerpt_before.is_empty() && !change.changed_excerpt_after.is_empty()
    });
    let status = if result.example_refs.len() >= min_examples
        && result.bad_pattern_refs.len() >= min_bad_patterns
        && (!requires_mapping || has_mapping)
    {
        "pass"
    } else {
        "fail"
    };

    EvalResult {
        task: task.task.clone(),
        chapter: task.chapter.clone(),
        status: status.to_string(),
        before: Some(serde_json::json!({
            "score": result.score_before,
        })),
        after: Some(serde_json::json!({
            "score": result.score_after,
            "example_refs": result.example_refs,
            "bad_pattern_refs": result.bad_pattern_refs,
        })),
        delta: Some(serde_json::json!({
            "target_changes": result.target_changes,
            "craft_memory_updates": result.craft_memory_updates,
            "has_excerpt_mapping": has_mapping,
        })),
        message: format!(
            "manual_craft_edit examples={}, bad_patterns={}, mapping={}",
            result.example_refs.len(),
            result.bad_pattern_refs.len(),
            has_mapping
        ),
    }
}

fn run_craft_memory_prompt_eval(task: &EvalTask, _fixture: &serde_json::Value) -> EvalResult {
    let sample = CraftMemoryPromptSamples {
        rule_id: "dialogue_function".to_string(),
        examples: vec![CraftMemoryPromptExample {
            rule_id: "dialogue_function".to_string(),
            excerpt_ref: "eval:craft_examples:dialogue".to_string(),
            excerpt: "林墨握紧寒影剑，低声说：现在你必须选择。".to_string(),
            reason: "作者认可：对话改变选择。".to_string(),
            score_delta: 0.42,
        }],
        bad_patterns: vec![CraftMemoryPromptBadPattern {
            rule_id: "dialogue_function".to_string(),
            evidence_ref: "eval:craft_bad_patterns:dialogue".to_string(),
            evidence_excerpt: "林墨说了一整段古剑来历，散修没有任何反应。".to_string(),
            correction: "让台词改变权力、信息或选择。".to_string(),
            rejected_count: 2,
        }],
    };
    let packet = compile_empowerment_prompt_with_memory(
        "审讯场景，林墨必须逼问散修",
        "对话推进",
        0,
        false,
        Some(5),
        Some(2000),
        None,
        &[sample],
    );
    let section = format_craft_prompt_section(&packet);
    let must_contain: Vec<String> = task.expected["must_contain"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|value| value.as_str().map(str::to_string))
        .collect();
    let missing = must_contain
        .iter()
        .filter(|needle| !section.contains(needle.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    let status = if missing.is_empty()
        && !packet.memory_examples.is_empty()
        && !packet.memory_bad_patterns.is_empty()
    {
        "pass"
    } else {
        "fail"
    };

    EvalResult {
        task: task.task.clone(),
        chapter: task.chapter.clone(),
        status: status.to_string(),
        before: None,
        after: Some(serde_json::json!({
            "memory_examples": packet.memory_examples,
            "memory_bad_patterns": packet.memory_bad_patterns,
            "prompt_section": section,
        })),
        delta: None,
        message: format!("craft_memory_prompt missing={:?}", missing),
    }
}

fn run_canon_conflict_eval(task: &EvalTask, fixture: &serde_json::Value) -> EvalResult {
    let candidate_text = task
        .expected
        .get("candidate_text")
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    let canon_rules = fixture["canon"]
        .as_array()
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    let mut conflicts = Vec::new();
    for rule in &canon_rules {
        let rule_id = rule["id"].as_str().unwrap_or("canon");
        for forbidden in rule["forbidden"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|value| value.as_str())
        {
            if candidate_text.contains(forbidden) {
                conflicts.push(format!("{rule_id}:{forbidden}"));
            }
        }
    }
    let expected_conflict = task.expected["canon_conflict"].as_bool().unwrap_or(false);
    let status = if expected_conflict != conflicts.is_empty() {
        "pass"
    } else {
        "fail"
    };

    EvalResult {
        task: task.task.clone(),
        chapter: task.chapter.clone(),
        status: status.to_string(),
        before: Some(serde_json::json!({
            "candidate_text": candidate_text,
            "canon_rules": canon_rules,
        })),
        after: Some(serde_json::json!({
            "conflicts": conflicts,
        })),
        delta: None,
        message: format!("canon_conflict conflicts={:?}", conflicts),
    }
}

fn run_planning_review_eval(task: &EvalTask, fixture: &serde_json::Value) -> EvalResult {
    let outline = fixture["outline"].as_array().unwrap();
    let chapter_summary = outline
        .iter()
        .find(|node| node["chapterTitle"].as_str() == Some(&task.chapter))
        .and_then(|node| node["summary"].as_str())
        .unwrap_or("");
    let next_summary = outline
        .windows(2)
        .find(|pair| pair[0]["chapterTitle"].as_str() == Some(&task.chapter))
        .and_then(|pair| pair[1]["summary"].as_str());
    let instruction = task.instruction.as_deref().unwrap_or("");
    let objective = format!("{chapter_summary} {instruction}");
    let packet = compile_empowerment_prompt(
        &objective,
        "计划评审",
        fixture["promises"]
            .as_array()
            .map(|promises| promises.len())
            .unwrap_or_default(),
        objective.contains("兑现") || objective.contains("代价"),
        Some(5),
        Some(1200),
        None,
    );
    let participants = ["林墨", "执事", "青云宗"]
        .into_iter()
        .map(str::to_string)
        .collect::<Vec<_>>();
    let open_promise_keywords = fixture["promises"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|promise| promise["status"].as_str() == Some("open"))
        .filter_map(|promise| promise["keyword"].as_str().map(str::to_string))
        .collect::<Vec<_>>();
    let plan = build_scene_craft_plan(
        &task.chapter,
        &objective,
        &participants,
        "计划评审",
        next_summary,
        &open_promise_keywords,
        &packet,
    );

    let expected_rules = task.expected["required_rules"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|value| value.as_str())
        .collect::<Vec<_>>();
    let missing_rules = expected_rules
        .iter()
        .filter(|rule| {
            !plan
                .selected_craft_rules
                .iter()
                .any(|selected| selected == **rule)
        })
        .map(|rule| (*rule).to_string())
        .collect::<Vec<_>>();
    let expected_payoffs = task.expected["required_payoff_keywords"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|value| value.as_str())
        .collect::<Vec<_>>();
    let missing_payoffs = expected_payoffs
        .iter()
        .filter(|keyword| {
            !plan
                .promise_or_anchor_payoff
                .iter()
                .any(|payoff| payoff.contains(**keyword))
        })
        .map(|keyword| (*keyword).to_string())
        .collect::<Vec<_>>();
    let requires_hook = task.expected["requires_ending_hook"]
        .as_bool()
        .unwrap_or(false);
    let hook_ok = !requires_hook || !plan.ending_hook.question_left_open.trim().is_empty();
    let status = if missing_rules.is_empty() && missing_payoffs.is_empty() && hook_ok {
        "pass"
    } else {
        "fail"
    };

    EvalResult {
        task: task.task.clone(),
        chapter: task.chapter.clone(),
        status: status.to_string(),
        before: Some(serde_json::json!({
            "objective": objective,
            "open_promise_keywords": open_promise_keywords,
        })),
        after: Some(serde_json::json!({
            "scene_plan": plan,
            "selected_rules": packet.craft_rules,
        })),
        delta: Some(serde_json::json!({
            "missing_rules": missing_rules,
            "missing_payoffs": missing_payoffs,
            "hook_ok": hook_ok,
        })),
        message: format!(
            "planning_review missing_rules={:?}, missing_payoffs={:?}, hook_ok={}",
            missing_rules, missing_payoffs, hook_ok
        ),
    }
}

fn run_promise_progression_eval(task: &EvalTask, fixture: &serde_json::Value) -> EvalResult {
    let chapter_text = fixture["chapters"][&task.chapter].as_str().unwrap_or("");
    let promises = fixture["promises"]
        .as_array()
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    let mut progressed = Vec::new();
    let mut missing = Vec::new();
    for promise in &promises {
        if promise["status"].as_str() != Some("open") {
            continue;
        }
        let title = promise["title"].as_str().unwrap_or("promise");
        let keyword_hit = promise["keyword"]
            .as_str()
            .is_some_and(|keyword| chapter_text.contains(keyword));
        let progress_hit = promise["progress_markers"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|value| value.as_str())
            .any(|marker| chapter_text.contains(marker));
        if keyword_hit && progress_hit {
            progressed.push(title.to_string());
        } else {
            missing.push(title.to_string());
        }
    }
    let min_progressed = task.expected["min_progressed"].as_u64().unwrap_or(1) as usize;
    let required_titles = task.expected["required_promises"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|value| value.as_str())
        .collect::<Vec<_>>();
    let missing_required = required_titles
        .iter()
        .filter(|title| !progressed.iter().any(|progressed| progressed == **title))
        .map(|title| (*title).to_string())
        .collect::<Vec<_>>();
    let status = if progressed.len() >= min_progressed && missing_required.is_empty() {
        "pass"
    } else {
        "fail"
    };

    EvalResult {
        task: task.task.clone(),
        chapter: task.chapter.clone(),
        status: status.to_string(),
        before: Some(serde_json::json!({
            "open_promises": promises,
        })),
        after: Some(serde_json::json!({
            "progressed": progressed,
            "missing": missing,
        })),
        delta: Some(serde_json::json!({
            "progressed_count": progressed.len(),
            "missing_required": missing_required,
        })),
        message: format!(
            "promise_progression progressed={:?}, missing_required={:?}",
            progressed, missing_required
        ),
    }
}

fn quality_signals_from_fixture(fixture: &serde_json::Value) -> ChapterQualitySignals {
    let mut anchors = Vec::new();
    for entry in fixture["lorebook"].as_array().into_iter().flatten() {
        if let Some(keyword) = entry["keyword"].as_str() {
            push_unique(&mut anchors, keyword);
        }
    }
    for outline in fixture["outline"].as_array().into_iter().flatten() {
        if let Some(summary) = outline["summary"].as_str() {
            for token in ["寒影剑", "林墨", "青云宗", "执事", "代价", "选择"] {
                if summary.contains(token) {
                    push_unique(&mut anchors, token);
                }
            }
        }
    }

    ChapterQualitySignals {
        anchor_keywords: anchors,
        author_voice: Some(AuthorVoiceSnapshot {
            voice_id: "fixture-voice".to_string(),
            rhythm: VoiceRhythm {
                avg_sentence_length: 28.0,
                sentence_variance: 8.0,
                paragraph_pacing: "medium".to_string(),
            },
            diction: VoiceDiction {
                register: "formal".to_string(),
                sensory_density: 0.5,
                subtext_ratio: 0.3,
            },
            pov: "third_person_limited".to_string(),
            dialogue_texture: "subtext_heavy".to_string(),
            sentence_shape: vec!["short action beats mixed with reflective consequence".to_string()],
            taboo_phrases: Vec::new(),
            confidence: 0.8,
            sample_refs: vec![
                "fixture:chapter:第一章".to_string(),
                "fixture:chapter:第二章".to_string(),
            ],
            last_updated_ms: 0,
        }),
    }
}

fn push_unique(values: &mut Vec<String>, value: &str) {
    let value = value.trim();
    if value.is_empty() {
        return;
    }
    if !values.iter().any(|existing| existing == value) {
        values.push(value.to_string());
    }
}

fn metric_delta_map(
    before: &agent_writer_lib::chapter_generation::ChapterQualityReport,
    after: &agent_writer_lib::chapter_generation::ChapterQualityReport,
) -> std::collections::HashMap<String, f32> {
    before
        .metric_results
        .iter()
        .filter_map(|before_metric| {
            after
                .metric_results
                .iter()
                .find(|after_metric| after_metric.metric == before_metric.metric)
                .map(|after_metric| {
                    (
                        before_metric.metric.clone(),
                        after_metric.score - before_metric.score,
                    )
                })
        })
        .collect()
}

fn result_key(result: &EvalResult) -> String {
    format!("{}:{}", result.task, result.chapter)
}

fn json_number(value: &serde_json::Value, path: &[&str]) -> Option<f32> {
    let mut current = value;
    for segment in path {
        current = current.get(*segment)?;
    }
    current.as_f64().map(|number| number as f32)
}

fn metric_scores(value: &serde_json::Value) -> BTreeMap<String, f32> {
    value
        .get("metric_results")
        .and_then(|metric_results| metric_results.as_object())
        .map(|metric_results| {
            metric_results
                .iter()
                .filter_map(|(metric, score)| {
                    score.as_f64().map(|score| (metric.clone(), score as f32))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn add_metric_counts(target: &mut BTreeMap<String, usize>, metrics: &serde_json::Value) {
    for metric in metrics.as_array().into_iter().flatten() {
        if let Some(metric) = metric.as_str() {
            *target.entry(metric.to_string()).or_insert(0) += 1;
        }
    }
}

fn as_rule_id(value: &serde_json::Value) -> Option<&str> {
    value
        .get("ruleId")
        .or_else(|| value.get("rule_id"))
        .and_then(|rule_id| rule_id.as_str())
        .filter(|rule_id| !rule_id.trim().is_empty())
}

fn craft_rule_mut<'a>(
    trends: &'a mut BTreeMap<String, CraftRuleEvalTrend>,
    rule_id: &str,
) -> &'a mut CraftRuleEvalTrend {
    trends
        .entry(rule_id.to_string())
        .or_insert_with(|| CraftRuleEvalTrend {
            rule_id: rule_id.to_string(),
            ..CraftRuleEvalTrend::default()
        })
}

fn collect_craft_rule_trend_samples(
    value: &serde_json::Value,
    trends: &mut BTreeMap<String, CraftRuleEvalTrend>,
    score_deltas: &mut BTreeMap<String, Vec<f32>>,
) {
    for update in value
        .get("craft_memory_updates")
        .and_then(|updates| updates.as_array())
        .into_iter()
        .flatten()
    {
        let Some(rule_id) = as_rule_id(update) else {
            continue;
        };
        let trend = craft_rule_mut(trends, rule_id);
        let decision = update
            .get("decision")
            .and_then(|decision| decision.as_str())
            .unwrap_or("");
        if decision.contains("accepted") {
            trend.accepted_updates += 1;
        } else if decision.contains("rejected") {
            trend.rejected_updates += 1;
        }
        add_metric_counts(&mut trend.matched_metrics, &update["matchedMetrics"]);
        add_metric_counts(&mut trend.matched_metrics, &update["matched_metrics"]);
        if let (Some(before), Some(after)) = (
            json_number(update, &["scoreBefore"])
                .or_else(|| json_number(update, &["score_before"])),
            json_number(update, &["scoreAfter"]).or_else(|| json_number(update, &["score_after"])),
        ) {
            score_deltas
                .entry(rule_id.to_string())
                .or_default()
                .push(after - before);
        }
    }

    for example in value
        .get("examples")
        .and_then(|examples| examples.as_array())
        .into_iter()
        .flatten()
    {
        let Some(rule_id) = as_rule_id(example) else {
            continue;
        };
        let trend = craft_rule_mut(trends, rule_id);
        trend.stored_examples += 1;
        if let Some(delta) =
            json_number(example, &["scoreDelta"]).or_else(|| json_number(example, &["score_delta"]))
        {
            score_deltas
                .entry(rule_id.to_string())
                .or_default()
                .push(delta);
        }
    }

    for bad_pattern in value
        .get("bad_patterns")
        .or_else(|| value.get("badPatterns"))
        .and_then(|bad_patterns| bad_patterns.as_array())
        .into_iter()
        .flatten()
    {
        if let Some(rule_id) = as_rule_id(bad_pattern) {
            craft_rule_mut(trends, rule_id).stored_bad_patterns += 1;
        }
    }

    for example in value
        .get("memory_examples")
        .or_else(|| value.get("memoryExamples"))
        .and_then(|examples| examples.as_array())
        .into_iter()
        .flatten()
    {
        let Some(rule_id) = as_rule_id(example) else {
            continue;
        };
        let trend = craft_rule_mut(trends, rule_id);
        trend.prompt_examples += 1;
        if let Some(delta) =
            json_number(example, &["scoreDelta"]).or_else(|| json_number(example, &["score_delta"]))
        {
            score_deltas
                .entry(rule_id.to_string())
                .or_default()
                .push(delta);
        }
    }

    for bad_pattern in value
        .get("memory_bad_patterns")
        .or_else(|| value.get("memoryBadPatterns"))
        .and_then(|bad_patterns| bad_patterns.as_array())
        .into_iter()
        .flatten()
    {
        if let Some(rule_id) = as_rule_id(bad_pattern) {
            craft_rule_mut(trends, rule_id).prompt_bad_patterns += 1;
        }
    }
}

fn average(values: &[f32]) -> Option<f32> {
    if values.is_empty() {
        None
    } else {
        Some(values.iter().sum::<f32>() / values.len() as f32)
    }
}

fn load_previous_eval_run(path: &Path) -> Option<PreviousEvalRun> {
    let text = std::fs::read_to_string(path).ok()?;
    let mut timestamp = String::new();
    let mut results = Vec::new();
    for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if value.get("run").and_then(|run| run.as_str()) == Some("eval") {
            timestamp = value
                .get("timestamp")
                .and_then(|timestamp| timestamp.as_str())
                .unwrap_or("")
                .to_string();
            continue;
        }
        if value.get("summary").and_then(|summary| summary.as_bool()) == Some(true) {
            continue;
        }
        if let Ok(result) = serde_json::from_value::<EvalResult>(value) {
            results.push(result);
        }
    }
    if results.is_empty() {
        None
    } else {
        Some(PreviousEvalRun { timestamp, results })
    }
}

fn build_eval_run_trend(timestamp: String, results: &[EvalResult]) -> EvalRunTrend {
    let pass = results
        .iter()
        .filter(|result| result.status == "pass")
        .count();
    let fail = results.len().saturating_sub(pass);
    let mut after_scores = Vec::new();
    let mut score_deltas = Vec::new();
    let mut metric_totals = BTreeMap::<String, (f32, usize)>::new();
    let mut craft_rule_trends = BTreeMap::<String, CraftRuleEvalTrend>::new();
    let mut craft_rule_score_deltas = BTreeMap::<String, Vec<f32>>::new();
    let mut task_status = BTreeMap::new();
    let mut task_after_score = BTreeMap::new();
    let mut failing_tasks = Vec::new();

    for result in results {
        let key = result_key(result);
        task_status.insert(key.clone(), result.status.clone());
        if result.status != "pass" {
            failing_tasks.push(key.clone());
        }

        if let Some(after) = result.after.as_ref() {
            if let Some(score) = json_number(after, &["overall_score"]) {
                after_scores.push(score);
                task_after_score.insert(key.clone(), score);
            } else if let Some(score) = json_number(after, &["score"]) {
                after_scores.push(score);
                task_after_score.insert(key.clone(), score);
            }

            for (metric, score) in metric_scores(after) {
                let entry = metric_totals.entry(metric).or_insert((0.0, 0));
                entry.0 += score;
                entry.1 += 1;
            }
            collect_craft_rule_trend_samples(
                after,
                &mut craft_rule_trends,
                &mut craft_rule_score_deltas,
            );
        }

        if let Some(delta) = result.delta.as_ref() {
            if let Some(score_delta) = json_number(delta, &["overall_score"]) {
                score_deltas.push(score_delta);
            }
            collect_craft_rule_trend_samples(
                delta,
                &mut craft_rule_trends,
                &mut craft_rule_score_deltas,
            );
        }
    }

    let metric_after_average = metric_totals
        .into_iter()
        .filter_map(|(metric, (total, count))| {
            if count > 0 {
                Some((metric, total / count as f32))
            } else {
                None
            }
        })
        .collect();
    for (rule_id, deltas) in craft_rule_score_deltas {
        let trend = craft_rule_mut(&mut craft_rule_trends, &rule_id);
        trend.score_delta_sample_count = deltas.len();
        trend.average_score_delta = average(&deltas);
    }

    EvalRunTrend {
        timestamp,
        task_count: results.len(),
        pass,
        fail,
        average_after_score: average(&after_scores),
        average_score_delta: average(&score_deltas),
        metric_after_average,
        craft_rule_trends,
        task_status,
        task_after_score,
        failing_tasks,
    }
}

fn option_delta(current: Option<f32>, previous: Option<f32>) -> Option<f32> {
    match (current, previous) {
        (Some(current), Some(previous)) => Some(current - previous),
        _ => None,
    }
}

fn usize_delta(current: usize, previous: usize) -> isize {
    current as isize - previous as isize
}

fn build_eval_trend_report(
    current: EvalRunTrend,
    previous: Option<EvalRunTrend>,
) -> EvalTrendReport {
    let Some(previous_trend) = previous else {
        return EvalTrendReport {
            current,
            previous: None,
            delta: None,
            regressions: Vec::new(),
        };
    };

    let mut metric_after_average_delta = BTreeMap::new();
    for (metric, current_score) in &current.metric_after_average {
        if let Some(previous_score) = previous_trend.metric_after_average.get(metric) {
            metric_after_average_delta.insert(metric.clone(), current_score - previous_score);
        }
    }
    let craft_rule_ids = current
        .craft_rule_trends
        .keys()
        .chain(previous_trend.craft_rule_trends.keys())
        .cloned()
        .collect::<BTreeSet<_>>();
    let mut craft_rule_trend_delta = BTreeMap::new();
    for rule_id in craft_rule_ids {
        let current_rule = current.craft_rule_trends.get(&rule_id);
        let previous_rule = previous_trend.craft_rule_trends.get(&rule_id);
        craft_rule_trend_delta.insert(
            rule_id,
            CraftRuleEvalTrendDelta {
                accepted_updates_delta: usize_delta(
                    current_rule
                        .map(|trend| trend.accepted_updates)
                        .unwrap_or_default(),
                    previous_rule
                        .map(|trend| trend.accepted_updates)
                        .unwrap_or_default(),
                ),
                rejected_updates_delta: usize_delta(
                    current_rule
                        .map(|trend| trend.rejected_updates)
                        .unwrap_or_default(),
                    previous_rule
                        .map(|trend| trend.rejected_updates)
                        .unwrap_or_default(),
                ),
                stored_examples_delta: usize_delta(
                    current_rule
                        .map(|trend| trend.stored_examples)
                        .unwrap_or_default(),
                    previous_rule
                        .map(|trend| trend.stored_examples)
                        .unwrap_or_default(),
                ),
                stored_bad_patterns_delta: usize_delta(
                    current_rule
                        .map(|trend| trend.stored_bad_patterns)
                        .unwrap_or_default(),
                    previous_rule
                        .map(|trend| trend.stored_bad_patterns)
                        .unwrap_or_default(),
                ),
                prompt_examples_delta: usize_delta(
                    current_rule
                        .map(|trend| trend.prompt_examples)
                        .unwrap_or_default(),
                    previous_rule
                        .map(|trend| trend.prompt_examples)
                        .unwrap_or_default(),
                ),
                prompt_bad_patterns_delta: usize_delta(
                    current_rule
                        .map(|trend| trend.prompt_bad_patterns)
                        .unwrap_or_default(),
                    previous_rule
                        .map(|trend| trend.prompt_bad_patterns)
                        .unwrap_or_default(),
                ),
                average_score_delta_delta: option_delta(
                    current_rule.and_then(|trend| trend.average_score_delta),
                    previous_rule.and_then(|trend| trend.average_score_delta),
                ),
            },
        );
    }

    let delta = EvalTrendDelta {
        pass_delta: current.pass as isize - previous_trend.pass as isize,
        fail_delta: current.fail as isize - previous_trend.fail as isize,
        average_after_score_delta: option_delta(
            current.average_after_score,
            previous_trend.average_after_score,
        ),
        average_score_delta_delta: option_delta(
            current.average_score_delta,
            previous_trend.average_score_delta,
        ),
        metric_after_average_delta,
        craft_rule_trend_delta,
    };

    let mut regressions = Vec::new();
    for (task, status) in &current.task_status {
        if status == "pass" {
            continue;
        }
        let previous_status = previous_trend
            .task_status
            .get(task)
            .map(String::as_str)
            .unwrap_or("missing");
        if previous_status == "pass" {
            regressions.push(EvalTrendRegression {
                kind: "task_status".to_string(),
                subject: task.clone(),
                previous: serde_json::json!(previous_status),
                current: serde_json::json!(status),
                message: format!("{task} regressed from pass to {status}"),
            });
        }
    }

    if let Some(score_delta) = delta.average_after_score_delta {
        if score_delta < -TREND_REGRESSION_THRESHOLD {
            regressions.push(EvalTrendRegression {
                kind: "average_after_score".to_string(),
                subject: "all_tasks".to_string(),
                previous: serde_json::json!(previous_trend.average_after_score),
                current: serde_json::json!(current.average_after_score),
                message: format!("average after score dropped by {:.3}", score_delta),
            });
        }
    }

    for (metric, metric_delta) in &delta.metric_after_average_delta {
        if *metric_delta < -TREND_REGRESSION_THRESHOLD {
            regressions.push(EvalTrendRegression {
                kind: "metric_after_average".to_string(),
                subject: metric.clone(),
                previous: serde_json::json!(previous_trend.metric_after_average.get(metric)),
                current: serde_json::json!(current.metric_after_average.get(metric)),
                message: format!("{metric} average dropped by {:.3}", metric_delta),
            });
        }
    }
    for (rule_id, rule_delta) in &delta.craft_rule_trend_delta {
        if let Some(score_delta) = rule_delta.average_score_delta_delta {
            if score_delta < -TREND_REGRESSION_THRESHOLD {
                regressions.push(EvalTrendRegression {
                    kind: "craft_rule_average_score_delta".to_string(),
                    subject: rule_id.clone(),
                    previous: serde_json::json!(previous_trend
                        .craft_rule_trends
                        .get(rule_id)
                        .and_then(|trend| trend.average_score_delta)),
                    current: serde_json::json!(current
                        .craft_rule_trends
                        .get(rule_id)
                        .and_then(|trend| trend.average_score_delta)),
                    message: format!(
                        "{rule_id} craft rule average score delta dropped by {:.3}",
                        score_delta
                    ),
                });
            }
        }
    }

    EvalTrendReport {
        current,
        previous: Some(previous_trend),
        delta: Some(delta),
        regressions,
    }
}

fn run_continuity_diagnostic_eval(task: &EvalTask, fixture: &serde_json::Value) -> EvalResult {
    let chapter_text = fixture["chapters"][&task.chapter].as_str().unwrap_or("");
    let lorebook = fixture["lorebook"].as_array().unwrap();

    let mut missing = Vec::new();
    for entry in lorebook {
        let keyword = entry["keyword"].as_str().unwrap_or("");
        if !chapter_text.contains(keyword) {
            missing.push(keyword.to_string());
        }
    }

    let expected_conflict = task.expected["canon_conflict"].as_bool().unwrap_or(false);
    let actual_conflict = !missing.is_empty();

    let status = if expected_conflict == actual_conflict {
        "pass"
    } else {
        "fail"
    };

    let message = format!(
        "check='{}', lore_entities={:?}, missing={:?}",
        task.check.as_deref().unwrap_or(""),
        task.expected["lore_entities"],
        missing
    );

    EvalResult {
        task: task.task.clone(),
        chapter: task.chapter.clone(),
        status: status.to_string(),
        before: None,
        after: None,
        delta: None,
        message,
    }
}

fn main() {
    let fixture = load_fixture();
    let tasks = load_tasks();
    let output_dir = fixture_dir();
    let output_path = output_dir.join("eval_output.jsonl");
    let trend_path = output_dir.join("eval_trend.json");
    let previous_run = load_previous_eval_run(&output_path);

    let mut results = Vec::new();
    for task in &tasks {
        let result = match task.task.as_str() {
            "chapter_generation" => run_chapter_generation_eval(task, &fixture),
            "quality_evaluation" => run_quality_evaluation_eval(task, &fixture),
            "quality_signals" => run_quality_signal_eval(task, &fixture),
            "targeted_revision" => run_targeted_revision_eval(task, &fixture),
            "craft_memory" => run_craft_memory_eval(task, &fixture),
            "manual_craft_edit" => run_manual_craft_edit_eval(task, &fixture),
            "craft_memory_prompt" => run_craft_memory_prompt_eval(task, &fixture),
            "canon_conflict" => run_canon_conflict_eval(task, &fixture),
            "planning_review" => run_planning_review_eval(task, &fixture),
            "promise_progression" => run_promise_progression_eval(task, &fixture),
            "continuity_diagnostic" => run_continuity_diagnostic_eval(task, &fixture),
            other => EvalResult {
                task: task.task.clone(),
                chapter: task.chapter.clone(),
                status: "skipped".to_string(),
                before: None,
                after: None,
                delta: None,
                message: format!("Unknown task type: {}", other),
            },
        };
        results.push(result);
    }

    let mut lines = Vec::new();
    let timestamp = chrono::Utc::now().to_rfc3339();
    // Header with run metadata
    lines.push(serde_json::json!({
        "run": "eval",
        "timestamp": timestamp,
        "task_count": tasks.len(),
    }));

    for result in &results {
        lines.push(serde_json::to_value(result).expect("serialize result"));
    }

    // Summary line
    let pass_count = results.iter().filter(|r| r.status == "pass").count();
    lines.push(serde_json::json!({
        "summary": true,
        "total": results.len(),
        "pass": pass_count,
        "fail": results.len() - pass_count,
    }));

    let output = lines
        .iter()
        .map(|l| serde_json::to_string(l).unwrap())
        .collect::<Vec<_>>()
        .join("\n");

    std::fs::write(&output_path, output).expect("write eval_output.jsonl");

    let current_trend = build_eval_run_trend(timestamp, &results);
    let previous_trend = previous_run.map(|run| build_eval_run_trend(run.timestamp, &run.results));
    let trend_report = build_eval_trend_report(current_trend, previous_trend);
    let trend_output =
        serde_json::to_string_pretty(&trend_report).expect("serialize eval trend report");
    std::fs::write(&trend_path, trend_output).expect("write eval_trend.json");

    println!("Writing eval complete: {}", output_path.display());
    println!("Trend report: {}", trend_path.display());
    println!(
        "  tasks: {}, pass: {}, fail: {}",
        results.len(),
        pass_count,
        results.len() - pass_count
    );
    for r in &results {
        println!("  [{}] {} {}: {}", r.status, r.task, r.chapter, r.message);
    }
    if !trend_report.regressions.is_empty() {
        println!("  regressions: {}", trend_report.regressions.len());
        for regression in &trend_report.regressions {
            println!("    - {}", regression.message);
        }
    }
    if pass_count != results.len() || !trend_report.regressions.is_empty() {
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eval_result_with_score(task: &str, chapter: &str, status: &str, score: f32) -> EvalResult {
        EvalResult {
            task: task.to_string(),
            chapter: chapter.to_string(),
            status: status.to_string(),
            before: None,
            after: Some(serde_json::json!({
                "overall_score": score,
                "metric_results": {
                    "dialogue_function": score,
                },
            })),
            delta: Some(serde_json::json!({
                "overall_score": 0.0,
            })),
            message: String::new(),
        }
    }

    fn eval_result_with_delta(task: &str, chapter: &str, delta: serde_json::Value) -> EvalResult {
        EvalResult {
            task: task.to_string(),
            chapter: chapter.to_string(),
            status: "pass".to_string(),
            before: None,
            after: None,
            delta: Some(delta),
            message: String::new(),
        }
    }

    #[test]
    fn eval_trend_reports_status_and_metric_regressions() {
        let previous = build_eval_run_trend(
            "previous".to_string(),
            &[eval_result_with_score(
                "quality_evaluation",
                "第二章",
                "pass",
                0.8,
            )],
        );
        let current = build_eval_run_trend(
            "current".to_string(),
            &[eval_result_with_score(
                "quality_evaluation",
                "第二章",
                "fail",
                0.6,
            )],
        );

        let report = build_eval_trend_report(current, Some(previous));

        assert_eq!(report.current.pass, 0);
        assert_eq!(report.current.fail, 1);
        assert!(report.delta.as_ref().is_some_and(|delta| {
            delta.pass_delta == -1
                && delta.fail_delta == 1
                && delta
                    .metric_after_average_delta
                    .get("dialogue_function")
                    .is_some_and(|delta| *delta < -TREND_REGRESSION_THRESHOLD)
        }));
        assert!(report
            .regressions
            .iter()
            .any(|regression| regression.kind == "task_status"));
        assert!(report
            .regressions
            .iter()
            .any(|regression| regression.kind == "metric_after_average"));
    }

    #[test]
    fn eval_trend_groups_craft_memory_evidence_by_rule() {
        let results = vec![
            EvalResult {
                task: "craft_memory".to_string(),
                chapter: "第二章".to_string(),
                status: "pass".to_string(),
                before: None,
                after: Some(serde_json::json!({
                    "examples": [{
                        "ruleId": "dialogue_function",
                        "scoreDelta": 0.42
                    }],
                    "bad_patterns": [{
                        "ruleId": "dialogue_function",
                        "rejectedCount": 2
                    }]
                })),
                delta: None,
                message: String::new(),
            },
            EvalResult {
                task: "craft_memory_prompt".to_string(),
                chapter: "第二章".to_string(),
                status: "pass".to_string(),
                before: None,
                after: Some(serde_json::json!({
                    "memory_examples": [{
                        "ruleId": "dialogue_function",
                        "scoreDelta": 0.40
                    }],
                    "memory_bad_patterns": [{
                        "ruleId": "dialogue_function"
                    }]
                })),
                delta: None,
                message: String::new(),
            },
            eval_result_with_delta(
                "manual_craft_edit",
                "第二章",
                serde_json::json!({
                    "craft_memory_updates": [{
                        "ruleId": "scene_objective",
                        "decision": "author_manual_edit_accepted",
                        "matchedMetrics": ["scene_causality"],
                        "scoreBefore": 0.50,
                        "scoreAfter": 0.90
                    }]
                }),
            ),
        ];

        let trend = build_eval_run_trend("current".to_string(), &results);
        let dialogue = trend
            .craft_rule_trends
            .get("dialogue_function")
            .expect("dialogue rule trend");
        assert_eq!(dialogue.stored_examples, 1);
        assert_eq!(dialogue.stored_bad_patterns, 1);
        assert_eq!(dialogue.prompt_examples, 1);
        assert_eq!(dialogue.prompt_bad_patterns, 1);
        assert_eq!(dialogue.score_delta_sample_count, 2);
        assert!(dialogue
            .average_score_delta
            .is_some_and(|score| score > 0.40 && score < 0.43));

        let scene = trend
            .craft_rule_trends
            .get("scene_objective")
            .expect("scene rule trend");
        assert_eq!(scene.accepted_updates, 1);
        assert_eq!(scene.matched_metrics.get("scene_causality"), Some(&1));
        assert!(scene
            .average_score_delta
            .is_some_and(|score| score > 0.39 && score < 0.41));
    }

    #[test]
    fn eval_trend_reports_craft_rule_score_regressions() {
        let previous = build_eval_run_trend(
            "previous".to_string(),
            &[eval_result_with_delta(
                "manual_craft_edit",
                "第二章",
                serde_json::json!({
                    "craft_memory_updates": [{
                        "ruleId": "scene_objective",
                        "decision": "author_manual_edit_accepted",
                        "matchedMetrics": ["scene_causality"],
                        "scoreBefore": 0.10,
                        "scoreAfter": 0.90
                    }]
                }),
            )],
        );
        let current = build_eval_run_trend(
            "current".to_string(),
            &[eval_result_with_delta(
                "manual_craft_edit",
                "第二章",
                serde_json::json!({
                    "craft_memory_updates": [{
                        "ruleId": "scene_objective",
                        "decision": "author_manual_edit_accepted",
                        "matchedMetrics": ["scene_causality"],
                        "scoreBefore": 0.10,
                        "scoreAfter": 0.20
                    }]
                }),
            )],
        );

        let report = build_eval_trend_report(current, Some(previous));

        assert!(report.regressions.iter().any(|regression| {
            regression.kind == "craft_rule_average_score_delta"
                && regression.subject == "scene_objective"
        }));
    }
}
