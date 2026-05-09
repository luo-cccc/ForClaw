use agent_writer_lib::chapter_generation::compile_empowerment_prompt;
use agent_writer_lib::chapter_generation::evaluate_chapter_quality;
use agent_writer_lib::chapter_generation::evaluate_chapter_quality_with_signals;
use agent_writer_lib::chapter_generation::ChapterQualitySignals;
use agent_writer_lib::chapter_generation::SceneCraftPlan;
use agent_writer_lib::writer_agent::author_voice::{
    AuthorVoiceSnapshot, VoiceDiction, VoiceRhythm,
};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct EvalTask {
    task: String,
    chapter: String,
    instruction: Option<String>,
    check: Option<String>,
    metrics: Option<Vec<String>>,
    expected: serde_json::Value,
}

fn load_fixture() -> serde_json::Value {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../fixtures/writing_eval/project.json"
    );
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn load_tasks() -> Vec<EvalTask> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../fixtures/writing_eval/eval_tasks.jsonl"
    );
    std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

#[test]
fn eval_fixture_chapter_exists() {
    let fixture = load_fixture();
    let chapter1 = fixture["chapters"]["第一章"].as_str().unwrap();
    assert!(!chapter1.is_empty());
    assert!(chapter1.contains("林墨"));
    assert!(chapter1.contains("古剑"));
}

#[test]
fn eval_fixture_lorebook_has_required_entities() {
    let fixture = load_fixture();
    let lorebook = fixture["lorebook"].as_array().unwrap();
    assert!(lorebook.len() >= 5);
    let keywords: Vec<&str> = lorebook
        .iter()
        .filter_map(|e| e["keyword"].as_str())
        .collect();
    assert!(keywords.contains(&"古剑"));
    assert!(keywords.contains(&"寒影剑"));
    assert!(keywords.contains(&"林墨"));
    assert!(keywords.contains(&"师门"));
    assert!(keywords.contains(&"青云宗"));
}

#[test]
fn eval_fixture_outline_has_two_chapters() {
    let fixture = load_fixture();
    let outline = fixture["outline"].as_array().unwrap();
    assert!(outline.len() >= 3);
    assert_eq!(outline[0]["chapterTitle"], "第一章");
    assert_eq!(outline[1]["chapterTitle"], "第二章");
    assert_eq!(outline[2]["chapterTitle"], "第三章");
}

#[test]
fn eval_chapter_generation_task() {
    let tasks = load_tasks();
    let gen_task = tasks
        .iter()
        .find(|t| t.task == "chapter_generation")
        .unwrap();
    assert_eq!(gen_task.chapter, "第一章");
    assert!(gen_task.instruction.is_some());
    let expected = &gen_task.expected;
    assert!(expected["min_chars"].as_u64().unwrap() >= 1000);
}

#[test]
fn eval_continuity_diagnostic_task() {
    let tasks = load_tasks();
    let diag_task = tasks
        .iter()
        .find(|t| t.task == "continuity_diagnostic")
        .unwrap();
    assert_eq!(diag_task.chapter, "第一章");
    assert!(diag_task.check.as_ref().unwrap().contains("古剑"));
    assert!(!diag_task.expected["canon_conflict"].as_bool().unwrap());
}

#[test]
fn eval_quality_evaluation_task() {
    let tasks = load_tasks();
    let qual_task = tasks
        .iter()
        .find(|t| t.task == "quality_evaluation")
        .unwrap();
    let metrics = qual_task.metrics.as_ref().unwrap();
    assert!(metrics.contains(&"dialogue_function".to_string()));
    assert!(metrics.contains(&"ending_hook".to_string()));

    // Run actual quality evaluation on fixture chapter
    let fixture = load_fixture();
    let chapter_text = fixture["chapters"]["第一章"].as_str().unwrap();
    let plan = SceneCraftPlan::default();
    let report = evaluate_chapter_quality(chapter_text, "第一章", &plan, &[], 500, 2000);

    // Verify expected quality thresholds
    let expected = &qual_task.expected;
    let min_score = expected["overall_score_min"].as_f64().unwrap() as f32;
    assert!(
        report.overall_score >= min_score,
        "Chapter quality {:.2} below expected minimum {:.2}",
        report.overall_score,
        min_score
    );

    // All 8 metrics present
    assert_eq!(report.metric_results.len(), 8);

    // Verify requested metrics have scores
    for metric_name in metrics {
        let result = report
            .metric_results
            .iter()
            .find(|m| &m.metric == metric_name)
            .unwrap_or_else(|| panic!("Metric {} not found in quality report", metric_name));
        assert!(result.score > 0.0, "Metric {} has zero score", metric_name);
    }
}

