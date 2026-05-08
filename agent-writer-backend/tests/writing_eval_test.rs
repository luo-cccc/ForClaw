use agent_writer_lib::chapter_generation::compile_empowerment_prompt;
use agent_writer_lib::chapter_generation::evaluate_chapter_quality;
use agent_writer_lib::chapter_generation::SceneCraftPlan;

#[test]
fn eval_fixture_chapter_quality() {
    // Load the fixture project chapter
    let fixture_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../fixtures/writing_eval/project.json"
    );
    let fixture: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(fixture_path).unwrap()).unwrap();

    let chapter_text = fixture["chapters"]["第一章"].as_str().unwrap();

    let plan = SceneCraftPlan::default();
    let report = evaluate_chapter_quality(chapter_text, "第一章", &plan, &[], 500, 2000);

    // Verify quality report is structured
    assert_eq!(report.metric_results.len(), 8);
    assert!(report.overall_score > 0.0);
    assert!(report.overall_score <= 1.0);

    // Chapter has dialogue -- dialogue_function should be evaluated
    let dialogue = report
        .metric_results
        .iter()
        .find(|m| m.metric == "dialogue_function")
        .unwrap();
    assert!(
        dialogue.score > 0.0,
        "dialogue_function should be scored for text with dialogue"
    );
}

#[test]
fn eval_fixture_prompt_compilation() {
    let objective = "主角发现古剑，面临选择：保密还是报告师门";

    let packet = compile_empowerment_prompt(
        objective,
        "关键选择场景",
        1,
        false,
        Some(5),
        Some(600),
        None,
    );

    assert!(
        !packet.chapter_discipline.is_empty(),
        "should select at least one craft rule for this objective"
    );
}

#[test]
fn eval_fixture_lorebook_consistency() {
    let fixture_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../fixtures/writing_eval/project.json"
    );
    let fixture: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(fixture_path).unwrap()).unwrap();

    let chapter = fixture["chapters"]["第一章"].as_str().unwrap();
    let lore_entities = ["林墨", "宗门", "古剑", "师门", "寒"];

    for entity in &lore_entities {
        assert!(
            chapter.contains(entity),
            "Chapter text should contain lore entity: {}",
            entity
        );
    }
}
