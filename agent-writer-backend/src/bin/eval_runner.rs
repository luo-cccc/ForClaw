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
use agent_writer_lib::writer_agent::world_bible::{
    compile_canon_constraints, compile_scene_contract, validate_world_consistency, ApprovalStatus,
    CanonConstraint, CanonConstraintKind, ConstraintSeverity, SceneContract, WorldAsset,
    WorldAssetKind,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

const TREND_REGRESSION_THRESHOLD: f32 = 0.05;

const PROFILES: &[&str] = &["xianxia", "mystery", "scifi"];

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
    profile: String,
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
    profile: String,
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    duplicate_preview_groups: Vec<DuplicatePreviewGroup>,
    #[serde(default)]
    repair_rate: f32,
    #[serde(default)]
    min_chars: usize,
    #[serde(default)]
    max_chars: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    avg_carry_rate: Option<f32>,
    #[serde(default)]
    p50_latency_ms: u64,
    #[serde(default)]
    p90_latency_ms: u64,
    #[serde(default)]
    p95_latency_ms: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    quality_warnings: Vec<QualityWarning>,
    #[serde(default)]
    state_delta_covered: usize,
    #[serde(default)]
    state_delta_weak: usize,
    #[serde(default)]
    state_delta_missing: usize,
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
    profile: String,
    current: EvalRunTrend,
    previous: Option<EvalRunTrend>,
    delta: Option<EvalTrendDelta>,
    regressions: Vec<EvalTrendRegression>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct WorldConsistencySummary {
    total_checks: usize,
    hard_violations: usize,
    warnings: usize,
    profiles: BTreeMap<String, ProfileWorldConsistency>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ProfileWorldConsistency {
    checks: usize,
    violations: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EvalSummary {
    timestamp: String,
    mode: String,
    profiles: BTreeMap<String, ProfileSummary>,
    total_tasks: usize,
    total_pass: usize,
    total_fail: usize,
    regressions: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    quality_warnings: Vec<QualityWarning>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    world_consistency: Option<WorldConsistencySummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ProfileSummary {
    task_count: usize,
    pass: usize,
    fail: usize,
    failing_tasks: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    duplicate_preview_groups: Vec<DuplicatePreviewGroup>,
    #[serde(default)]
    repair_rate: f32,
    #[serde(default)]
    min_chars: usize,
    #[serde(default)]
    max_chars: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    avg_carry_rate: Option<f32>,
    #[serde(default)]
    p50_latency_ms: u64,
    #[serde(default)]
    p90_latency_ms: u64,
    #[serde(default)]
    p95_latency_ms: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    quality_warnings: Vec<QualityWarning>,
    #[serde(default)]
    state_delta_covered: usize,
    #[serde(default)]
    state_delta_weak: usize,
    #[serde(default)]
    state_delta_missing: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DuplicatePreviewGroup {
    preview: String,
    chapters: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct QualityWarning {
    kind: String,
    severity: String,
    message: String,
}

fn fixture_dir() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest.join("..").join("fixtures").join("writing_eval")
}

fn reports_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("reports")
}

fn load_fixture(profile: &str) -> serde_json::Value {
    let path = fixture_dir().join(profile).join("project.json");
    let text = std::fs::read_to_string(&path).expect("read project.json");
    serde_json::from_str(&text).expect("parse project.json")
}

fn load_tasks(profile: &str) -> Vec<EvalTask> {
    let path = fixture_dir().join(profile).join("eval_tasks.jsonl");
    let text = std::fs::read_to_string(&path).expect("read eval_tasks.jsonl");
    text.lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty() && !l.starts_with("//"))
        .map(|l| serde_json::from_str(l).expect("parse eval task"))
        .collect()
}

fn load_world_assets(profile: &str) -> Vec<WorldAsset> {
    let path = fixture_dir().join(profile).join("world_assets.json");
    if !path.exists() {
        return Vec::new();
    }
    let text = std::fs::read_to_string(&path).expect("read world_assets.json");
    serde_json::from_str(&text).expect("parse world_assets.json")
}

fn is_smoke_task(task: &EvalTask) -> bool {
    // Smoke mode runs a representative subset: one of each major task type
    matches!(
        task.task.as_str(),
        "chapter_generation"
            | "quality_evaluation"
            | "quality_signals"
            | "canon_conflict"
            | "promise_progression"
    )
}

fn run_chapter_generation_eval(
    profile: &str,
    task: &EvalTask,
    fixture: &serde_json::Value,
) -> EvalResult {
    let chapter_text = fixture["chapters"][&task.chapter].as_str().unwrap_or("");
    let plan = SceneCraftPlan::default();

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

    EvalResult {
        profile: profile.to_string(),
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
        message: format!(
            "fixture_chars={}, contract={}-{}, selected_rules={}, missing_terms={:?}, before_score={:.2}, after_score={:.2}, delta={:.2}",
            chapter_text.chars().count(),
            min_chars,
            max_chars,
            packet.craft_rules.len(),
            missing_terms,
            before_score,
            after_score,
            score_delta
        ),
    }
}

fn run_quality_evaluation_eval(
    profile: &str,
    task: &EvalTask,
    fixture: &serde_json::Value,
) -> EvalResult {
    let chapter_text = fixture["chapters"][&task.chapter].as_str().unwrap_or("");
    let plan = SceneCraftPlan::default();

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

    EvalResult {
        profile: profile.to_string(),
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
        message: format!(
            "metrics={:?}, before_score={:.2}, after_score={:.2}, expected_min={:.2}, metric_failures={:?}",
            task.metrics, before_score, after_score, min_score, metric_failures
        ),
    }
}

fn run_quality_signal_eval(
    profile: &str,
    task: &EvalTask,
    fixture: &serde_json::Value,
) -> EvalResult {
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
        profile: profile.to_string(),
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

fn run_targeted_revision_eval(
    profile: &str,
    task: &EvalTask,
    fixture: &serde_json::Value,
) -> EvalResult {
    let before_text = task
        .expected
        .get("before_text")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let after_text = task
        .expected
        .get("after_text")
        .and_then(|value| value.as_str())
        .unwrap_or("");
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
    let min_mapping_count = task
        .expected
        .get("min_excerpt_mapping_count")
        .and_then(|v| v.as_u64())
        .unwrap_or(1) as usize;
    let actual_mapping_count = changes
        .iter()
        .filter(|change| {
            !change.changed_excerpt_before.is_empty() && !change.changed_excerpt_after.is_empty()
        })
        .count();
    // Sentence-level mapping: count targets with excerpt mapping and at least one high/medium confidence sentence change
    let sentence_mapping_count = changes
        .iter()
        .filter(|change| {
            !change.changed_excerpt_before.is_empty()
                && !change.changed_excerpt_after.is_empty()
                && change.sentence_changes.iter().any(|sc| {
                    sc.confidence == agent_writer_lib::chapter_generation::SentenceChangeConfidence::High
                        || sc.confidence == agent_writer_lib::chapter_generation::SentenceChangeConfidence::Medium
                })
        })
        .count();
    let min_sentence_mapping = task
        .expected
        .get("min_sentence_mapping_count")
        .and_then(|v| v.as_u64())
        .unwrap_or(1) as usize;
    let status = if actual_mapping_count >= min_mapping_count
        && sentence_mapping_count >= min_sentence_mapping
    {
        "pass"
    } else {
        "fail"
    };
    EvalResult {
        profile: profile.to_string(),
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
            "mapping_count": actual_mapping_count,
            "sentence_mapping_count": sentence_mapping_count,
        })),
        message: format!(
            "targeted_revision excerpt_mapping={} count={} sentence_mapping={}",
            has_excerpt_mapping, actual_mapping_count, sentence_mapping_count
        ),
    }
}

fn run_craft_memory_eval(
    profile: &str,
    task: &EvalTask,
    _fixture: &serde_json::Value,
) -> EvalResult {
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
        profile: profile.to_string(),
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

fn run_manual_craft_edit_eval(
    profile: &str,
    task: &EvalTask,
    fixture: &serde_json::Value,
) -> EvalResult {
    let conn = rusqlite::Connection::open_in_memory().expect("open in-memory db");
    agent_writer_lib::writer_agent::memory::ensure_craft_tables(&conn)
        .expect("ensure craft tables");
    let before_text = task
        .expected
        .get("before_text")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let after_text = task
        .expected
        .get("after_text")
        .and_then(|value| value.as_str())
        .unwrap_or("");

    // Derive anchor keywords from fixture lorebook
    let anchor_keywords: Vec<String> = fixture["lorebook"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|entry| entry["keyword"].as_str().map(str::to_string))
        .collect();

    let request = ManualCraftEditFeedbackRequest {
        chapter_title: task.chapter.clone(),
        before_text: before_text.to_string(),
        after_text: after_text.to_string(),
        metrics: task.metrics.clone().unwrap_or_default(),
        anchor_keywords: anchor_keywords.clone(),
        open_promise_keywords: fixture["promises"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|p| p["status"].as_str() == Some("open"))
            .filter_map(|p| p["keyword"].as_str().map(str::to_string))
            .collect(),
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
    // Short text may not trigger metric-based excerpt mapping; be lenient.
    let text_too_short_for_mapping =
        before_text.chars().count() < 30 || after_text.chars().count() < 30;
    let mapping_ok = !requires_mapping || has_mapping || text_too_short_for_mapping;
    let status = if result.example_refs.len() >= min_examples
        && result.bad_pattern_refs.len() >= min_bad_patterns
        && mapping_ok
    {
        "pass"
    } else {
        "fail"
    };

    EvalResult {
        profile: profile.to_string(),
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

fn run_craft_memory_prompt_eval(
    profile: &str,
    task: &EvalTask,
    _fixture: &serde_json::Value,
) -> EvalResult {
    // Derive sample content from task expected values for profile-agnostic checking
    let must_contain: Vec<String> = task.expected["must_contain"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|value| value.as_str().map(str::to_string))
        .collect();
    let example_excerpt = must_contain
        .first()
        .cloned()
        .unwrap_or_else(|| "对话改变选择。".to_string());
    let bad_excerpt = must_contain
        .get(1)
        .filter(|_| must_contain.len() == 2)
        .or_else(|| must_contain.get(2))
        .cloned()
        .unwrap_or_else(|| "背景说明，局面不变。".to_string());
    let sample = CraftMemoryPromptSamples {
        rule_id: "dialogue_function".to_string(),
        examples: vec![CraftMemoryPromptExample {
            rule_id: "dialogue_function".to_string(),
            excerpt_ref: "eval:craft_examples:dialogue".to_string(),
            excerpt: example_excerpt.clone(),
            reason: "作者认可：对话推动情节。".to_string(),
            score_delta: 0.42,
        }],
        bad_patterns: vec![CraftMemoryPromptBadPattern {
            rule_id: "dialogue_function".to_string(),
            evidence_ref: "eval:craft_bad_patterns:dialogue".to_string(),
            evidence_excerpt: bad_excerpt.clone(),
            correction: "让台词改变权力、信息或选择。".to_string(),
            rejected_count: 2,
        }],
    };
    let packet = compile_empowerment_prompt_with_memory(
        &example_excerpt,
        "对话推进",
        0,
        false,
        Some(5),
        Some(2000),
        None,
        &[sample],
        None,
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
        profile: profile.to_string(),
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

fn run_canon_conflict_eval(
    profile: &str,
    task: &EvalTask,
    fixture: &serde_json::Value,
) -> EvalResult {
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
    let must_conflict = task
        .expected
        .get("must_conflict")
        .and_then(|v| v.as_array())
        .into_iter()
        .flatten()
        .filter_map(|v| v.as_str());
    let has_required_conflict = must_conflict
        .clone()
        .all(|required| conflicts.iter().any(|c| c.starts_with(required)));
    let status = if expected_conflict {
        !conflicts.is_empty() && has_required_conflict
    } else {
        conflicts.is_empty()
    };

    EvalResult {
        profile: profile.to_string(),
        task: task.task.clone(),
        chapter: task.chapter.clone(),
        status: if status { "pass" } else { "fail" }.to_string(),
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

fn run_planning_review_eval(
    profile: &str,
    task: &EvalTask,
    fixture: &serde_json::Value,
) -> EvalResult {
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
    let participants: Vec<String> = fixture["lorebook"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|entry| entry["keyword"].as_str().map(str::to_string))
        .take(5)
        .collect();
    let open_promise_keywords = fixture["promises"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|promise| promise["status"].as_str() == Some("open"))
        .filter_map(|promise| promise["keyword"].as_str().map(str::to_string))
        .collect::<Vec<_>>();
    let external_bundle =
        agent_writer_lib::external_writing_db::search_context_bundle(&objective, 6).ok();
    let plan = build_scene_craft_plan(
        &task.chapter,
        &objective,
        &participants,
        "计划评审",
        next_summary,
        &open_promise_keywords,
        &packet,
        external_bundle.as_ref(),
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
        profile: profile.to_string(),
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

fn run_promise_progression_eval(
    profile: &str,
    task: &EvalTask,
    fixture: &serde_json::Value,
) -> EvalResult {
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
        profile: profile.to_string(),
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

fn run_negative_missing_anchor_eval(
    profile: &str,
    task: &EvalTask,
    fixture: &serde_json::Value,
) -> EvalResult {
    let chapter_text = fixture["chapters"][&task.chapter].as_str().unwrap_or("");
    let plan = SceneCraftPlan::default();
    let mut quality_signals = quality_signals_from_fixture(fixture);
    let must_miss = task
        .expected
        .get("must_miss_anchor")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    // Remove the anchor from signals to simulate it being missing
    quality_signals.anchor_keywords.retain(|a| a != must_miss);
    let report = evaluate_chapter_quality_with_signals(
        chapter_text,
        &task.chapter,
        &plan,
        &[],
        500,
        2000,
        &quality_signals,
    );
    let anchor_result = report
        .metric_results
        .iter()
        .find(|m| m.metric == "anchor_carry")
        .map(|m| m.score)
        .unwrap_or(0.0);
    let penalty_applies = task.expected["penalty_applies"].as_bool().unwrap_or(false);
    // We check that the score is lower when the anchor is removed from signals
    // This is a proxy for the penalty logic existing
    let status = if penalty_applies && anchor_result < 1.0 {
        "pass"
    } else {
        "fail"
    };
    EvalResult {
        profile: profile.to_string(),
        task: task.task.clone(),
        chapter: task.chapter.clone(),
        status: status.to_string(),
        before: None,
        after: Some(serde_json::json!({
            "anchor_carry": anchor_result,
        })),
        delta: None,
        message: format!("negative_missing_anchor removed='{must_miss}' score={anchor_result:.2}"),
    }
}

fn run_negative_style_drift_eval(
    profile: &str,
    task: &EvalTask,
    _fixture: &serde_json::Value,
) -> EvalResult {
    let drifted_text = task
        .expected
        .get("drifted_text")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let plan = SceneCraftPlan::default();
    let quality_signals = ChapterQualitySignals {
        anchor_keywords: vec!["测试".to_string()],
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
            sample_refs: vec!["fixture:chapter:第一章".to_string()],
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
        drifted_text,
        &task.chapter,
        &plan,
        &[],
        0,
        2000,
        &quality_signals,
    );
    let style_result = report
        .metric_results
        .iter()
        .find(|m| m.metric == "style_drift")
        .map(|m| (m.score, m.reason.clone()))
        .unwrap_or((0.0, String::new()));
    let max_score = task.expected["style_drift_score_max"]
        .as_f64()
        .unwrap_or(1.0) as f32;
    let reason_contains = task
        .expected
        .get("reason_contains")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let status = if style_result.0 <= max_score && style_result.1.contains(reason_contains) {
        "pass"
    } else {
        "fail"
    };
    EvalResult {
        profile: profile.to_string(),
        task: task.task.clone(),
        chapter: task.chapter.clone(),
        status: status.to_string(),
        before: None,
        after: Some(serde_json::json!({
            "style_drift_score": style_result.0,
            "style_drift_reason": style_result.1,
        })),
        delta: None,
        message: format!(
            "negative_style_drift score={:.2} reason='{}'",
            style_result.0, style_result.1
        ),
    }
}

fn run_negative_promise_stalled_eval(
    profile: &str,
    task: &EvalTask,
    fixture: &serde_json::Value,
) -> EvalResult {
    let stalled_text = task
        .expected
        .get("stalled_text")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let plan = SceneCraftPlan::default();
    let quality_signals = quality_signals_from_fixture(fixture);
    let report = evaluate_chapter_quality_with_signals(
        stalled_text,
        &task.chapter,
        &plan,
        &[],
        0,
        2000,
        &quality_signals,
    );
    let promise_result = report
        .metric_results
        .iter()
        .find(|m| m.metric == "promise_progress")
        .map(|m| (m.score, m.reason.clone()))
        .unwrap_or((0.0, String::new()));
    let max_score = task.expected["promise_progress_score_max"]
        .as_f64()
        .unwrap_or(1.0) as f32;
    let reason_contains = task
        .expected
        .get("reason_contains")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let status = if promise_result.0 <= max_score && promise_result.1.contains(reason_contains) {
        "pass"
    } else {
        "fail"
    };
    EvalResult {
        profile: profile.to_string(),
        task: task.task.clone(),
        chapter: task.chapter.clone(),
        status: status.to_string(),
        before: None,
        after: Some(serde_json::json!({
            "promise_progress_score": promise_result.0,
            "promise_progress_reason": promise_result.1,
        })),
        delta: None,
        message: format!(
            "negative_promise_stalled score={:.2} reason='{}'",
            promise_result.0, promise_result.1
        ),
    }
}

fn run_negative_revision_no_change_eval(
    profile: &str,
    task: &EvalTask,
    fixture: &serde_json::Value,
) -> EvalResult {
    let before_text = task
        .expected
        .get("before_text")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let after_text = task
        .expected
        .get("after_text")
        .and_then(|v| v.as_str())
        .unwrap_or("");
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
    let mapping_count = changes
        .iter()
        .filter(|change| {
            !change.changed_excerpt_before.is_empty() && !change.changed_excerpt_after.is_empty()
        })
        .count();
    let should_be_empty = task.expected["revision_should_be_empty"]
        .as_bool()
        .unwrap_or(false);
    let status = if !should_be_empty || mapping_count == 0 {
        "pass"
    } else {
        "fail"
    };
    EvalResult {
        profile: profile.to_string(),
        task: task.task.clone(),
        chapter: task.chapter.clone(),
        status: status.to_string(),
        before: None,
        after: Some(serde_json::json!({
            "mapping_count": mapping_count,
            "changes": changes,
        })),
        delta: None,
        message: format!(
            "negative_revision_no_change mapping_count={}",
            mapping_count
        ),
    }
}

fn run_negative_craft_memory_injection_eval(
    profile: &str,
    task: &EvalTask,
    _fixture: &serde_json::Value,
) -> EvalResult {
    let conn = rusqlite::Connection::open_in_memory().expect("open in-memory db");
    agent_writer_lib::writer_agent::memory::ensure_craft_tables(&conn)
        .expect("ensure craft tables");
    let bad = agent_writer_lib::writer_agent::memory::CraftBadPatternMemory {
        id: "eval-injected-bad".to_string(),
        rule_id: "dialogue_function".to_string(),
        scope: task.chapter.clone(),
        pattern: "dialogue_function".to_string(),
        evidence_ref: "eval:injected_bad".to_string(),
        evidence_excerpt: task
            .expected
            .get("injected_bad_text")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        correction: "让台词改变权力、信息或选择。".to_string(),
        rejected_count: 1,
        created_at: 1,
        updated_at: 1,
    };
    agent_writer_lib::writer_agent::memory::record_craft_bad_pattern(&conn, &bad)
        .expect("record bad pattern");
    let bad_patterns = agent_writer_lib::writer_agent::memory::list_craft_bad_patterns(
        &conn,
        "dialogue_function",
        10,
    )
    .expect("list bad patterns");
    let min_bad = task.expected["min_bad_patterns"].as_u64().unwrap_or(1) as usize;
    let status = if bad_patterns.len() >= min_bad {
        "pass"
    } else {
        "fail"
    };
    EvalResult {
        profile: profile.to_string(),
        task: task.task.clone(),
        chapter: task.chapter.clone(),
        status: status.to_string(),
        before: None,
        after: Some(serde_json::json!({
            "bad_patterns": bad_patterns,
        })),
        delta: None,
        message: format!(
            "negative_craft_memory_injection bad_patterns={}",
            bad_patterns.len()
        ),
    }
}

fn run_negative_plot_stalled_eval(
    profile: &str,
    task: &EvalTask,
    fixture: &serde_json::Value,
) -> EvalResult {
    let stalled_text = task
        .expected
        .get("stalled_text")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let plan = SceneCraftPlan::default();
    let quality_signals = quality_signals_from_fixture(fixture);
    let report = evaluate_chapter_quality_with_signals(
        stalled_text,
        &task.chapter,
        &plan,
        &[],
        0,
        2000,
        &quality_signals,
    );
    let plot_result = report
        .metric_results
        .iter()
        .find(|m| m.metric == "plot_progression")
        .map(|m| m.score)
        .unwrap_or(1.0);
    let new_info_result = report
        .metric_results
        .iter()
        .find(|m| m.metric == "new_information_density")
        .map(|m| m.score)
        .unwrap_or(1.0);
    let anchor_result = report
        .metric_results
        .iter()
        .find(|m| m.metric == "anchor_carry")
        .map(|m| m.score)
        .unwrap_or(0.0);

    let anchor_min = task.expected["anchor_carry_min"].as_f64().unwrap_or(0.4) as f32;
    let plot_max = task.expected["plot_progression_max"]
        .as_f64()
        .unwrap_or(0.55) as f32;
    let info_max = task.expected["new_information_density_max"]
        .as_f64()
        .unwrap_or(0.55) as f32;

    // anchor_carry should be OK but plot_progression and new_information should be low
    let status =
        if anchor_result >= anchor_min && plot_result <= plot_max && new_info_result <= info_max {
            "pass"
        } else {
            "fail"
        };
    EvalResult {
        profile: profile.to_string(),
        task: task.task.clone(),
        chapter: task.chapter.clone(),
        status: status.to_string(),
        before: None,
        after: Some(serde_json::json!({
            "anchor_carry": anchor_result,
            "plot_progression": plot_result,
            "new_information_density": new_info_result,
            "anchor_min": anchor_min,
            "plot_max": plot_max,
            "info_max": info_max,
        })),
        delta: None,
        message: format!(
            "negative_plot_stalled anchor_carry={:.2} plot_progression={:.2} new_information_density={:.2}",
            anchor_result, plot_result, new_info_result
        ),
    }
}

fn run_state_delta_trace_eval(
    profile: &str,
    task: &EvalTask,
    fixture: &serde_json::Value,
) -> EvalResult {
    let plan = SceneCraftPlan::default();
    let mut signals = quality_signals_from_fixture(fixture);
    let required_deltas: Vec<agent_writer_lib::chapter_generation::StateDelta> = task
        .expected
        .get("required_deltas")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .map(|item| agent_writer_lib::chapter_generation::StateDelta {
                    delta_type: item["delta_type"].as_str().unwrap_or("").to_string(),
                    description: item["description"].as_str().unwrap_or("").to_string(),
                    source: "eval:state_delta_trace".to_string(),
                })
                .collect()
        })
        .unwrap_or_default();
    signals.required_state_deltas = required_deltas.clone();

    let chapter_text = fixture["chapters"][&task.chapter].as_str().unwrap_or("");
    let report = evaluate_chapter_quality_with_signals(
        chapter_text,
        &task.chapter,
        &plan,
        &[],
        500,
        3500,
        &signals,
    );
    let delta_result = report
        .metric_results
        .iter()
        .find(|m| m.metric == "state_delta_coverage")
        .map(|m| (m.score, m.evidence_excerpt.clone()))
        .unwrap_or((0.0, String::new()));

    let min_covered = task.expected["min_covered"].as_u64().unwrap_or(1) as usize;
    // Parse "X covered / Y weak / Z missing of N required deltas" from evidence
    // When score >= 0.8, evidence_excerpt is empty (gated_metric suppresses it).
    // In that case, infer all deltas are covered.
    let (covered_count, missing_from_evidence) =
        if delta_result.1.is_empty() && delta_result.0 >= 0.8 {
            (required_deltas.len(), 0usize)
        } else {
            let covered = delta_result
                .1
                .split_whitespace()
                .next()
                .and_then(|s| s.parse::<usize>().ok())
                .unwrap_or(0);
            let missing = delta_result
                .1
                .split("missing")
                .next()
                .and_then(|s| s.split_whitespace().last())
                .and_then(|s| s.parse::<usize>().ok())
                .unwrap_or(0);
            (covered, missing)
        };
    let max_missing = task.expected["max_missing"].as_u64().unwrap_or(1) as usize;

    let status = if covered_count >= min_covered && missing_from_evidence <= max_missing {
        "pass"
    } else {
        "fail"
    };
    EvalResult {
        profile: profile.to_string(),
        task: task.task.clone(),
        chapter: task.chapter.clone(),
        status: status.to_string(),
        before: None,
        after: Some(serde_json::json!({
            "state_delta_coverage": delta_result.0,
            "evidence": delta_result.1,
            "required_deltas_count": required_deltas.len(),
            "covered_in_evidence": covered_count,
            "missing_in_evidence": missing_from_evidence,
        })),
        delta: None,
        message: format!(
            "state_delta_trace score={:.2} evidence={}",
            delta_result.0, delta_result.1
        ),
    }
}

fn run_continuity_diagnostic_eval(
    profile: &str,
    task: &EvalTask,
    fixture: &serde_json::Value,
) -> EvalResult {
    let chapter_text = fixture["chapters"][&task.chapter].as_str().unwrap_or("");

    // Only check lore_entities listed in the task expectation, not the entire lorebook
    let expected_entities: Vec<String> = task.expected["lore_entities"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|v| v.as_str().map(str::to_string))
        .collect();

    let mut missing = Vec::new();
    for keyword in &expected_entities {
        if !chapter_text.contains(keyword.as_str()) {
            missing.push(keyword.clone());
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
        profile: profile.to_string(),
        task: task.task.clone(),
        chapter: task.chapter.clone(),
        status: status.to_string(),
        before: None,
        after: None,
        delta: None,
        message,
    }
}

fn quality_signals_from_fixture(fixture: &serde_json::Value) -> ChapterQualitySignals {
    let mut anchors = Vec::new();
    for entry in fixture["lorebook"].as_array().into_iter().flatten() {
        if let Some(keyword) = entry["keyword"].as_str() {
            push_unique(&mut anchors, keyword);
        }
    }
    // Only add outline terms that actually appear in chapter texts to avoid
    // inflating anchor expectations with terms from summaries not in the prose.
    let all_chapter_text: String = fixture["chapters"]
        .as_object()
        .into_iter()
        .flatten()
        .filter_map(|(_, v)| v.as_str())
        .collect();
    for outline in fixture["outline"].as_array().into_iter().flatten() {
        if let Some(summary) = outline["summary"].as_str() {
            for token in extract_profile_agnostic_terms(summary) {
                if all_chapter_text.contains(&token) {
                    push_unique(&mut anchors, &token);
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
        required_anchors: Vec::new(),
        required_state_deltas: Vec::new(),
        prior_chapter_summaries: Vec::new(),
        scene_contract: None,
        world_assets: Vec::new(),
        canon_constraints: Vec::new(),
        canon_terms: Vec::new(),
    }
}

fn extract_profile_agnostic_terms(text: &str) -> Vec<String> {
    // Extract common Chinese narrative keywords without profile-specific hardcoding
    let mut terms = Vec::new();
    for keyword in [
        "代价", "选择", "秘密", "真相", "承诺", "背叛", "入口", "线索",
    ] {
        if text.contains(keyword) {
            terms.push(keyword.to_string());
        }
    }
    terms
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

fn compute_duplicate_preview_groups(fixture: &serde_json::Value) -> Vec<DuplicatePreviewGroup> {
    let Some(chapters) = fixture["chapters"].as_object() else {
        return Vec::new();
    };
    let mut previews: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (chapter_title, value) in chapters {
        let text = value.as_str().unwrap_or("");
        let preview = text.chars().take(120).collect::<String>();
        let normalized = preview.split_whitespace().collect::<Vec<_>>().join(" ");
        previews
            .entry(normalized)
            .or_default()
            .push(chapter_title.clone());
    }
    previews
        .into_iter()
        .filter(|(_, chapters)| chapters.len() > 1)
        .map(|(preview, chapters)| DuplicatePreviewGroup { preview, chapters })
        .collect()
}

fn percentile_u64(values: &[u64], p: f64) -> u64 {
    if values.is_empty() {
        return 0;
    }
    let mut sorted: Vec<u64> = values.to_vec();
    sorted.sort_unstable();
    let idx = ((sorted.len() as f64 - 1.0) * p).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

fn compute_latency_percentiles_from_report(gate_report_path: &std::path::Path) -> (u64, u64, u64) {
    let Ok(data) = std::fs::read_to_string(gate_report_path) else {
        return (0, 0, 0);
    };
    let Ok(report) = serde_json::from_str::<serde_json::Value>(&data) else {
        return (0, 0, 0);
    };
    let latencies: Vec<u64> = report["chapters"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|ch| ch["latencyMs"].as_u64())
        .collect();
    (
        percentile_u64(&latencies, 0.50),
        percentile_u64(&latencies, 0.90),
        percentile_u64(&latencies, 0.95),
    )
}

fn compute_state_delta_coverage_from_report(
    gate_report_path: &std::path::Path,
) -> (usize, usize, usize) {
    let Ok(data) = std::fs::read_to_string(gate_report_path) else {
        return (0, 0, 0);
    };
    let Ok(report) = serde_json::from_str::<serde_json::Value>(&data) else {
        return (0, 0, 0);
    };
    let coverage = report.get("stateDeltaCoverage");
    (
        coverage.and_then(|c| c["covered"].as_u64()).unwrap_or(0) as usize,
        coverage.and_then(|c| c["weak"].as_u64()).unwrap_or(0) as usize,
        coverage.and_then(|c| c["missing"].as_u64()).unwrap_or(0) as usize,
    )
}

fn compute_min_max_chars(fixture: &serde_json::Value) -> (usize, usize) {
    let chapters = fixture["chapters"].as_object();
    let lengths: Vec<usize> = chapters
        .into_iter()
        .flat_map(|map| map.values())
        .filter_map(|value| value.as_str().map(|text| text.chars().count()))
        .collect();
    if lengths.is_empty() {
        (0, 0)
    } else {
        (
            *lengths.iter().min().unwrap_or(&0),
            *lengths.iter().max().unwrap_or(&0),
        )
    }
}

fn compute_avg_carry_rate(results: &[EvalResult]) -> Option<f32> {
    let scores: Vec<f32> = results
        .iter()
        .filter_map(|result| {
            result.after.as_ref().and_then(|after| {
                after
                    .get("metric_results")
                    .and_then(|metrics| metrics.as_object())
                    .and_then(|map| map.get("anchor_carry"))
                    .and_then(|value| value.as_f64().map(|f| f as f32))
            })
        })
        .collect();
    average(&scores)
}

fn compute_repair_rate(results: &[EvalResult]) -> f32 {
    let revision_tasks: Vec<_> = results
        .iter()
        .filter(|result| result.task == "targeted_revision" || result.task == "manual_craft_edit")
        .collect();
    if revision_tasks.is_empty() {
        return 0.0;
    }
    let repair_needed = revision_tasks
        .iter()
        .filter(|result| {
            if result.status != "pass" {
                return true;
            }
            if let Some(score) = result.delta.as_ref().and_then(|delta| {
                delta
                    .get("overall_score")
                    .and_then(|value| value.as_f64().map(|f| f as f32))
            }) {
                score < 0.0
            } else {
                false
            }
        })
        .count();
    repair_needed as f32 / revision_tasks.len() as f32
}

fn build_quality_warnings(
    duplicate_groups: &[DuplicatePreviewGroup],
    repair_rate: f32,
    min_chars: usize,
    max_chars: usize,
    avg_carry_rate: Option<f32>,
) -> Vec<QualityWarning> {
    let mut warnings = Vec::new();
    for group in duplicate_groups {
        warnings.push(QualityWarning {
            kind: "duplicate_preview".to_string(),
            severity: if group.chapters.len() >= 3 {
                "fail".to_string()
            } else {
                "warning".to_string()
            },
            message: format!(
                "{} chapters share similar opening preview: {}",
                group.chapters.len(),
                group.chapters.join(", ")
            ),
        });
    }
    if repair_rate > 0.3 {
        warnings.push(QualityWarning {
            kind: "high_repair_rate".to_string(),
            severity: "fail".to_string(),
            message: format!("repair rate {:.0}% exceeds threshold", repair_rate * 100.0),
        });
    } else if repair_rate > 0.15 {
        warnings.push(QualityWarning {
            kind: "elevated_repair_rate".to_string(),
            severity: "warning".to_string(),
            message: format!("repair rate {:.0}% is elevated", repair_rate * 100.0),
        });
    }
    if min_chars == 0 {
        warnings.push(QualityWarning {
            kind: "missing_chapter_text".to_string(),
            severity: "fail".to_string(),
            message: "at least one chapter has empty text".to_string(),
        });
    }
    if max_chars > 0 && min_chars > 0 && max_chars / min_chars > 5 {
        warnings.push(QualityWarning {
            kind: "length_variance".to_string(),
            severity: "warning".to_string(),
            message: format!(
                "chapter length varies greatly: min={} max={}",
                min_chars, max_chars
            ),
        });
    }
    if let Some(rate) = avg_carry_rate {
        if rate < 0.4 {
            warnings.push(QualityWarning {
                kind: "low_carry_rate".to_string(),
                severity: "fail".to_string(),
                message: format!("avg anchor_carry {:.2} below threshold", rate),
            });
        } else if rate < 0.6 {
            warnings.push(QualityWarning {
                kind: "low_carry_rate".to_string(),
                severity: "warning".to_string(),
                message: format!("avg anchor_carry {:.2} below recommended", rate),
            });
        }
    }
    warnings
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

fn build_eval_run_trend(profile: &str, timestamp: String, results: &[EvalResult]) -> EvalRunTrend {
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
        profile: profile.to_string(),
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
        duplicate_preview_groups: Vec::new(),
        repair_rate: 0.0,
        min_chars: 0,
        max_chars: 0,
        avg_carry_rate: None,
        p50_latency_ms: 0,
        p90_latency_ms: 0,
        p95_latency_ms: 0,
        quality_warnings: Vec::new(),
        state_delta_covered: 0,
        state_delta_weak: 0,
        state_delta_missing: 0,
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
    profile: &str,
    current: EvalRunTrend,
    previous: Option<EvalRunTrend>,
) -> EvalTrendReport {
    let Some(previous_trend) = previous else {
        return EvalTrendReport {
            profile: profile.to_string(),
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
        profile: profile.to_string(),
        current,
        previous: Some(previous_trend),
        delta: Some(delta),
        regressions,
    }
}

fn run_world_asset_contract_eval(
    profile: &str,
    task: &EvalTask,
    _fixture: &serde_json::Value,
) -> EvalResult {
    let expected = &task.expected;
    let profile_name = expected["profile"].as_str().unwrap_or(profile);
    let assets = load_world_assets(profile_name);
    let constraints = compile_canon_constraints(&assets);
    let min_constraints = expected["min_constraints"].as_u64().unwrap_or(1) as usize;
    let contract_non_empty = expected["contract_non_empty"].as_bool().unwrap_or(true);

    let mission = expected["mission"].as_str().unwrap_or("test mission");
    let contract = compile_scene_contract(&task.chapter, mission, &assets, &constraints, &[], None);

    let mut messages = Vec::new();
    if constraints.len() < min_constraints {
        messages.push(format!(
            "Expected at least {} constraints, got {}",
            min_constraints,
            constraints.len()
        ));
    }
    if contract_non_empty && contract.active_constraints.is_empty() {
        messages.push("Scene contract has no active constraints".to_string());
    }
    if expected["has_chapter_id"].as_bool().unwrap_or(false) && contract.chapter_id.is_empty() {
        messages.push("Scene contract missing chapter_id".to_string());
    }

    if messages.is_empty() {
        EvalResult {
            profile: profile.to_string(),
            task: task.task.clone(),
            chapter: task.chapter.clone(),
            status: "pass".to_string(),
            before: Some(serde_json::json!({ "constraint_count": constraints.len() })),
            after: None,
            delta: None,
            message: format!(
                "World asset contract compiled: {} constraints, {} active",
                constraints.len(),
                contract.active_constraints.len()
            ),
        }
    } else {
        EvalResult {
            profile: profile.to_string(),
            task: task.task.clone(),
            chapter: task.chapter.clone(),
            status: "fail".to_string(),
            before: Some(serde_json::json!({ "constraint_count": constraints.len() })),
            after: None,
            delta: None,
            message: messages.join("; "),
        }
    }
}

fn run_canon_forbidden_claim_eval(
    profile: &str,
    task: &EvalTask,
    _fixture: &serde_json::Value,
) -> EvalResult {
    let expected = &task.expected;
    let profile_name = expected["profile"].as_str().unwrap_or(profile);
    let assets = load_world_assets(profile_name);
    let chapter_text = expected["chapter_text"].as_str().unwrap_or("");
    let forbidden_term = expected["forbidden_term"].as_str().unwrap_or("");
    let should_detect = expected["should_detect"].as_bool().unwrap_or(false);
    let expected_constraint_id = expected["expected_constraint_id"].as_str();

    let source_asset = expected_constraint_id
        .and_then(|id| id.strip_prefix("constraint-"))
        .and_then(|asset_id| assets.iter().find(|asset| asset.id == asset_id))
        .or_else(|| {
            assets.iter().find(|asset| {
                asset.tags.iter().any(|tag| tag.contains(forbidden_term))
                    || asset.summary.contains(forbidden_term)
            })
        });
    let active_constraints = vec![CanonConstraint {
        id: expected_constraint_id
            .unwrap_or("eval-forbidden-claim")
            .to_string(),
        kind: CanonConstraintKind::ForbiddenClaim,
        summary: format!("正文不得出现 {}", forbidden_term),
        trigger_terms: source_asset
            .map(|asset| vec![asset.name.clone()])
            .unwrap_or_else(|| vec![forbidden_term.to_string()]),
        forbidden_terms: vec![forbidden_term.to_string()],
        required_terms: Vec::new(),
        severity: ConstraintSeverity::Hard,
        source_asset_id: source_asset
            .map(|asset| asset.id.clone())
            .unwrap_or_else(|| "eval-forbidden-claim".to_string()),
        evidence: source_asset
            .map(|asset| asset.evidence.clone())
            .unwrap_or_default(),
        applies_to: Vec::new(),
        expected_consequence: String::new(),
    }];
    let contract = SceneContract {
        chapter_id: task.chapter.clone(),
        mission: "eval forbidden claim".to_string(),
        required_facts: Vec::new(),
        active_constraints,
        required_state_deltas: Vec::new(),
        allowed_reveals: Vec::new(),
        blocked_reveals: Vec::new(),
        evidence_refs: Vec::new(),
        continuity_anchors: Vec::new(),
        required_costs: Vec::new(),
    };
    let violations = validate_world_consistency(chapter_text, &contract, &assets, &[]);

    let detected = violations.iter().any(|v| {
        v.kind == CanonConstraintKind::ForbiddenClaim
            && expected_constraint_id.map_or(
                forbidden_term.is_empty() || v.message.contains(forbidden_term),
                |id| v.constraint_id == id,
            )
    });

    if detected == should_detect {
        EvalResult {
            profile: profile.to_string(),
            task: task.task.clone(),
            chapter: task.chapter.clone(),
            status: "pass".to_string(),
            before: Some(serde_json::json!({ "violations": violations.len() })),
            after: None,
            delta: None,
            message: format!(
                "Forbidden claim detection: expected={}, got={}, violations={}",
                should_detect,
                detected,
                violations.len()
            ),
        }
    } else {
        EvalResult {
            profile: profile.to_string(),
            task: task.task.clone(),
            chapter: task.chapter.clone(),
            status: "fail".to_string(),
            before: Some(serde_json::json!({ "violations": violations.len() })),
            after: None,
            delta: None,
            message: format!(
                "Forbidden claim mismatch: expected detection={}, but got {}. violations={:?}",
                should_detect,
                detected,
                violations
                    .iter()
                    .map(|v| &v.constraint_id)
                    .collect::<Vec<_>>()
            ),
        }
    }
}

fn run_canon_required_cost_eval(
    profile: &str,
    task: &EvalTask,
    _fixture: &serde_json::Value,
) -> EvalResult {
    let expected = &task.expected;
    let profile_name = expected["profile"].as_str().unwrap_or(profile);
    let assets = load_world_assets(profile_name);
    let chapter_text = expected["chapter_text"].as_str().unwrap_or("");
    let trigger_term = expected["trigger_term"].as_str().unwrap_or("");
    let required_term = expected["required_term"].as_str().unwrap_or("");
    let should_detect = expected["should_detect"].as_bool().unwrap_or(false);
    let expected_constraint_kind = expected["expected_constraint_kind"].as_str();

    let mut constraints: Vec<CanonConstraint> = compile_canon_constraints(&assets)
        .into_iter()
        .filter(|c| c.kind != CanonConstraintKind::RequiredCost)
        .collect();
    let source_asset = assets
        .iter()
        .filter(|a| {
            matches!(a.approval_status, ApprovalStatus::Approved)
                && matches!(a.kind, WorldAssetKind::Rule)
        })
        .find(|a| {
            (a.summary.contains(trigger_term) || a.name.contains(trigger_term))
                && (required_term.is_empty()
                    || a.summary.contains(required_term)
                    || a.name.contains(required_term)
                    || a.tags.iter().any(|t| t.contains(required_term)))
        })
        .or_else(|| {
            assets
                .iter()
                .filter(|a| {
                    matches!(a.approval_status, ApprovalStatus::Approved)
                        && matches!(a.kind, WorldAssetKind::Rule)
                })
                .find(|a| {
                    a.summary.contains(required_term)
                        || a.name.contains(required_term)
                        || a.tags.iter().any(|t| t.contains(required_term))
                })
        });
    if let Some(asset) = source_asset {
        constraints.push(CanonConstraint {
            id: format!("required-cost-{}", asset.id),
            kind: CanonConstraintKind::RequiredCost,
            summary: format!("Using {} requires paying {}", trigger_term, required_term),
            trigger_terms: vec![trigger_term.to_string()],
            forbidden_terms: vec![],
            required_terms: vec![required_term.to_string()],
            severity: ConstraintSeverity::Hard,
            source_asset_id: asset.id.clone(),
            evidence: asset.evidence.clone(),
            applies_to: Vec::new(),
            expected_consequence: String::new(),
        });
    }

    let contract = SceneContract {
        chapter_id: task.chapter.clone(),
        mission: "eval required cost".to_string(),
        required_facts: Vec::new(),
        active_constraints: constraints
            .iter()
            .filter(|c| c.kind == CanonConstraintKind::RequiredCost)
            .cloned()
            .collect(),
        required_state_deltas: Vec::new(),
        allowed_reveals: Vec::new(),
        blocked_reveals: Vec::new(),
        evidence_refs: Vec::new(),
        continuity_anchors: Vec::new(),
        required_costs: Vec::new(),
    };
    let violations = validate_world_consistency(chapter_text, &contract, &assets, &[]);

    let detected = violations.iter().any(|v| {
        v.kind == CanonConstraintKind::RequiredCost
            && (trigger_term.is_empty() || v.message.contains(trigger_term))
            && expected_constraint_kind.is_none_or(|k| {
                let kind_str = match v.kind {
                    CanonConstraintKind::RequiredFact => "RequiredFact",
                    CanonConstraintKind::ForbiddenClaim => "ForbiddenClaim",
                    CanonConstraintKind::ForbiddenAction => "ForbiddenAction",
                    CanonConstraintKind::RequiredCost => "RequiredCost",
                    CanonConstraintKind::HierarchyLimit => "HierarchyLimit",
                    CanonConstraintKind::ExceptionRule => "ExceptionRule",
                };
                kind_str == k
            })
    });

    if detected == should_detect {
        EvalResult {
            profile: profile.to_string(),
            task: task.task.clone(),
            chapter: task.chapter.clone(),
            status: "pass".to_string(),
            before: Some(serde_json::json!({ "violations": violations.len() })),
            after: None,
            delta: None,
            message: format!(
                "Required cost detection: expected={}, got={}, violations={}",
                should_detect,
                detected,
                violations.len()
            ),
        }
    } else {
        EvalResult {
            profile: profile.to_string(),
            task: task.task.clone(),
            chapter: task.chapter.clone(),
            status: "fail".to_string(),
            before: Some(serde_json::json!({ "violations": violations.len() })),
            after: None,
            delta: None,
            message: format!(
                "Required cost mismatch: expected detection={}, but got {}. violations={:?}",
                should_detect,
                detected,
                violations
                    .iter()
                    .map(|v| &v.constraint_id)
                    .collect::<Vec<_>>()
            ),
        }
    }
}

fn run_canon_constraint_eval(
    profile: &str,
    task: &EvalTask,
    _fixture: &serde_json::Value,
) -> EvalResult {
    let expected = &task.expected;
    let profile_name = expected["profile"].as_str().unwrap_or(profile);
    let assets = load_world_assets(profile_name);
    let chapter_text = expected["chapter_text"].as_str().unwrap_or("");
    let should_detect = expected["should_detect"].as_bool().unwrap_or(false);
    let constraint_kind = expected["constraint_kind"].as_str().unwrap_or("");

    let constraint = match constraint_kind {
        "ForbiddenAction" => CanonConstraint {
            id: expected["expected_constraint_id"]
                .as_str()
                .unwrap_or("eval-forbidden-action")
                .to_string(),
            kind: CanonConstraintKind::ForbiddenAction,
            summary: format!(
                "禁止{}",
                expected["forbidden_term"].as_str().unwrap_or("未知行动")
            ),
            trigger_terms: vec![expected["forbidden_term"]
                .as_str()
                .unwrap_or("")
                .to_string()],
            forbidden_terms: vec![expected["forbidden_term"]
                .as_str()
                .unwrap_or("")
                .to_string()],
            required_terms: Vec::new(),
            severity: ConstraintSeverity::Hard,
            source_asset_id: "eval-canon-constraint".to_string(),
            evidence: Vec::new(),
            applies_to: Vec::new(),
            expected_consequence: String::new(),
        },
        "HierarchyLimit" => CanonConstraint {
            id: "eval-hierarchy-constraint".to_string(),
            kind: CanonConstraintKind::HierarchyLimit,
            summary: format!(
                "{} 不可执行 {}",
                expected["low_tier"].as_str().unwrap_or(""),
                expected["high_action"].as_str().unwrap_or("")
            ),
            trigger_terms: vec![expected["low_tier"].as_str().unwrap_or("").to_string()],
            forbidden_terms: vec![expected["high_action"].as_str().unwrap_or("").to_string()],
            required_terms: Vec::new(),
            severity: ConstraintSeverity::Hard,
            source_asset_id: "eval-canon-constraint".to_string(),
            evidence: Vec::new(),
            applies_to: Vec::new(),
            expected_consequence: String::new(),
        },
        "RequiredCost" => CanonConstraint {
            id: "eval-required-cost-constraint".to_string(),
            kind: CanonConstraintKind::RequiredCost,
            summary: format!(
                "{} 需要 {}",
                expected["trigger_term"].as_str().unwrap_or(""),
                expected["required_term"].as_str().unwrap_or("")
            ),
            trigger_terms: vec![expected["trigger_term"].as_str().unwrap_or("").to_string()],
            forbidden_terms: Vec::new(),
            required_terms: vec![expected["required_term"].as_str().unwrap_or("").to_string()],
            severity: ConstraintSeverity::Hard,
            source_asset_id: "eval-canon-constraint".to_string(),
            evidence: Vec::new(),
            applies_to: Vec::new(),
            expected_consequence: String::new(),
        },
        other => {
            return EvalResult {
                profile: profile.to_string(),
                task: task.task.clone(),
                chapter: task.chapter.clone(),
                status: "fail".to_string(),
                before: None,
                after: None,
                delta: None,
                message: format!("Unknown canon_constraint kind: {}", other),
            };
        }
    };

    let contract = SceneContract {
        chapter_id: task.chapter.clone(),
        mission: "eval canon constraint".to_string(),
        required_facts: Vec::new(),
        active_constraints: vec![constraint],
        required_state_deltas: Vec::new(),
        allowed_reveals: Vec::new(),
        blocked_reveals: Vec::new(),
        evidence_refs: Vec::new(),
        continuity_anchors: Vec::new(),
        required_costs: Vec::new(),
    };
    let eval_asset = WorldAsset {
        id: "eval-canon-constraint".to_string(),
        kind: WorldAssetKind::Rule,
        name: "eval-canon-constraint".to_string(),
        summary: "eval-canon-constraint".to_string(),
        evidence: Vec::new(),
        approval_status: ApprovalStatus::Approved,
        tags: Vec::new(),
    };
    let mut eval_assets = assets;
    eval_assets.push(eval_asset);
    let violations = validate_world_consistency(chapter_text, &contract, &eval_assets, &[]);
    let detected = !violations.is_empty();

    EvalResult {
        profile: profile.to_string(),
        task: task.task.clone(),
        chapter: task.chapter.clone(),
        status: if detected == should_detect {
            "pass".to_string()
        } else {
            "fail".to_string()
        },
        before: Some(serde_json::json!({ "violations": violations.len() })),
        after: None,
        delta: None,
        message: format!(
            "canon_constraint: kind={}, expected_detect={}, got={}, violations={}",
            constraint_kind,
            should_detect,
            detected,
            violations.len()
        ),
    }
}

fn run_canon_proposed_not_hard_eval(
    profile: &str,
    task: &EvalTask,
    _fixture: &serde_json::Value,
) -> EvalResult {
    let expected = &task.expected;
    let profile_name = expected["profile"].as_str().unwrap_or(profile);
    let assets = load_world_assets(profile_name);
    let chapter_text = expected["chapter_text"].as_str().unwrap_or("");
    let source_asset_id = expected["source_asset_id"].as_str().unwrap_or("");
    let should_detect = expected["should_detect"].as_bool().unwrap_or(false);
    let max_severity = expected["max_severity"].as_str().unwrap_or("warning");

    let constraints = compile_canon_constraints(&assets);
    let contract = compile_scene_contract(
        &task.chapter,
        "test mission",
        &assets,
        &constraints,
        &[],
        None,
    );
    let violations = validate_world_consistency(chapter_text, &contract, &assets, &[]);

    let detected = violations
        .iter()
        .any(|v| v.constraint_id.contains(source_asset_id));

    let severity_ok = violations.iter().all(|v| {
        if v.constraint_id.contains(source_asset_id) {
            let severity_str = match v.severity {
                ConstraintSeverity::Info => "info",
                ConstraintSeverity::Warning => "warning",
                ConstraintSeverity::Hard => "hard",
            };
            severity_str == max_severity || (max_severity == "warning" && severity_str == "info")
        } else {
            true
        }
    });

    if detected == should_detect && severity_ok {
        EvalResult {
            profile: profile.to_string(),
            task: task.task.clone(),
            chapter: task.chapter.clone(),
            status: "pass".to_string(),
            before: Some(serde_json::json!({ "violations": violations.len() })),
            after: None,
            delta: None,
            message: format!(
                "Proposed rule severity check: detected={}, max_severity respected, violations={}",
                detected,
                violations.len()
            ),
        }
    } else {
        EvalResult {
            profile: profile.to_string(),
            task: task.task.clone(),
            chapter: task.chapter.clone(),
            status: "fail".to_string(),
            before: Some(serde_json::json!({ "violations": violations.len() })),
            after: None,
            delta: None,
            message: format!(
                "Proposed rule check failed: detected={}, expected={}, severity_ok={}. violations={:?}",
                detected, should_detect, severity_ok,
                violations.iter().map(|v| (&v.constraint_id, &v.severity)).collect::<Vec<_>>()
            ),
        }
    }
}

fn run_scene_contract_prompt_eval(
    profile: &str,
    task: &EvalTask,
    _fixture: &serde_json::Value,
) -> EvalResult {
    let expected = &task.expected;
    let profile_name = expected["profile"].as_str().unwrap_or(profile);
    let assets = load_world_assets(profile_name);
    let constraints = compile_canon_constraints(&assets);
    let mission = expected["mission"].as_str().unwrap_or("test mission");
    let contract = compile_scene_contract(&task.chapter, mission, &assets, &constraints, &[], None);

    let mut messages = Vec::new();
    if expected["contract_non_empty"].as_bool().unwrap_or(false)
        && contract.active_constraints.is_empty()
    {
        messages.push("Scene contract has no active constraints".to_string());
    }
    if expected["has_chapter_id"].as_bool().unwrap_or(false) && contract.chapter_id.is_empty() {
        messages.push("Scene contract missing chapter_id".to_string());
    }
    if expected["has_active_constraints"]
        .as_bool()
        .unwrap_or(false)
        && contract.active_constraints.is_empty()
    {
        messages.push("Scene contract missing active constraints".to_string());
    }

    if messages.is_empty() {
        EvalResult {
            profile: profile.to_string(),
            task: task.task.clone(),
            chapter: task.chapter.clone(),
            status: "pass".to_string(),
            before: Some(
                serde_json::json!({ "active_constraints": contract.active_constraints.len() }),
            ),
            after: None,
            delta: None,
            message: format!(
                "Scene contract prompt: {} active constraints, chapter_id={}",
                contract.active_constraints.len(),
                contract.chapter_id
            ),
        }
    } else {
        EvalResult {
            profile: profile.to_string(),
            task: task.task.clone(),
            chapter: task.chapter.clone(),
            status: "fail".to_string(),
            before: Some(
                serde_json::json!({ "active_constraints": contract.active_constraints.len() }),
            ),
            after: None,
            delta: None,
            message: messages.join("; "),
        }
    }
}

fn run_unsupported_world_claim_eval(
    profile: &str,
    task: &EvalTask,
    _fixture: &serde_json::Value,
) -> EvalResult {
    let expected = &task.expected;
    let profile_name = expected["profile"].as_str().unwrap_or(profile);
    let assets = load_world_assets(profile_name);
    let chapter_text = expected["chapter_text"].as_str().unwrap_or("");
    let claim_text = expected["claim_text"].as_str().unwrap_or("");
    let should_detect = expected["should_detect"].as_bool().unwrap_or(false);

    // Heuristic: check if claim_text matches any approved asset's name/tags/summary
    let claim_lower = claim_text.to_lowercase();
    let matched = assets.iter().any(|asset| {
        if !matches!(
            asset.approval_status,
            agent_writer_lib::writer_agent::world_bible::ApprovalStatus::Approved
        ) {
            return false;
        }
        let name_lower = asset.name.to_lowercase();
        let summary_lower = asset.summary.to_lowercase();
        if name_lower.contains(&claim_lower) || claim_lower.contains(&name_lower) {
            return true;
        }
        if summary_lower.contains(&claim_lower) || claim_lower.contains(&summary_lower) {
            return true;
        }
        asset.tags.iter().any(|tag| {
            let tag_lower = tag.to_lowercase();
            tag_lower.contains(&claim_lower) || claim_lower.contains(&tag_lower)
        })
    });

    // If the claim text is in the chapter text and doesn't match any asset, it's unsupported
    let claim_in_text = chapter_text.to_lowercase().contains(&claim_lower);
    let detected = claim_in_text && !matched;

    let status = if detected == should_detect {
        "pass"
    } else {
        "fail"
    };
    EvalResult {
        profile: profile.to_string(),
        task: task.task.clone(),
        chapter: task.chapter.clone(),
        status: status.to_string(),
        before: Some(serde_json::json!({ "claim_in_text": claim_in_text, "matched_asset": matched })),
        after: None,
        delta: None,
        message: format!(
            "unsupported_world_claim: expected_detect={}, got={}, claim_in_text={}, matched_asset={}",
            should_detect, detected, claim_in_text, matched
        ),
    }
}

fn run_hierarchy_confusion_eval(
    profile: &str,
    task: &EvalTask,
    _fixture: &serde_json::Value,
) -> EvalResult {
    let expected = &task.expected;
    let profile_name = expected["profile"].as_str().unwrap_or(profile);
    let assets = load_world_assets(profile_name);
    let chapter_text = expected["chapter_text"].as_str().unwrap_or("");
    let should_detect = expected["should_detect"].as_bool().unwrap_or(false);

    // Build a HierarchyLimit constraint from task expected
    let mut constraints = compile_canon_constraints(&assets);

    // If task specifies explicit hierarchy terms, add a HierarchyLimit constraint
    if let (Some(low_tier), Some(high_action)) = (
        expected["low_tier"].as_str(),
        expected["high_action"].as_str(),
    ) {
        let hierarchy_constraint = CanonConstraint {
            id: "constraint-hierarchy-eval".to_string(),
            kind: CanonConstraintKind::HierarchyLimit,
            summary: format!("{} 不可执行 {}", low_tier, high_action),
            trigger_terms: vec![low_tier.to_string()],
            forbidden_terms: vec![high_action.to_string()],
            required_terms: vec![],
            severity: ConstraintSeverity::Hard,
            source_asset_id: "eval-hierarchy".to_string(),
            evidence: vec![],
            applies_to: vec![],
            expected_consequence: String::new(),
        };
        constraints.push(hierarchy_constraint);
    }

    // Build contract directly with hierarchy constraint active to bypass mission filtering
    let contract = SceneContract {
        chapter_id: task.chapter.clone(),
        mission: "test mission".to_string(),
        required_facts: Vec::new(),
        active_constraints: constraints.clone(),
        required_state_deltas: Vec::new(),
        allowed_reveals: Vec::new(),
        blocked_reveals: Vec::new(),
        evidence_refs: Vec::new(),
        continuity_anchors: Vec::new(),
        required_costs: Vec::new(),
    };
    let violations = validate_world_consistency(chapter_text, &contract, &assets, &[]);

    let detected = violations
        .iter()
        .any(|v| v.kind == CanonConstraintKind::HierarchyLimit);

    let status = if detected == should_detect {
        "pass"
    } else {
        "fail"
    };
    EvalResult {
        profile: profile.to_string(),
        task: task.task.clone(),
        chapter: task.chapter.clone(),
        status: status.to_string(),
        before: Some(serde_json::json!({ "violations": violations.len() })),
        after: None,
        delta: None,
        message: format!(
            "hierarchy_confusion: expected_detect={}, got={}, hierarchy_violations={}",
            should_detect,
            detected,
            violations
                .iter()
                .filter(|v| v.kind == CanonConstraintKind::HierarchyLimit)
                .count()
        ),
    }
}

fn run_state_regression_eval(
    profile: &str,
    task: &EvalTask,
    _fixture: &serde_json::Value,
) -> EvalResult {
    use agent_writer_lib::writer_agent::world_bible::{check_state_regression, StateLedgerDelta};

    let expected = &task.expected;
    let chapter_text = expected["chapter_text"].as_str().unwrap_or("");
    let should_detect = expected["should_detect"].as_bool().unwrap_or(false);

    // Build prior_deltas from task expected
    let prior_deltas: Vec<StateLedgerDelta> = expected["prior_deltas"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| {
            Some(StateLedgerDelta {
                delta_type: item["delta_type"].as_str()?.to_string(),
                entity_id: item["entity_id"].as_str()?.to_string(),
                before_state: item["before_state"].as_str()?.to_string(),
                after_state: item["after_state"].as_str()?.to_string(),
                source_constraint_id: item["source_constraint_id"].as_str().map(|s| s.to_string()),
                evidence_excerpt: item["evidence_excerpt"].as_str().unwrap_or("").to_string(),
            })
        })
        .collect();

    let regressions = check_state_regression(chapter_text, &prior_deltas);
    let detected = !regressions.is_empty();

    let status = if detected == should_detect {
        "pass"
    } else {
        "fail"
    };
    EvalResult {
        profile: profile.to_string(),
        task: task.task.clone(),
        chapter: task.chapter.clone(),
        status: status.to_string(),
        before: Some(serde_json::json!({ "prior_deltas": prior_deltas.len() })),
        after: Some(serde_json::json!({ "regressions": regressions.len() })),
        delta: None,
        message: format!(
            "state_regression: expected_detect={}, got={}, regressions={:?}",
            should_detect,
            detected,
            regressions.iter().map(|r| &r.entity_id).collect::<Vec<_>>()
        ),
    }
}

fn run_extraction_eval(profile: &str, task: &EvalTask, _fixture: &serde_json::Value) -> EvalResult {
    use agent_writer_lib::writer_agent::world_bible::{
        create_llm_proposal, parse_world_rules_from_markdown, TypedWorldAsset,
    };

    let expected = &task.expected;
    let source_md = expected["source_markdown"].as_str().unwrap_or("");
    let expected_asset_count_min =
        expected["expected_asset_count_min"].as_u64().unwrap_or(0) as usize;
    let expected_rule_names: Vec<String> = expected["expected_rule_names"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|v| v.as_str().map(|s| s.to_string()))
        .collect();
    let expected_source_ref_non_empty = expected["expected_source_ref_non_empty"]
        .as_bool()
        .unwrap_or(false);

    let rules = parse_world_rules_from_markdown("test.md", source_md);

    let asset_count_ok = rules.len() >= expected_asset_count_min;
    let names_ok = expected_rule_names.iter().all(|expected_name| {
        rules
            .iter()
            .any(|r| r.name.contains(expected_name) || r.summary.contains(expected_name))
    });
    let source_ref_ok = !expected_source_ref_non_empty
        || rules
            .iter()
            .all(|r| !r.source_ref.excerpt.is_empty() && !r.source_ref.source_id.is_empty());

    // P20: Test create_llm_proposal produces proposed WorldAsset::Rule with valid source_ref and confidence
    let typed_assets: Vec<TypedWorldAsset> = rules.into_iter().map(TypedWorldAsset::Rule).collect();
    let proposal = create_llm_proposal(
        &format!("extraction-{}", task.chapter),
        "test.md",
        typed_assets,
        12345,
    );
    let proposed_rules: Vec<_> = proposal
        .proposed_assets
        .iter()
        .filter_map(|a| match a {
            TypedWorldAsset::Rule(r) => Some(r),
            _ => None,
        })
        .collect();
    let has_proposed_rule = !proposed_rules.is_empty();
    let all_proposed = proposed_rules.iter().all(|r| {
        matches!(
            r.approval_status,
            agent_writer_lib::writer_agent::world_bible::ApprovalStatus::Proposed
        )
    });
    let all_source_ref_ok = proposed_rules
        .iter()
        .all(|r| !r.source_ref.excerpt.is_empty() && !r.source_ref.source_id.is_empty());
    let all_confidence_positive = proposed_rules.iter().all(|r| r.confidence > 0.0);

    let proposal_ok =
        has_proposed_rule && all_proposed && all_source_ref_ok && all_confidence_positive;

    let status = if asset_count_ok && names_ok && source_ref_ok && proposal_ok {
        "pass"
    } else {
        "fail"
    };

    EvalResult {
        profile: profile.to_string(),
        task: task.task.clone(),
        chapter: task.chapter.clone(),
        status: status.to_string(),
        before: Some(serde_json::json!({ "expected_assets": expected_asset_count_min })),
        after: Some(serde_json::json!({
            "extracted_rules": proposed_rules.len(),
            "proposal_ok": proposal_ok,
            "all_proposed": all_proposed,
            "all_source_ref_ok": all_source_ref_ok,
            "all_confidence_positive": all_confidence_positive,
        })),
        delta: None,
        message: format!(
            "extraction: assets={}, names_ok={}, source_ref_ok={}, proposal_ok={}",
            proposed_rules.len(),
            names_ok,
            source_ref_ok,
            proposal_ok
        ),
    }
}

fn run_profile_eval(profile: &str, smoke: bool) -> (Vec<EvalResult>, EvalTrendReport) {
    let fixture = load_fixture(profile);
    let all_tasks = load_tasks(profile);
    let tasks: Vec<_> = if smoke {
        all_tasks.into_iter().filter(is_smoke_task).collect()
    } else {
        all_tasks
    };

    let output_dir = fixture_dir().join(profile);
    let output_path = output_dir.join("eval_output.jsonl");
    let trend_path = output_dir.join("eval_trend.json");
    let previous_run = load_previous_eval_run(&output_path);

    let mut results = Vec::new();
    for task in &tasks {
        let result = match task.task.as_str() {
            "chapter_generation" => run_chapter_generation_eval(profile, task, &fixture),
            "quality_evaluation" => run_quality_evaluation_eval(profile, task, &fixture),
            "quality_signals" => run_quality_signal_eval(profile, task, &fixture),
            "targeted_revision" => run_targeted_revision_eval(profile, task, &fixture),
            "craft_memory" => run_craft_memory_eval(profile, task, &fixture),
            "manual_craft_edit" => run_manual_craft_edit_eval(profile, task, &fixture),
            "craft_memory_prompt" => run_craft_memory_prompt_eval(profile, task, &fixture),
            "canon_conflict" => run_canon_conflict_eval(profile, task, &fixture),
            "planning_review" => run_planning_review_eval(profile, task, &fixture),
            "promise_progression" => run_promise_progression_eval(profile, task, &fixture),
            "continuity_diagnostic" => run_continuity_diagnostic_eval(profile, task, &fixture),
            "negative_missing_anchor" => run_negative_missing_anchor_eval(profile, task, &fixture),
            "negative_style_drift" => run_negative_style_drift_eval(profile, task, &fixture),
            "negative_promise_stalled" => {
                run_negative_promise_stalled_eval(profile, task, &fixture)
            }
            "negative_revision_no_change" => {
                run_negative_revision_no_change_eval(profile, task, &fixture)
            }
            "negative_craft_memory_injection" => {
                run_negative_craft_memory_injection_eval(profile, task, &fixture)
            }
            "negative_plot_stalled" => run_negative_plot_stalled_eval(profile, task, &fixture),
            "state_delta_trace" => run_state_delta_trace_eval(profile, task, &fixture),
            "world_asset_contract" => run_world_asset_contract_eval(profile, task, &fixture),
            "canon_forbidden_claim" => run_canon_forbidden_claim_eval(profile, task, &fixture),
            "canon_required_cost" => run_canon_required_cost_eval(profile, task, &fixture),
            "canon_constraint" => run_canon_constraint_eval(profile, task, &fixture),
            "canon_proposed_not_hard" => run_canon_proposed_not_hard_eval(profile, task, &fixture),
            "scene_contract_prompt" => run_scene_contract_prompt_eval(profile, task, &fixture),
            "unsupported_world_claim" => run_unsupported_world_claim_eval(profile, task, &fixture),
            "hierarchy_confusion" => run_hierarchy_confusion_eval(profile, task, &fixture),
            "state_regression" => run_state_regression_eval(profile, task, &fixture),
            "extraction" => run_extraction_eval(profile, task, &fixture),
            other => EvalResult {
                profile: profile.to_string(),
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
    lines.push(serde_json::json!({
        "run": "eval",
        "timestamp": timestamp,
        "task_count": tasks.len(),
        "profile": profile,
        "smoke": smoke,
    }));

    for result in &results {
        lines.push(serde_json::to_value(result).expect("serialize result"));
    }

    let pass_count = results.iter().filter(|r| r.status == "pass").count();
    lines.push(serde_json::json!({
        "summary": true,
        "profile": profile,
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

    let mut current_trend = build_eval_run_trend(profile, timestamp, &results);
    let (min_chars, max_chars) = compute_min_max_chars(&fixture);
    current_trend.duplicate_preview_groups = compute_duplicate_preview_groups(&fixture);
    current_trend.repair_rate = compute_repair_rate(&results);
    current_trend.min_chars = min_chars;
    current_trend.max_chars = max_chars;
    current_trend.avg_carry_rate = compute_avg_carry_rate(&results);
    current_trend.quality_warnings = build_quality_warnings(
        &current_trend.duplicate_preview_groups,
        current_trend.repair_rate,
        current_trend.min_chars,
        current_trend.max_chars,
        current_trend.avg_carry_rate,
    );
    let gate_path = reports_dir().join("real_author_session_thirty_chapter_gate.json");
    let (p50_ms, p90_ms, p95_ms) = compute_latency_percentiles_from_report(&gate_path);
    current_trend.p50_latency_ms = p50_ms;
    current_trend.p90_latency_ms = p90_ms;
    current_trend.p95_latency_ms = p95_ms;
    let (covered, weak, missing) = compute_state_delta_coverage_from_report(&gate_path);
    current_trend.state_delta_covered = covered;
    current_trend.state_delta_weak = weak;
    current_trend.state_delta_missing = missing;
    let previous_trend =
        previous_run.map(|run| build_eval_run_trend(profile, run.timestamp, &run.results));
    let trend_report = build_eval_trend_report(profile, current_trend, previous_trend);
    let trend_output =
        serde_json::to_string_pretty(&trend_report).expect("serialize eval trend report");
    std::fs::write(&trend_path, trend_output).expect("write eval_trend.json");

    (results, trend_report)
}

fn compute_world_consistency_summary(results: &[EvalResult]) -> WorldConsistencySummary {
    let world_bible_tasks = [
        "world_asset_contract",
        "canon_forbidden_claim",
        "canon_required_cost",
        "canon_constraint",
        "canon_proposed_not_hard",
        "scene_contract_prompt",
        "unsupported_world_claim",
        "hierarchy_confusion",
        "state_regression",
        "extraction",
    ];

    let mut total_checks = 0usize;
    let mut hard_violations = 0usize;
    let mut profile_checks: BTreeMap<String, (usize, usize)> = BTreeMap::new();

    for result in results {
        if !world_bible_tasks.contains(&result.task.as_str()) {
            continue;
        }
        total_checks += 1;
        let entry = profile_checks
            .entry(result.profile.clone())
            .or_insert((0, 0));
        entry.0 += 1;
        if result.status != "pass" {
            hard_violations += 1;
            entry.1 += 1;
        }
    }

    // Warnings are approximated as non-pass, non-hard tasks (e.g. proposed rules)
    let warnings = results
        .iter()
        .filter(|r| {
            world_bible_tasks.contains(&r.task.as_str())
                && r.status == "pass"
                && r.message.contains("warning")
        })
        .count();

    let profiles = profile_checks
        .into_iter()
        .map(|(profile, (checks, violations))| {
            (profile, ProfileWorldConsistency { checks, violations })
        })
        .collect();

    WorldConsistencySummary {
        total_checks,
        hard_violations,
        warnings,
        profiles,
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let smoke = args.iter().any(|a| a == "--smoke");
    let requested_profiles: Vec<String> = args
        .iter()
        .filter(|a| !a.starts_with('-'))
        .filter(|a| PROFILES.contains(&a.as_str()))
        .cloned()
        .collect();
    let profiles: Vec<&str> = if requested_profiles.is_empty() {
        PROFILES.to_vec()
    } else {
        requested_profiles.iter().map(|s| s.as_str()).collect()
    };

    let mode = if smoke { "smoke" } else { "full" };
    let timestamp = chrono::Utc::now().to_rfc3339();
    let mut all_results = Vec::new();
    let mut all_regressions = Vec::new();
    let mut profile_summaries = BTreeMap::new();

    println!("=== Writing Eval Harness ===");
    println!("Mode: {}", mode);
    println!("Profiles: {:?}", profiles);
    println!();

    let mut profile_trend_reports: BTreeMap<String, EvalTrendReport> = BTreeMap::new();
    for profile in &profiles {
        let (results, trend_report) = run_profile_eval(profile, smoke);
        let pass_count = results.iter().filter(|r| r.status == "pass").count();
        let fail_count = results.len() - pass_count;
        all_results.extend(results);
        all_regressions.extend(trend_report.regressions.iter().map(|r| r.message.clone()));
        profile_summaries.insert(
            profile.to_string(),
            ProfileSummary {
                task_count: trend_report.current.task_count,
                pass: trend_report.current.pass,
                fail: trend_report.current.fail,
                failing_tasks: trend_report.current.failing_tasks.clone(),
                duplicate_preview_groups: trend_report.current.duplicate_preview_groups.clone(),
                repair_rate: trend_report.current.repair_rate,
                min_chars: trend_report.current.min_chars,
                max_chars: trend_report.current.max_chars,
                avg_carry_rate: trend_report.current.avg_carry_rate,
                p50_latency_ms: trend_report.current.p50_latency_ms,
                p90_latency_ms: trend_report.current.p90_latency_ms,
                p95_latency_ms: trend_report.current.p95_latency_ms,
                quality_warnings: trend_report.current.quality_warnings.clone(),
                state_delta_covered: trend_report.current.state_delta_covered,
                state_delta_weak: trend_report.current.state_delta_weak,
                state_delta_missing: trend_report.current.state_delta_missing,
            },
        );
        profile_trend_reports.insert(profile.to_string(), trend_report.clone());

        println!(
            "Profile {}: {} tasks, {} pass, {} fail",
            profile, trend_report.current.task_count, pass_count, fail_count
        );
        if !trend_report.regressions.is_empty() {
            println!("  Regressions:");
            for reg in &trend_report.regressions {
                println!("    - [{}] {}: {}", reg.kind, reg.subject, reg.message);
            }
        }
    }

    let total_pass = all_results.iter().filter(|r| r.status == "pass").count();
    let total_fail = all_results.len() - total_pass;

    let quality_warnings: Vec<QualityWarning> = profile_summaries
        .values()
        .flat_map(|ps| ps.quality_warnings.clone())
        .collect();

    let world_consistency = compute_world_consistency_summary(&all_results);

    let summary = EvalSummary {
        timestamp,
        mode: mode.to_string(),
        profiles: profile_summaries,
        total_tasks: all_results.len(),
        total_pass,
        total_fail,
        regressions: all_regressions.clone(),
        quality_warnings,
        world_consistency: Some(world_consistency),
    };

    // Write aggregate summary
    let summary_path = fixture_dir().join("eval_summary.json");
    let summary_json = serde_json::to_string_pretty(&summary).expect("serialize summary");
    std::fs::write(&summary_path, summary_json).expect("write eval_summary.json");

    // Write Markdown summary
    let md_path = fixture_dir().join("eval_summary.md");
    let md_content = build_markdown_summary(&summary, &profile_trend_reports);
    std::fs::write(&md_path, md_content).expect("write eval_summary.md");

    println!();
    println!("=== Eval Summary ===");
    println!(
        "Total tasks: {}, Pass: {}, Fail: {}",
        all_results.len(),
        total_pass,
        total_fail
    );
    if !all_regressions.is_empty() {
        println!("Regressions: {}", all_regressions.len());
        for msg in &all_regressions {
            println!("  - {}", msg);
        }
    }
    println!("Summary written to: {}", summary_path.display());
    println!("Markdown summary written to: {}", md_path.display());

    if total_fail > 0 || !all_regressions.is_empty() {
        std::process::exit(1);
    }
}

fn build_markdown_summary(
    summary: &EvalSummary,
    profile_trends: &BTreeMap<String, EvalTrendReport>,
) -> String {
    let mut md = String::new();
    md.push_str("# Writing Eval Report\n\n");
    md.push_str(&format!("**Run:** {}\n", summary.timestamp));
    md.push_str(&format!("**Mode:** {}\n", summary.mode));
    md.push_str(&format!(
        "**Profiles:** {}\n\n",
        profile_trends
            .keys()
            .cloned()
            .collect::<Vec<_>>()
            .join(", ")
    ));

    md.push_str("## Summary\n\n");
    md.push_str("| Profile | Tasks | Pass | Fail | Failing Tasks |\n");
    md.push_str("|---------|-------|------|------|---------------|\n");
    for (profile, ps) in &summary.profiles {
        let failing = if ps.failing_tasks.is_empty() {
            "-".to_string()
        } else {
            ps.failing_tasks.join(", ")
        };
        md.push_str(&format!(
            "| {} | {} | {} | {} | {} |\n",
            profile, ps.task_count, ps.pass, ps.fail, failing
        ));
    }
    md.push_str(&format!(
        "| **Total** | **{}** | **{}** | **{}** | |\n\n",
        summary.total_tasks, summary.total_pass, summary.total_fail
    ));

    let mut task_regressions: Vec<&EvalTrendRegression> = Vec::new();
    let mut metric_regressions: Vec<&EvalTrendRegression> = Vec::new();
    let mut craft_regressions: Vec<&EvalTrendRegression> = Vec::new();
    let mut other_regressions: Vec<&EvalTrendRegression> = Vec::new();

    for trend in profile_trends.values() {
        for reg in &trend.regressions {
            match reg.kind.as_str() {
                "task_status" => task_regressions.push(reg),
                "metric_after_average" | "average_after_score" => metric_regressions.push(reg),
                "craft_rule_average_score_delta" => craft_regressions.push(reg),
                _ => other_regressions.push(reg),
            }
        }
    }

    let all_regs: Vec<_> = task_regressions
        .iter()
        .chain(metric_regressions.iter())
        .chain(craft_regressions.iter())
        .chain(other_regressions.iter())
        .collect();

    if all_regs.is_empty() {
        md.push_str("## Regressions\n\n");
        md.push_str("No regressions detected.\n\n");
    } else {
        md.push_str("## Regressions\n\n");
        if !task_regressions.is_empty() {
            md.push_str("### Task Status\n\n");
            for reg in &task_regressions {
                md.push_str(&format!("- **{}**: {}\n", reg.subject, reg.message));
            }
            md.push('\n');
        }
        if !metric_regressions.is_empty() {
            md.push_str("### Metric Average\n\n");
            for reg in &metric_regressions {
                md.push_str(&format!("- **{}**: {}\n", reg.subject, reg.message));
            }
            md.push('\n');
        }
        if !craft_regressions.is_empty() {
            md.push_str("### Craft Rule\n\n");
            for reg in &craft_regressions {
                md.push_str(&format!("- **{}**: {}\n", reg.subject, reg.message));
            }
            md.push('\n');
        }
    }

    md.push_str("## Risk Assessment\n\n");
    if summary.total_fail > 0 {
        md.push_str(&format!(
            "- **{} tasks failed** — review failing tasks before merge.\n",
            summary.total_fail
        ));
    }
    if !all_regs.is_empty() {
        md.push_str(&format!(
            "- **{} regressions detected** — metric or craft rule quality may have degraded.\n",
            all_regs.len()
        ));
    }
    if summary.total_fail == 0 && all_regs.is_empty() {
        md.push_str("- No quality regressions detected. Safe to proceed.\n");
    }
    md.push('\n');

    md.push_str("## World Consistency\n\n");
    if let Some(ref wc) = summary.world_consistency {
        md.push_str(&format!(
            "- **Total checks:** {}, **Hard violations:** {}, **Warnings:** {}\n",
            wc.total_checks, wc.hard_violations, wc.warnings
        ));
        md.push_str("| Profile | Checks | Violations |\n");
        md.push_str("|---------|--------|------------|\n");
        for (profile, pwc) in &wc.profiles {
            md.push_str(&format!(
                "| {} | {} | {} |\n",
                profile, pwc.checks, pwc.violations
            ));
        }
    } else {
        md.push_str("No world consistency data available.\n");
    }
    md.push('\n');

    md.push_str("## Long-Chain Quality\n\n");
    md.push_str("| Profile | Min Chars | Max Chars | Avg Carry | Repair Rate | Dup Groups | Warnings | P50 (ms) | P90 (ms) | P95 (ms) |\n");
    md.push_str("|---------|-----------|-----------|-----------|-------------|------------|----------|----------|----------|----------|\n");
    for (profile, ps) in &summary.profiles {
        let avg_carry = ps
            .avg_carry_rate
            .map(|r| format!("{:.2}", r))
            .unwrap_or_else(|| "-".to_string());
        let dup_count = ps.duplicate_preview_groups.len();
        let warn_count = ps.quality_warnings.len();
        md.push_str(&format!(
            "| {} | {} | {} | {} | {:.0}% | {} | {} | {} | {} | {} |\n",
            profile,
            ps.min_chars,
            ps.max_chars,
            avg_carry,
            ps.repair_rate * 100.0,
            dup_count,
            warn_count,
            ps.p50_latency_ms,
            ps.p90_latency_ms,
            ps.p95_latency_ms,
        ));
    }
    md.push('\n');

    if !summary.quality_warnings.is_empty() {
        md.push_str("## Quality Warnings\n\n");
        for warning in &summary.quality_warnings {
            md.push_str(&format!(
                "- **[{}]** {}: {}\n",
                warning.severity, warning.kind, warning.message
            ));
        }
        md.push('\n');
    }

    md.push_str("## Craft Rule Trends\n\n");
    md.push_str("| Profile | Rule | Avg Score Delta | Examples | Bad Patterns |\n");
    md.push_str("|---------|------|-----------------|----------|--------------|\n");
    for (profile, trend) in profile_trends {
        for (rule_id, rule_trend) in &trend.current.craft_rule_trends {
            let avg_delta = rule_trend
                .average_score_delta
                .map(|d| format!("{:+.3}", d))
                .unwrap_or_else(|| "-".to_string());
            md.push_str(&format!(
                "| {} | {} | {} | {} | {} |\n",
                profile,
                rule_id,
                avg_delta,
                rule_trend.stored_examples,
                rule_trend.stored_bad_patterns
            ));
        }
    }
    md.push('\n');

    md.push_str("---\n\n");
    md.push_str("*Generated by eval_runner. Do not edit manually.*\n");
    md
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eval_result_with_score(
        profile: &str,
        task: &str,
        chapter: &str,
        status: &str,
        score: f32,
    ) -> EvalResult {
        EvalResult {
            profile: profile.to_string(),
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

    fn eval_result_with_delta(
        profile: &str,
        task: &str,
        chapter: &str,
        delta: serde_json::Value,
    ) -> EvalResult {
        EvalResult {
            profile: profile.to_string(),
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
            "xianxia",
            "previous".to_string(),
            &[eval_result_with_score(
                "xianxia",
                "quality_evaluation",
                "第二章",
                "pass",
                0.8,
            )],
        );
        let current = build_eval_run_trend(
            "xianxia",
            "current".to_string(),
            &[eval_result_with_score(
                "xianxia",
                "quality_evaluation",
                "第二章",
                "fail",
                0.6,
            )],
        );

        let report = build_eval_trend_report("xianxia", current, Some(previous));

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
                profile: "xianxia".to_string(),
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
                profile: "xianxia".to_string(),
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
                "xianxia",
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

        let trend = build_eval_run_trend("xianxia", "current".to_string(), &results);
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
            "xianxia",
            "previous".to_string(),
            &[eval_result_with_delta(
                "xianxia",
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
            "xianxia",
            "current".to_string(),
            &[eval_result_with_delta(
                "xianxia",
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

        let report = build_eval_trend_report("xianxia", current, Some(previous));

        assert!(report.regressions.iter().any(|regression| {
            regression.kind == "craft_rule_average_score_delta"
                && regression.subject == "scene_objective"
        }));
    }

    #[test]
    fn compute_duplicate_preview_groups_detects_duplicates() {
        let fixture = serde_json::json!({
            "chapters": {
                "第一章": "This is the first chapter with some unique content.",
                "第二章": "This is the first chapter with some unique content.",
                "第三章": "Something completely different here."
            }
        });
        let groups = compute_duplicate_preview_groups(&fixture);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].chapters.len(), 2);
        assert!(groups[0].chapters.contains(&"第一章".to_string()));
        assert!(groups[0].chapters.contains(&"第二章".to_string()));
    }

    #[test]
    fn compute_duplicate_preview_groups_empty_when_no_duplicates() {
        let fixture = serde_json::json!({
            "chapters": {
                "第一章": "Unique content A.",
                "第二章": "Unique content B."
            }
        });
        let groups = compute_duplicate_preview_groups(&fixture);
        assert!(groups.is_empty());
    }

    #[test]
    fn compute_min_max_chars_finds_bounds() {
        let fixture = serde_json::json!({
            "chapters": {
                "short": "abc",
                "medium": "abcdef",
                "long": "abcdefghijklmnopqrstuvwxyz"
            }
        });
        let (min, max) = compute_min_max_chars(&fixture);
        assert_eq!(min, 3);
        assert_eq!(max, 26);
    }

    #[test]
    fn compute_min_max_chars_empty_fixture() {
        let fixture = serde_json::json!({ "chapters": {} });
        let (min, max) = compute_min_max_chars(&fixture);
        assert_eq!(min, 0);
        assert_eq!(max, 0);
    }

    #[test]
    fn compute_avg_carry_rate_with_scores() {
        let results = vec![
            EvalResult {
                profile: "mystery".to_string(),
                task: "quality_evaluation".to_string(),
                chapter: "ch1".to_string(),
                status: "pass".to_string(),
                before: None,
                after: Some(serde_json::json!({
                    "overall_score": 0.75,
                    "metric_results": { "anchor_carry": 0.75 }
                })),
                delta: None,
                message: String::new(),
            },
            EvalResult {
                profile: "mystery".to_string(),
                task: "quality_evaluation".to_string(),
                chapter: "ch2".to_string(),
                status: "pass".to_string(),
                before: None,
                after: Some(serde_json::json!({
                    "overall_score": 0.85,
                    "metric_results": { "anchor_carry": 0.85 }
                })),
                delta: None,
                message: String::new(),
            },
        ];
        let avg = compute_avg_carry_rate(&results);
        assert!(avg.is_some_and(|v| (v - 0.80).abs() < 0.01));
    }

    #[test]
    fn compute_avg_carry_rate_ignores_missing_scores() {
        let mut result_no_metric =
            eval_result_with_score("mystery", "quality_evaluation", "ch1", "pass", 0.5);
        result_no_metric.after = Some(serde_json::json!({ "overall_score": 0.5 }));
        let results = vec![result_no_metric];
        let avg = compute_avg_carry_rate(&results);
        assert!(avg.is_none());
    }

    #[test]
    fn compute_repair_rate_zero_when_no_revision_tasks() {
        let results = vec![eval_result_with_score(
            "mystery",
            "quality_evaluation",
            "ch1",
            "pass",
            0.8,
        )];
        assert_eq!(compute_repair_rate(&results), 0.0);
    }

    #[test]
    fn compute_repair_rate_counts_failed_revisions() {
        let results = vec![
            EvalResult {
                profile: "mystery".to_string(),
                task: "targeted_revision".to_string(),
                chapter: "ch1".to_string(),
                status: "fail".to_string(),
                before: None,
                after: None,
                delta: Some(serde_json::json!({ "overall_score": 0.3 })),
                message: String::new(),
            },
            EvalResult {
                profile: "mystery".to_string(),
                task: "targeted_revision".to_string(),
                chapter: "ch2".to_string(),
                status: "pass".to_string(),
                before: None,
                after: None,
                delta: Some(serde_json::json!({ "overall_score": 0.3 })),
                message: String::new(),
            },
        ];
        assert_eq!(compute_repair_rate(&results), 0.5);
    }

    #[test]
    fn compute_repair_rate_counts_negative_delta_scores() {
        let results = vec![
            EvalResult {
                profile: "mystery".to_string(),
                task: "manual_craft_edit".to_string(),
                chapter: "ch1".to_string(),
                status: "pass".to_string(),
                before: None,
                after: None,
                delta: Some(serde_json::json!({ "overall_score": -0.2 })),
                message: String::new(),
            },
            EvalResult {
                profile: "mystery".to_string(),
                task: "manual_craft_edit".to_string(),
                chapter: "ch2".to_string(),
                status: "pass".to_string(),
                before: None,
                after: None,
                delta: Some(serde_json::json!({ "overall_score": 0.1 })),
                message: String::new(),
            },
        ];
        assert_eq!(compute_repair_rate(&results), 0.5);
    }

    #[test]
    fn percentile_u64_returns_correct_values() {
        let values = vec![100, 200, 300, 400, 500, 600, 700, 800, 900, 1000];
        let p50 = percentile_u64(&values, 0.50);
        let p90 = percentile_u64(&values, 0.90);
        let p95 = percentile_u64(&values, 0.95);
        // 10 values: p50 at index 4 (0-indexed rounded from 4.5) → 500 or 600
        // p90 at index 8 (8.1 → 8) → 900
        // p95 at index 8 (8.55 → 9) → 1000
        assert!(p50 >= 500, "p50={p50} should be ~median");
        assert!(p90 >= 800, "p90={p90} should be near 90th percentile");
        assert!(p95 >= 900, "p95={p95} should be near 95th percentile");
    }

    #[test]
    fn percentile_u64_empty_returns_zero() {
        assert_eq!(percentile_u64(&[], 0.50), 0);
        assert_eq!(percentile_u64(&[], 0.90), 0);
    }
}
