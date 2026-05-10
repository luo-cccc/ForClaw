use std::collections::HashMap;
use std::sync::OnceLock;

use crate::writer_agent::memory::CraftRuleStats;

// ── Library loader ──

fn craft_library() -> &'static Vec<CraftRule> {
    static LIBRARY: OnceLock<Vec<CraftRule>> = OnceLock::new();
    LIBRARY.get_or_init(|| {
        serde_json::from_str(include_str!("../../../config/craft-library.json"))
            .expect("craft-library.json must be valid")
    })
}

/// Public accessor for external stats lookup (e.g., craft_memory integration)
pub fn craft_library_for_stats() -> &'static Vec<CraftRule> {
    craft_library()
}

const DEFAULT_MAX_RULES: usize = 5;
const DEFAULT_MAX_PROMPT_CHARS: usize = 600;

// ── Scene type inference ──

#[derive(Debug, Clone, PartialEq)]
enum SceneType {
    ChapterDraft,
    DialogueScene,
    ActionScene,
    RevelationScene,
    TurningPoint,
}

fn infer_scene_type(objective: &str, target_beat: &str) -> SceneType {
    let text = format!("{} {}", objective, target_beat).to_lowercase();
    if text.contains("审讯") || text.contains("询问") || text.contains("逼问") || text.contains("追问") {
        SceneType::DialogueScene
    } else if text.contains("战斗") || text.contains("对决") || text.contains("出手") || text.contains("交战") {
        SceneType::ActionScene
    } else if text.contains("揭示") || text.contains("发现") || text.contains("真相") || text.contains("秘密") {
        SceneType::RevelationScene
    } else if text.contains("转折") || text.contains("决定") || text.contains("选择") || text.contains("背叛") {
        SceneType::TurningPoint
    } else {
        SceneType::ChapterDraft
    }
}

fn scene_type_to_applies_tag(scene_type: &SceneType) -> &'static str {
    match scene_type {
        SceneType::ChapterDraft => "chapter_draft",
        SceneType::DialogueScene => "dialogue_scene",
        SceneType::ActionScene => "chapter_draft",
        SceneType::RevelationScene => "worldbuilding_reveal",
        SceneType::TurningPoint => "turning_point",
    }
}

// ── Prompt Compiler ──

pub fn compile_empowerment_prompt(
    objective: &str,
    target_beat: &str,
    _open_promise_count: usize,
    has_near_payoff: bool,
    max_rules: Option<usize>,
    max_prompt_chars: Option<usize>,
    rule_stats: Option<&HashMap<String, CraftRuleStats>>,
) -> EmpowermentPromptPacket {
    compile_empowerment_prompt_with_memory(
        objective,
        target_beat,
        _open_promise_count,
        has_near_payoff,
        max_rules,
        max_prompt_chars,
        rule_stats,
        &[],
        None,
    )
}

