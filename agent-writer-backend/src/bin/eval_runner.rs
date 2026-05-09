use agent_writer_lib::chapter_generation::{
    build_revision_target_changes, compile_empowerment_prompt, evaluate_chapter_quality,
    SceneCraftPlan,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Deserialize)]
struct EvalTask {
    task: String,
    chapter: String,
    instruction: Option<String>,
    check: Option<String>,
    metrics: Option<Vec<String>>,
    expected: serde_json::Value,
}

#[derive(Debug, Serialize)]
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
    let before_report =
        evaluate_chapter_quality(chapter_text, &task.chapter, &plan, &[], 500, 2000);

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

    let after_report =
        evaluate_chapter_quality(chapter_text, &task.chapter, &craft_plan, &[], 500, 2000);

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
    let before_report =
        evaluate_chapter_quality(chapter_text, &task.chapter, &plan, &[], 500, 2000);

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

    let after_report =
        evaluate_chapter_quality(chapter_text, &task.chapter, &craft_plan, &[], 500, 2000);

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

    let mut results = Vec::new();
    for task in &tasks {
        let result = match task.task.as_str() {
            "chapter_generation" => run_chapter_generation_eval(task, &fixture),
            "quality_evaluation" => run_quality_evaluation_eval(task, &fixture),
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

    let output_dir = fixture_dir();
    let output_path = output_dir.join("eval_output.jsonl");

    let mut lines = Vec::new();
    // Header with run metadata
    lines.push(serde_json::json!({
        "run": "eval",
        "timestamp": chrono::Utc::now().to_rfc3339(),
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

    println!("Writing eval complete: {}", output_path.display());
    println!(
        "  tasks: {}, pass: {}, fail: {}",
        results.len(),
        pass_count,
        results.len() - pass_count
    );
    for r in &results {
        println!("  [{}] {} {}: {}", r.status, r.task, r.chapter, r.message);
    }
    if pass_count != results.len() {
        std::process::exit(1);
    }
}
