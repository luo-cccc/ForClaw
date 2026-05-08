use agent_writer_lib::chapter_generation::compile_empowerment_prompt;
use agent_writer_lib::chapter_generation::evaluate_chapter_quality;
use agent_writer_lib::chapter_generation::SceneCraftPlan;
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
    assert_eq!(lorebook.len(), 3);
    let keywords: Vec<&str> = lorebook
        .iter()
        .filter_map(|e| e["keyword"].as_str())
        .collect();
    assert!(keywords.contains(&"古剑"));
    assert!(keywords.contains(&"林墨"));
    assert!(keywords.contains(&"师门"));
}

#[test]
fn eval_fixture_outline_has_two_chapters() {
    let fixture = load_fixture();
    let outline = fixture["outline"].as_array().unwrap();
    assert_eq!(outline.len(), 2);
    assert_eq!(outline[0]["chapterTitle"], "第一章");
    assert_eq!(outline[1]["chapterTitle"], "第二章");
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
fn eval_chapter_entity_consistency() {
    let fixture = load_fixture();
    let chapter = fixture["chapters"]["第一章"].as_str().unwrap();
    let lorebook = fixture["lorebook"].as_array().unwrap();

    // Every lore entity keyword should appear in the chapter
    for entry in lorebook {
        let keyword = entry["keyword"].as_str().unwrap();
        assert!(
            chapter.contains(keyword),
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