pub fn compile_empowerment_prompt_with_memory(
    objective: &str,
    target_beat: &str,
    _open_promise_count: usize,
    has_near_payoff: bool,
    max_rules: Option<usize>,
    max_prompt_chars: Option<usize>,
    rule_stats: Option<&HashMap<String, CraftRuleStats>>,
    memory_samples: &[CraftMemoryPromptSamples],
    scene_contract: Option<&crate::writer_agent::world_bible::SceneContract>,
) -> EmpowermentPromptPacket {
    let max_rules = max_rules.unwrap_or(DEFAULT_MAX_RULES);
    let max_prompt_chars = max_prompt_chars.unwrap_or(DEFAULT_MAX_PROMPT_CHARS);
    let library = craft_library();
    let scene_type = infer_scene_type(objective, target_beat);
    let scene_tag = scene_type_to_applies_tag(&scene_type);

    // Collect matching rules
    let mut candidates: Vec<&CraftRule> = library
        .iter()
        .filter(|rule| rule.applies_when.iter().any(|tag| tag == scene_tag || tag == "chapter_draft"))
        .collect();

    // Force-select promise_advance if near payoff
    if has_near_payoff {
        if let Some(pos) = candidates.iter().position(|r| r.id == "promise_advance") {
            let rule = candidates.remove(pos);
            candidates.insert(0, rule);
        }
    }

    // Greedy select up to limits
    let mut selected: Vec<CraftRuleSelection> = Vec::new();
    let mut chars_used = 0usize;
    for rule in candidates.iter().take(max_rules) {
        let rule_chars = rule.instruction.chars().count() + rule.revision_hint.chars().count();
        if chars_used + rule_chars > max_prompt_chars && !selected.is_empty() {
            break;
        }
        chars_used += rule_chars;
        let base_priority = if rule.id == "promise_advance" && has_near_payoff { 10 } else { 5 };
        let adjusted_priority = if let Some(stats_map) = rule_stats {
            if let Some(stats) = stats_map.get(&rule.id) {
                let boost = (stats.acceptance_rate() - 0.5) * 5.0;
                (base_priority as f32 + boost).clamp(1.0, 10.0) as u8
            } else {
                base_priority
            }
        } else {
            base_priority
        };
        selected.push(CraftRuleSelection {
            rule_id: rule.id.clone(),
            reason: format!("当前场景类型匹配: {}", rule.name),
            evidence_refs: vec![format!("scene_type:{}", scene_tag)],
            priority: adjusted_priority,
        });
    }

    // Build output sections
    let chapter_discipline: Vec<String> = selected
        .iter()
        .filter_map(|sel| {
            library.iter().find(|r| r.id == sel.rule_id).map(|r| r.instruction.clone())
        })
        .collect();

    let must_avoid: Vec<String> = selected
        .iter()
        .filter_map(|sel| {
            library.iter().find(|r| r.id == sel.rule_id).map(|r| r.anti_patterns.join("；"))
        })
        .filter(|s| !s.is_empty())
        .collect();

    let self_checklist: Vec<String> = selected
        .iter()
        .filter_map(|sel| {
            library.iter().find(|r| r.id == sel.rule_id).map(|r| {
                r.diagnostic_signals
                    .iter()
                    .map(|sig| format!("检查: {} ({})", r.name, sig))
                    .collect::<Vec<_>>()
                    .join("; ")
            })
        })
        .filter(|s| !s.is_empty())
        .collect();

    let selected_rule_ids: Vec<&str> = selected.iter().map(|sel| sel.rule_id.as_str()).collect();
    let memory_examples = memory_samples
        .iter()
        .filter(|sample| selected_rule_ids.contains(&sample.rule_id.as_str()))
        .flat_map(|sample| sample.examples.iter().take(1).cloned())
        .take(3)
        .collect::<Vec<_>>();
    let memory_bad_patterns = memory_samples
        .iter()
        .filter(|sample| selected_rule_ids.contains(&sample.rule_id.as_str()))
        .flat_map(|sample| sample.bad_patterns.iter().take(1).cloned())
        .take(3)
        .collect::<Vec<_>>();
    chars_used += memory_examples
        .iter()
        .map(|example| example.excerpt.chars().count() + example.reason.chars().count())
        .sum::<usize>();
    chars_used += memory_bad_patterns
        .iter()
        .map(|pattern| {
            pattern.evidence_excerpt.chars().count() + pattern.correction.chars().count()
        })
        .sum::<usize>();

    let scene_contract_prompt = scene_contract
        .map(|sc| format_scene_contract_prompt(sc))
        .unwrap_or_default();

    EmpowermentPromptPacket {
        craft_rules: selected,
        chapter_discipline,
        must_avoid,
        self_checklist,
        memory_examples,
        memory_bad_patterns,
        total_token_estimate: chars_used,
        scene_contract_prompt,
    }
}

// ── SceneCraftPlan Builder ──

