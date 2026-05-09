use agent_writer_lib::chapter_generation::compile_empowerment_prompt;
use agent_writer_lib::chapter_generation::compile_empowerment_prompt_with_memory;
use agent_writer_lib::chapter_generation::evaluate_chapter_quality;
use agent_writer_lib::chapter_generation::evaluate_chapter_quality_with_signals;
use agent_writer_lib::chapter_generation::format_craft_prompt_section;
use agent_writer_lib::chapter_generation::ChapterQualitySignals;
use agent_writer_lib::chapter_generation::CraftMemoryPromptBadPattern;
use agent_writer_lib::chapter_generation::CraftMemoryPromptExample;
use agent_writer_lib::chapter_generation::CraftMemoryPromptSamples;
use agent_writer_lib::chapter_generation::ManualCraftEditFeedbackRequest;
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

fn fixture_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../fixtures/writing_eval"))
}

fn load_fixture(profile: &str) -> serde_json::Value {
    let path = fixture_dir().join(profile).join("project.json");
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn load_tasks(profile: &str) -> Vec<EvalTask> {
    let path = fixture_dir().join(profile).join("eval_tasks.jsonl");
    std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

fn all_tasks() -> Vec<(String, EvalTask)> {
    let mut all = Vec::new();
    for profile in ["xianxia", "mystery", "scifi"] {
        for task in load_tasks(profile) {
            all.push((profile.to_string(), task));
        }
    }
    all
}

#[test]
fn eval_xianxia_fixture_chapter_exists() {
    let fixture = load_fixture("xianxia");
    let chapter1 = fixture["chapters"]["第一章"].as_str().unwrap();
    assert!(!chapter1.is_empty());
    assert!(chapter1.contains("林墨"));
    assert!(chapter1.contains("古剑"));
}

#[test]
fn eval_xianxia_fixture_lorebook_has_required_entities() {
    let fixture = load_fixture("xianxia");
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
fn eval_xianxia_fixture_outline_has_two_chapters() {
    let fixture = load_fixture("xianxia");
    let outline = fixture["outline"].as_array().unwrap();
    assert!(outline.len() >= 4);
    assert_eq!(outline[0]["chapterTitle"], "第一章");
    assert_eq!(outline[1]["chapterTitle"], "第二章");
    assert_eq!(outline[2]["chapterTitle"], "第三章");
    assert_eq!(outline[3]["chapterTitle"], "第四章");
}

#[test]
fn eval_xianxia_chapter_generation_task() {
    let tasks = load_tasks("xianxia");
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
fn eval_xianxia_continuity_diagnostic_task() {
    let tasks = load_tasks("xianxia");
    let diag_task = tasks
        .iter()
        .find(|t| t.task == "continuity_diagnostic")
        .unwrap();
    assert_eq!(diag_task.chapter, "第一章");
    assert!(diag_task.check.as_ref().unwrap().contains("古剑"));
    assert!(!diag_task.expected["canon_conflict"].as_bool().unwrap());
}

#[test]
fn eval_xianxia_quality_evaluation_task() {
    let tasks = load_tasks("xianxia");
    let qual_task = tasks
        .iter()
        .find(|t| t.task == "quality_evaluation")
        .unwrap();
    let metrics = qual_task.metrics.as_ref().unwrap();
    assert!(metrics.contains(&"dialogue_function".to_string()));
    assert!(metrics.contains(&"ending_hook".to_string()));

    // Run actual quality evaluation on fixture chapter
    let fixture = load_fixture("xianxia");
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
fn eval_matrix_has_three_profiles() {
    for profile in ["xianxia", "mystery", "scifi"] {
        let fixture = load_fixture(profile);
        assert!(
            fixture["chapters"].as_object().map(|o| !o.is_empty()).unwrap_or(false),
            "profile {} should have chapters",
            profile
        );
        let tasks = load_tasks(profile);
        assert!(
            !tasks.is_empty(),
            "profile {} should have eval tasks",
            profile
        );
    }
}

#[test]
fn eval_matrix_has_30_plus_tasks() {
    let total = all_tasks().len();
    assert!(
        total >= 30,
        "eval matrix should have at least 30 tasks, got {}",
        total
    );
}

#[test]
fn eval_matrix_covers_required_task_types() {
    let all = all_tasks();
    let task_types: std::collections::HashSet<String> = all
        .iter()
        .map(|(_, task)| task.task.clone())
        .collect();

    let required = [
        "chapter_generation",
        "quality_evaluation",
        "quality_signals",
        "targeted_revision",
        "craft_memory",
        "manual_craft_edit",
        "craft_memory_prompt",
        "canon_conflict",
        "planning_review",
        "promise_progression",
        "continuity_diagnostic",
    ];
    for req in &required {
        assert!(
            task_types.contains(*req),
            "eval matrix missing task type: {}",
            req
        );
    }
}

#[test]
fn eval_matrix_has_negative_cases() {
    let all = all_tasks();
    let negative_types: Vec<String> = all
        .iter()
        .filter(|(_, task)| task.task.starts_with("negative_"))
        .map(|(_, task)| task.task.clone())
        .collect();

    assert!(
        negative_types.iter().any(|t| t == "negative_missing_anchor"),
        "should have negative_missing_anchor"
    );
    assert!(
        negative_types.iter().any(|t| t == "negative_style_drift"),
        "should have negative_style_drift"
    );
    assert!(
        negative_types.iter().any(|t| t == "negative_promise_stalled"),
        "should have negative_promise_stalled"
    );
    assert!(
        negative_types.iter().any(|t| t == "negative_revision_no_change"),
        "should have negative_revision_no_change"
    );
    assert!(
        negative_types
            .iter()
            .any(|t| t == "negative_craft_memory_injection"),
        "should have negative_craft_memory_injection"
    );
}

#[test]
fn eval_xianxia_has_expanded_task_coverage() {
    let tasks = load_tasks("xianxia");
    assert!(
        tasks.len() >= 13,
        "xianxia eval fixture should cover more than the initial 13 tasks"
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
    assert!(tasks
        .iter()
        .any(|task| task.task == "manual_craft_edit" && task.chapter == "第二章"));
    assert!(tasks
        .iter()
        .any(|task| task.task == "craft_memory_prompt" && task.chapter == "第二章"));
    assert!(tasks
        .iter()
        .any(|task| task.task == "canon_conflict" && task.chapter == "第二章"));
    assert!(tasks
        .iter()
        .any(|task| task.task == "planning_review" && task.chapter == "第三章"));
    assert!(tasks
        .iter()
        .any(|task| task.task == "promise_progression" && task.chapter == "第二章"));
}

#[test]
fn eval_xianxia_fixture_has_canon_and_promise_coverage() {
    let fixture = load_fixture("xianxia");
    let canon = fixture["canon"].as_array().expect("canon fixture");
    assert!(canon.iter().any(|rule| {
        rule["id"] == "cold-shadow-cost"
            && rule["forbidden"]
                .as_array()
                .is_some_and(|forbidden| !forbidden.is_empty())
    }));

    let promises = fixture["promises"].as_array().expect("promise fixture");
    assert!(promises.iter().any(|promise| {
        promise["title"] == "寒影剑代价"
            && promise["status"] == "open"
            && promise["progress_markers"]
                .as_array()
                .is_some_and(|markers| markers.iter().any(|marker| marker == "白发"))
    }));
}

#[test]
fn eval_second_chapter_quality_has_metric_thresholds() {
    let tasks = load_tasks("xianxia");
    let qual_task = tasks
        .iter()
        .find(|t| t.task == "quality_evaluation" && t.chapter == "第二章")
        .unwrap();
    let metric_min = qual_task.expected["metric_min"].as_object().unwrap();
    assert!(metric_min.contains_key("dialogue_function"));
    assert!(metric_min.contains_key("ending_hook"));

    let fixture = load_fixture("xianxia");
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
fn eval_xianxia_chapter_entity_consistency() {
    let fixture = load_fixture("xianxia");
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
fn eval_xianxia_chapter_has_dialogue_and_action() {
    let fixture = load_fixture("xianxia");
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
fn eval_xianxia_craft_prompt_for_fixture_objective() {
    let fixture = load_fixture("xianxia");
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
    let fixture = load_fixture("xianxia");
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
        required_anchors: Vec::new(),
    }
}

#[test]
fn manual_craft_edit_feedback_persists_examples_and_bad_patterns() {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    agent_writer_lib::writer_agent::memory::ensure_craft_tables(&conn).unwrap();
    let request = ManualCraftEditFeedbackRequest {
        chapter_title: "第二章".to_string(),
        before_text: "林墨说：这是古剑。散修站在门口，没有回答。".to_string(),
        after_text: "林墨握紧寒影剑，低声说：现在你必须选择。散修因此停在门口，第一次露出退意。"
            .to_string(),
        metrics: vec![
            "dialogue_function".to_string(),
            "scene_causality".to_string(),
        ],
        anchor_keywords: vec!["寒影剑".to_string(), "林墨".to_string(), "选择".to_string()],
        open_promise_keywords: vec!["寒影剑".to_string()],
        author_voice: fixture_quality_signals().author_voice,
        target_min_chars: Some(0),
        target_max_chars: Some(2000),
        source_ref: Some("test:manual_author_edit".to_string()),
        author_approved: true,
    };
    let result =
        agent_writer_lib::chapter_generation::record_manual_craft_edit_feedback(&conn, request)
            .unwrap();

    assert!(
        !result.example_refs.is_empty(),
        "manual edit should create good craft examples"
    );
    assert!(
        !result.bad_pattern_refs.is_empty(),
        "manual edit should create rejected before-pattern memory"
    );
    assert!(result.target_changes.iter().any(|change| {
        !change.changed_excerpt_before.is_empty() && !change.changed_excerpt_after.is_empty()
    }));
}

#[test]
fn craft_memory_samples_flow_into_prompt_section() {
    let packet = compile_empowerment_prompt_with_memory(
        "审讯场景，林墨必须逼问散修",
        "对话推进",
        0,
        false,
        Some(5),
        Some(2000),
        None,
        &[CraftMemoryPromptSamples {
            rule_id: "dialogue_function".to_string(),
            examples: vec![CraftMemoryPromptExample {
                rule_id: "dialogue_function".to_string(),
                excerpt_ref: "test:example".to_string(),
                excerpt: "林墨握紧寒影剑，低声说：现在你必须选择。".to_string(),
                reason: "作者认可：对话改变选择。".to_string(),
                score_delta: 0.42,
            }],
            bad_patterns: vec![CraftMemoryPromptBadPattern {
                rule_id: "dialogue_function".to_string(),
                evidence_ref: "test:bad".to_string(),
                evidence_excerpt: "林墨说了一整段古剑来历，散修没有任何反应。".to_string(),
                correction: "让台词改变权力、信息或选择。".to_string(),
                rejected_count: 2,
            }],
        }],
    );
    let section = format_craft_prompt_section(&packet);

    assert!(section.contains("项目写法记忆"));
    assert!(section.contains("必须选择"));
    assert!(section.contains("一整段古剑来历"));
}