#[test]
fn eval_fixture_has_expanded_task_coverage() {
    let tasks = load_tasks();
    assert!(
        tasks.len() >= 8,
        "eval fixture should cover more than the initial 3 shallow tasks"
    );
    assert!(tasks
        .iter()
        .any(|task| task.task == "quality_evaluation" && task.chapter == "第二章"));
    assert!(tasks
        .iter()
        .any(|task| task.task == "chapter_generation" && task.chapter == "第二章"));
    assert!(tasks
        .iter()
        .any(|task| task.task == "quality_signals" && task.chapter == "第二章"));
    assert!(tasks
        .iter()
        .any(|task| task.task == "targeted_revision" && task.chapter == "第二章"));
    assert!(tasks
        .iter()
        .any(|task| task.task == "craft_memory" && task.chapter == "第二章"));
}

#[test]
fn eval_second_chapter_quality_has_metric_thresholds() {
    let tasks = load_tasks();
    let qual_task = tasks
        .iter()
        .find(|t| t.task == "quality_evaluation" && t.chapter == "第二章")
        .unwrap();
    let metric_min = qual_task.expected["metric_min"].as_object().unwrap();
    assert!(metric_min.contains_key("dialogue_function"));
    assert!(metric_min.contains_key("ending_hook"));

    let fixture = load_fixture();
    let chapter_text = fixture["chapters"]["第二章"].as_str().unwrap();
    let plan = SceneCraftPlan::default();
    let report =
        evaluate_chapter_quality(chapter_text, "第二章", &plan, &["寒影剑".into()], 500, 2000);
    for (metric, min) in metric_min {
        let result = report
            .metric_results
            .iter()
            .find(|result| result.metric == *metric)
            .unwrap_or_else(|| panic!("Metric {} not found", metric));
        assert!(
            result.score >= min.as_f64().unwrap() as f32,
            "{} score {:.2} below fixture minimum {}",
            metric,
            result.score,
            min
        );
    }
}

#[test]
fn eval_chapter_entity_consistency() {
    let fixture = load_fixture();
    let chapter = fixture["chapters"]["第一章"].as_str().unwrap();
    let lorebook = fixture["lorebook"].as_array().unwrap();

    // Every first-chapter expected lore entity keyword should appear in the chapter.
    for entry in lorebook {
        let keyword = entry["keyword"].as_str().unwrap();
        if !["古剑", "寒影剑", "林墨", "师门", "青云宗"].contains(&keyword) {
            continue;
        }
        let acceptable_alias_hit =
            keyword == "寒影剑" && chapter.contains("寒") && chapter.contains("影");
        assert!(
            chapter.contains(keyword) || acceptable_alias_hit,
            "Chapter should reference lore entity: {}",
            keyword
        );
    }
}

#[test]
fn eval_chapter_has_dialogue_and_action() {
    let fixture = load_fixture();
    let chapter = fixture["chapters"]["第一章"].as_str().unwrap();

    // Chapter should contain dialogue markers
    // (Chinese narration often uses '...' single quotes for embedded speech)
    let has_dialogue = chapter.contains('说')
        || chapter.contains('"')
        || chapter.contains('\u{201c}')
        || chapter.contains('\u{2018}')
        || chapter.contains('\'');
    assert!(has_dialogue, "Chapter should have dialogue");

    // Chapter should contain action verbs
    let action_verbs = ["推", "走", "看", "拿", "碰"];
    let has_action = action_verbs.iter().any(|v| chapter.contains(v));
    assert!(has_action, "Chapter should have action verbs");
}

#[test]
fn eval_craft_prompt_for_fixture_objective() {
    let fixture = load_fixture();
    let outline = fixture["outline"].as_array().unwrap();
    let summary = outline[0]["summary"].as_str().unwrap();

    let packet =
        compile_empowerment_prompt(summary, "关键选择", 1, false, Some(5), Some(600), None);

    assert!(
        !packet.craft_rules.is_empty(),
        "Should select craft rules for this objective"
    );
    assert!(
        !packet.chapter_discipline.is_empty(),
        "Should have discipline"
    );
}

#[test]
fn eval_quality_signals_use_real_anchor_and_voice_metrics() {
    let fixture = load_fixture();
    let chapter_text = fixture["chapters"]["第二章"].as_str().unwrap();
    let plan = SceneCraftPlan::default();
    let signals = fixture_quality_signals();
    let report = evaluate_chapter_quality_with_signals(
        chapter_text,
        "第二章",
        &plan,
        &[],
        500,
        2000,
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
    assert!(
        !anchor.reason.contains("证据不足"),
        "anchor_carry should use fixture anchors, got {}",
        anchor.reason
    );
    assert!(
        !style.reason.contains("证据不足"),
        "style_drift should use fixture voice snapshot, got {}",
        style.reason
    );
    assert!(
        anchor.score >= 0.35,
        "anchor_carry too low: {:.2}",
        anchor.score
    );
    assert!(
        style.score >= 0.55,
        "style_drift too low: {:.2}",
        style.score
    );
}

fn fixture_quality_signals() -> ChapterQualitySignals {
    ChapterQualitySignals {
        anchor_keywords: ["古剑", "寒影剑", "林墨", "青云宗", "代价", "选择"]
            .into_iter()
            .map(str::to_string)
            .collect(),
        author_voice: Some(AuthorVoiceSnapshot {
            voice_id: "test-fixture-voice".to_string(),
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