pub fn build_scene_craft_plan(
    chapter_title: &str,
    objective: &str,
    participants: &[String],
    target_beat: &str,
    next_chapter_summary: Option<&str>,
    open_promise_keywords: &[String],
    packet: &EmpowermentPromptPacket,
) -> SceneCraftPlan {
    let objective_text = if objective.trim().is_empty() {
        target_beat.to_string()
    } else {
        objective.to_string()
    };

    let promise_payoff: Vec<String> = open_promise_keywords
        .iter()
        .filter(|kw| objective_text.contains(kw.as_str()))
        .cloned()
        .collect();

    let question_left_open = next_chapter_summary
        .map(|s| s.chars().take(80).collect::<String>())
        .unwrap_or_default();

    // Derive conflict pressure from participants and objective
    let conflict_pressure = if !participants.is_empty() && !objective_text.is_empty() {
        let conflict_source = if objective_text.contains("追杀") || objective_text.contains("战斗") {
            "外部威胁".to_string()
        } else if objective_text.contains("选择") || objective_text.contains("决定") {
            "内在抉择".to_string()
        } else if objective_text.contains("秘密") || objective_text.contains("真相") {
            "信息差".to_string()
        } else {
            "情境压力".to_string()
        };
        ConflictPressure {
            source: conflict_source,
            escalation: objective_text.contains("升级") || objective_text.contains("加剧"),
            cost_or_consequence: String::new(),
        }
    } else {
        ConflictPressure::default()
    };

    // Basic emotional curve from scene type keywords
    let mut emotional_beats = Vec::new();
    if objective_text.contains("平静") || objective_text.contains("日常") {
        emotional_beats.push(EmotionalBeat {
            position: "opening".into(),
            emotion: "平静".into(),
            trigger: "场景开端".into(),
        });
    }
    if objective_text.contains("紧张") || objective_text.contains("冲突") || objective_text.contains("追杀") {
        emotional_beats.push(EmotionalBeat {
            position: "mid".into(),
            emotion: "紧张".into(),
            trigger: "冲突升级".into(),
        });
    }
    if objective_text.contains("揭示") || objective_text.contains("发现") || objective_text.contains("真相") {
        emotional_beats.push(EmotionalBeat {
            position: "climax".into(),
            emotion: "震惊/觉悟".into(),
            trigger: "真相揭示".into(),
        });
    }

    SceneCraftPlan {
        scene_id: format!("scene-{}", chapter_title),
        chapter_title: chapter_title.to_string(),
        objective: objective_text,
        participants: participants.to_vec(),
        conflict_pressure,
        character_choice: CharacterChoice::default(),
        information_release: Vec::new(),
        withheld_information: Vec::new(),
        emotional_curve: emotional_beats,
        promise_or_anchor_payoff: promise_payoff,
        ending_hook: EndingHook {
            consequence_delivered: String::new(),
            question_left_open,
        },
        selected_craft_rules: packet.craft_rules.iter().map(|r| r.rule_id.clone()).collect(),
        must_avoid: packet.must_avoid.clone(),
        evidence_refs: packet
            .craft_rules
            .iter()
            .flat_map(|r| r.evidence_refs.clone())
            .collect(),
        required_state_deltas: Vec::new(),
    }
}

/// Format a SceneContract into a prompt section for the LLM.
pub fn format_scene_contract_prompt(
    contract: &crate::writer_agent::world_bible::SceneContract,
) -> String {
    let mut section = String::new();
    section.push_str("\n\n## 世界观约束契约\n\n");
    section.push_str(&format!("- 章节ID: {}\n", contract.chapter_id));
    section.push_str(&format!("- 本章任务: {}\n", contract.mission));

    if !contract.required_facts.is_empty() {
        section.push_str("\n### 必须出现的事实\n");
        for fact in &contract.required_facts {
            section.push_str(&format!("- [{}] {}\n", fact.id, fact.summary));
        }
    }

    if !contract.active_constraints.is_empty() {
        section.push_str("\n### 活跃约束\n");
        for c in &contract.active_constraints {
            let severity_label = match c.severity {
                crate::writer_agent::world_bible::ConstraintSeverity::Hard => "【硬性】",
                crate::writer_agent::world_bible::ConstraintSeverity::Warning => "【警告】",
                crate::writer_agent::world_bible::ConstraintSeverity::Info => "【提示】",
            };
            section.push_str(&format!(
                "- {}{} [{:?}]: {}\n",
                severity_label, c.id, c.kind, c.summary
            ));
            if !c.forbidden_terms.is_empty() {
                section.push_str(&format!("  - 禁用词: {}\n", c.forbidden_terms.join(", ")));
            }
            if !c.required_terms.is_empty() {
                section.push_str(&format!("  - 要求词: {}\n", c.required_terms.join(", ")));
            }
        }
    }

    if !contract.allowed_reveals.is_empty() {
        section.push_str("\n### 允许揭示的信息\n");
        for reveal in &contract.allowed_reveals {
            section.push_str(&format!("- {}\n", reveal));
        }
    }

    if !contract.blocked_reveals.is_empty() {
        section.push_str("\n### 禁止揭示的信息\n");
        for block in &contract.blocked_reveals {
            section.push_str(&format!("- {}\n", block));
        }
    }

    if !contract.required_state_deltas.is_empty() {
        section.push_str("\n### 要求状态变更\n");
        for delta in &contract.required_state_deltas {
            section.push_str(&format!(
                "- [{}] {} (来源: {})\n",
                delta.delta_type, delta.description, delta.source
            ));
        }
    }

    section
}

// ── Prompt Section Formatter ──

pub fn format_craft_prompt_section(packet: &EmpowermentPromptPacket) -> String {
    let mut section = String::new();

    if !packet.scene_contract_prompt.is_empty() {
        section.push_str(&packet.scene_contract_prompt);
    }

    if !packet.chapter_discipline.is_empty() {
        section.push_str("\n\n## 本章写作纪律\n\n");
        for (i, d) in packet.chapter_discipline.iter().enumerate() {
            section.push_str(&format!("{}. {}\n", i + 1, d));
        }
    }

    if !packet.must_avoid.is_empty() {
        section.push_str("\n## 本章禁忌\n\n");
        for item in &packet.must_avoid {
            section.push_str(&format!("- {}\n", item));
        }
    }

    if !packet.self_checklist.is_empty() {
        section.push_str("\n## 写后自检\n\n");
        for item in &packet.self_checklist {
            section.push_str(&format!("- {}\n", item));
        }
    }

    if !packet.memory_examples.is_empty() || !packet.memory_bad_patterns.is_empty() {
        section.push_str("\n## 项目写法记忆\n\n");
        if !packet.memory_examples.is_empty() {
            section.push_str("可借鉴的作者认可写法：\n");
            for example in &packet.memory_examples {
                section.push_str(&format!(
                    "- [{}] {}（{}）\n",
                    example.rule_id,
                    sanitize_prompt_memory_line(&example.excerpt, 120),
                    sanitize_prompt_memory_line(&example.reason, 80)
                ));
            }
        }
        if !packet.memory_bad_patterns.is_empty() {
            section.push_str("必须避开的已拒绝写法：\n");
            for pattern in &packet.memory_bad_patterns {
                section.push_str(&format!(
                    "- [{}] 避免：{}；改法：{}\n",
                    pattern.rule_id,
                    sanitize_prompt_memory_line(&pattern.evidence_excerpt, 120),
                    sanitize_prompt_memory_line(&pattern.correction, 80)
                ));
            }
        }
    }

    section
}

fn sanitize_prompt_memory_line(text: &str, max_chars: usize) -> String {
    let normalized = text
        .replace(['\r', '\n'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let mut snippet: String = normalized.chars().take(max_chars).collect();
    if normalized.chars().count() > max_chars {
        snippet.push_str("...");
    }
    snippet
}

// ── Tests ──

#[cfg(test)]
mod craft_prompt_tests {
    use super::*;

    #[test]
    fn empty_context_falls_back_to_chapter_draft() {
        // Empty objective/target_beat infers ChapterDraft, which selects
        // all rules with "chapter_draft" in their applies_when.
        let packet = compile_empowerment_prompt("", "", 0, false, None, None, None);
        assert!(!packet.craft_rules.is_empty(), "empty context should still select chapter_draft rules");
        assert!(!packet.chapter_discipline.is_empty());
    }

    #[test]
    fn respects_max_rules_limit() {
        let packet = compile_empowerment_prompt(
            "本章继续推进主线剧情",
            "审讯场景", 3, true,
            Some(3), None, None,
        );
        assert!(packet.craft_rules.len() <= 3);
    }

    #[test]
    fn near_payoff_forces_promise_advance() {
        let packet = compile_empowerment_prompt(
            "本章揭开伏笔",
            "关键揭示", 2, true,
            Some(5), Some(2000), None,
        );
        let has_promise = packet
            .craft_rules
            .iter()
            .any(|r| r.rule_id == "promise_advance");
        assert!(has_promise, "near_payoff should force promise_advance selection");
    }

    #[test]
    fn dialogue_scene_selects_dialogue_rules() {
        let packet = compile_empowerment_prompt(
            "高潮审讯：逼问真相",
            "对话推进", 0, false,
            Some(5), Some(2000), None,
        );
        let ids: Vec<&str> = packet.craft_rules.iter().map(|r| r.rule_id.as_str()).collect();
        assert!(ids.contains(&"dialogue_function"), "dialogue scene should select dialogue rules");
    }

    #[test]
    fn scene_craft_plan_defaults_missing_fields() {
        let packet = EmpowermentPromptPacket {
            craft_rules: vec![],
            chapter_discipline: vec![],
            must_avoid: vec![],
            self_checklist: vec![],
            memory_examples: vec![],
            memory_bad_patterns: vec![],
            total_token_estimate: 0,
            scene_contract_prompt: String::new(),
        };
        let plan = build_scene_craft_plan(
            "test-chapter", "objective", &[], "beat", None, &[], &packet,
        );
        assert_eq!(plan.chapter_title, "test-chapter");
        assert!(plan.emotional_curve.is_empty());
        assert!(plan.ending_hook.question_left_open.is_empty());
    }

    #[test]
    fn format_generates_sections() {
        let packet = EmpowermentPromptPacket {
            craft_rules: vec![],
            chapter_discipline: vec!["规则1".into()],
            must_avoid: vec!["禁忌1".into()],
            self_checklist: vec!["检查1".into()],
            memory_examples: vec![],
            memory_bad_patterns: vec![],
            total_token_estimate: 100,
            scene_contract_prompt: String::new(),
        };
        let section = format_craft_prompt_section(&packet);
        assert!(section.contains("本章写作纪律"));
        assert!(section.contains("本章禁忌"));
        assert!(section.contains("写后自检"));
    }

    #[test]
    fn prompt_section_includes_memory_examples_and_bad_patterns() {
        let packet = compile_empowerment_prompt_with_memory(
            "审讯场景，林墨必须逼问真相",
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
                    excerpt_ref: "craft_examples:good".to_string(),
                    excerpt: "他说：现在你必须选择。".to_string(),
                    reason: "作者手改后对话改变选择".to_string(),
                    score_delta: 0.4,
                }],
                bad_patterns: vec![CraftMemoryPromptBadPattern {
                    rule_id: "dialogue_function".to_string(),
                    evidence_ref: "craft_bad_patterns:bad".to_string(),
                    evidence_excerpt: "他说了一整段背景。".to_string(),
                    correction: "让台词改变权力、信息或选择。".to_string(),
                    rejected_count: 2,
                }],
            }],
            None,
        );
        let section = format_craft_prompt_section(&packet);
        assert!(section.contains("项目写法记忆"));
        assert!(section.contains("必须选择"));
        assert!(section.contains("一整段背景"));
    }

    #[test]
    fn scene_contract_prompt_injected_when_provided() {
        let contract = crate::writer_agent::world_bible::SceneContract {
            chapter_id: "ch1".to_string(),
            mission: "test mission".to_string(),
            required_facts: vec![],
            active_constraints: vec![crate::writer_agent::world_bible::CanonConstraint {
                id: "c1".to_string(),
                kind: crate::writer_agent::world_bible::CanonConstraintKind::ForbiddenClaim,
                summary: "禁止飞剑".to_string(),
                trigger_terms: vec!["剑".to_string()],
                forbidden_terms: vec!["飞剑".to_string()],
                required_terms: vec![],
                severity: crate::writer_agent::world_bible::ConstraintSeverity::Hard,
                source_asset_id: "asset1".to_string(),
                evidence: vec![],
                applies_to: Vec::new(),
                expected_consequence: String::new(),
            }],
            required_state_deltas: vec![],
            allowed_reveals: vec!["允许揭示".to_string()],
            blocked_reveals: vec!["禁止揭示".to_string()],
            evidence_refs: vec![],
            continuity_anchors: Vec::new(),
            required_costs: Vec::new(),
        };

        let prompt = format_scene_contract_prompt(&contract);
        assert!(prompt.contains("世界观约束契约"));
        assert!(prompt.contains("ch1"));
        assert!(prompt.contains("test mission"));
        assert!(prompt.contains("禁止飞剑"));
        assert!(prompt.contains("允许揭示"));
        assert!(prompt.contains("禁止揭示"));
    }

    #[test]
    fn craft_prompt_section_includes_scene_contract() {
        let contract = crate::writer_agent::world_bible::SceneContract {
            chapter_id: "ch1".to_string(),
            mission: "test".to_string(),
            required_facts: vec![],
            active_constraints: vec![],
            required_state_deltas: vec![],
            allowed_reveals: vec![],
            blocked_reveals: vec![],
            evidence_refs: vec![],
            continuity_anchors: Vec::new(),
            required_costs: Vec::new(),
        };

        let packet = compile_empowerment_prompt_with_memory(
            "test objective",
            "test beat",
            0,
            false,
            Some(3),
            Some(600),
            None,
            &[],
            Some(&contract),
        );

        assert!(!packet.scene_contract_prompt.is_empty());
        let section = format_craft_prompt_section(&packet);
        assert!(section.contains("世界观约束契约"));
    }
}
