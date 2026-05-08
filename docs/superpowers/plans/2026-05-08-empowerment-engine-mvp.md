# Writing Empowerment Engine MVP Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the writing empowerment engine sidecar: Craft Library, Prompt Compiler, SceneCraftPlan, and ChapterQualityReport. All new modules hang next to the existing chapter generation pipeline — no save-path modifications.

**Architecture:** Four new files (config JSON + 3 Rust modules). Craft Library loads via `include_str!` at compile time. Prompt Compiler selects rules by scene type + promise state. SceneCraftPlan is rule-derived from outline/context. QualityReport uses 8 heuristics with mandatory evidence gating.

**Tech Stack:** Rust, serde_json, `include_str!` for config embedding

---

## File Map

| File | Create/Modify | Responsibility |
|------|--------------|----------------|
| `config/craft-library.json` | Create | 8 craft rules as JSON |
| `agent-writer-backend/src/chapter_generation/craft_types.rs` | Create | Rust types: CraftRule, EmpowermentPromptPacket, SceneCraftPlan, ChapterQualityReport |
| `agent-writer-backend/src/chapter_generation/craft_prompt.rs` | Create | Prompt Compiler + SceneCraftPlan builder |
| `agent-writer-backend/src/chapter_generation/craft_quality.rs` | Create | ChapterQualityReport evaluator |
| `agent-writer-backend/src/chapter_generation/mod.rs` | Modify | Register new modules via `include!` |
| `agent-writer-backend/src/chapter_generation/draft_and_save.in.rs` | Modify | Inject craft prompt into system prompt |

---

### Task 1: Craft Library JSON + Rust Types

**Files:**
- Create: `config/craft-library.json`
- Create: `agent-writer-backend/src/chapter_generation/craft_types.rs`

- [ ] **Step 1: Create craft-library.json**

Write `config/craft-library.json` with all 8 craft rules. Full content from spec Section 1 — a JSON array of 8 objects with fields: id, category, name, applies_when, instruction, anti_patterns, diagnostic_signals, revision_hint, token_cost_hint.

```powershell
mkdir -p config
```

```json
[
  {
    "id": "scene_objective",
    "category": "structure",
    "name": "场景目标",
    "applies_when": ["chapter_draft", "scene_transition"],
    "instruction": "每场戏必须有即时目标和阻力。角色想要什么 + 什么阻碍 = 戏剧张力。没有这对矛盾的段落不是戏，是说明文。",
    "anti_patterns": ["角色被动跟随事件", "只描述环境氛围无行动意图"],
    "diagnostic_signals": ["scene_causality", "character_choice"],
    "revision_hint": "找出场景中最想要某物的角色，给目标加一个当前场景内可感知的阻碍。",
    "token_cost_hint": 80
  },
  {
    "id": "conflict_pressure",
    "category": "structure",
    "name": "冲突压力",
    "applies_when": ["chapter_draft", "scene_transition", "climax"],
    "instruction": "冲突必须改变局面——选择、代价、信息三者至少变化其一。解决了冲突却没改变这三者中的任何一个，冲突是装饰。",
    "anti_patterns": ["冲突后一切照旧", "角色做了选择但没有付出代价"],
    "diagnostic_signals": ["scene_causality", "character_choice"],
    "revision_hint": "在冲突结束后加一个具体代价：失去信任、消耗资源、暴露弱点、做出无法撤回的选择。",
    "token_cost_hint": 85
  },
  {
    "id": "dialogue_function",
    "category": "dialogue",
    "name": "对话功能",
    "applies_when": ["chapter_draft", "dialogue_scene"],
    "instruction": "对话必须改变权力、关系、信息或选择。角色不能说只是解释背景——台词要试探、回避、威胁、隐瞒或带代价地承认。",
    "anti_patterns": ["角色轮流讲背景资料", "对话结束局面完全不变"],
    "diagnostic_signals": ["dialogue_function"],
    "revision_hint": "检查每段对话：它改变了什么？什么都没改的，删掉或改成角色在回避/试探。",
    "token_cost_hint": 90
  },
  {
    "id": "setting_in_scene",
    "category": "worldbuilding",
    "name": "设定入戏",
    "applies_when": ["chapter_draft", "worldbuilding_reveal"],
    "instruction": "世界观设定通过行动、误解、代价和后果进入正文。不要用旁白解释设定——让它成为角色的阻碍、误解来源或交易筹码。",
    "anti_patterns": ["旁白/叙述者大段解释世界规则", "设定信息脱离当前场景张力独立存在"],
    "diagnostic_signals": ["exposition_ratio"],
    "revision_hint": "把解释性段落中50%的信息改成角色因不了解规则而犯错、或因利用规则而付出代价。",
    "token_cost_hint": 85
  },
  {
    "id": "emotion_externalized",
    "category": "prose",
    "name": "情绪外化",
    "applies_when": ["chapter_draft", "emotional_beat"],
    "instruction": "少直接命名情绪，多用动作、停顿、身体反应和环境选择来传递。'他很愤怒'不如'他把杯子放回桌上——太轻了，没发出声音'。",
    "anti_patterns": ["直接命名情绪状态", "叙述者告诉读者角色感觉如何"],
    "diagnostic_signals": ["exposition_ratio"],
    "revision_hint": "搜索情绪词（愤怒/悲伤/恐惧/喜悦/紧张），替换为身体动作或环境互动。",
    "token_cost_hint": 75
  },
  {
    "id": "promise_advance",
    "category": "plot",
    "name": "伏笔推进",
    "applies_when": ["chapter_draft", "mid_volume"],
    "instruction": "伏笔必须被推进、误导、兑现或明确延后。伏笔只被提到但状态不变 = 读者开始遗忘。每章至少推进一条 open promise。",
    "anti_patterns": ["伏笔被提及但不改变状态", "所有伏笔留到最后一章一次性兑现"],
    "diagnostic_signals": ["promise_progress"],
    "revision_hint": "找出本章最相关的 open promise，给一个微小的推进——新线索、新障碍、或角色做出影响该伏笔的决定。",
    "token_cost_hint": 75
  },
  {
    "id": "ending_hook",
    "category": "structure",
    "name": "章末钩子",
    "applies_when": ["chapter_draft", "chapter_end"],
    "instruction": "章末必须有已发生后果和未解决问题。'他们继续赶路'是日记结尾，不是钩子。钩子是'她看了他一眼——他刚才说谎了，两个人都知道'。",
    "anti_patterns": ["章节在平淡中自然结束", "只有设置悬念没有兑现本事件的后果"],
    "diagnostic_signals": ["ending_hook"],
    "revision_hint": "加一个刚发生的后果（不是大事件，是一个具体变化）和一个未解决的问题（不是谜语，是一个角色面临的选择）。",
    "token_cost_hint": 70
  },
  {
    "id": "genre_pleasure",
    "category": "genre",
    "name": "类型快感",
    "applies_when": ["chapter_draft"],
    "instruction": "不同小说类型满足不同读者期待。悬疑要信息差和误导后的揭示。仙侠要代价和境界压力。言情要关系位移和试探。每章至少有一个类型快感时刻。",
    "anti_patterns": ["写了战斗但没有代价", "写了悬疑但没有新信息或新误导"],
    "diagnostic_signals": ["scene_causality", "promise_progress"],
    "revision_hint": "确认本章的小说类型，在合适位置插入一个该类型的标志性快感时刻。",
    "token_cost_hint": 80
  }
]
```

- [ ] **Step 2: Create craft_types.rs**

Create `agent-writer-backend/src/chapter_generation/craft_types.rs` with all Rust types from spec Section 1:

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CraftRule {
    pub id: String,
    pub category: String,
    pub name: String,
    pub applies_when: Vec<String>,
    pub instruction: String,
    pub anti_patterns: Vec<String>,
    pub diagnostic_signals: Vec<String>,
    pub revision_hint: String,
    pub token_cost_hint: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CraftRuleSelection {
    pub rule_id: String,
    pub reason: String,
    pub evidence_refs: Vec<String>,
    pub priority: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmpowermentPromptPacket {
    pub craft_rules: Vec<CraftRuleSelection>,
    pub chapter_discipline: Vec<String>,
    pub must_avoid: Vec<String>,
    pub self_checklist: Vec<String>,
    pub total_token_estimate: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SceneCraftPlan {
    pub scene_id: String,
    pub chapter_title: String,
    pub objective: String,
    pub participants: Vec<String>,
    pub conflict_pressure: ConflictPressure,
    pub character_choice: CharacterChoice,
    pub information_release: Vec<String>,
    pub withheld_information: Vec<String>,
    pub emotional_curve: Vec<EmotionalBeat>,
    pub promise_or_anchor_payoff: Vec<String>,
    pub ending_hook: EndingHook,
    pub selected_craft_rules: Vec<String>,
    pub must_avoid: Vec<String>,
    pub evidence_refs: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConflictPressure {
    pub source: String,
    pub escalation: bool,
    pub cost_or_consequence: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CharacterChoice {
    pub character: String,
    pub options: Vec<String>,
    pub cost: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmotionalBeat {
    pub position: String,
    pub emotion: String,
    pub trigger: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EndingHook {
    pub consequence_delivered: String,
    pub question_left_open: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IssueSeverity {
    None,
    Minor,
    Major,
    Fatal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QualityMetricResult {
    pub metric: String,
    pub score: f32,
    pub severity: IssueSeverity,
    pub evidence_excerpt: String,
    pub rule_source: String,
    pub reason: String,
    pub revision_hint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QualityIssue {
    pub metric: String,
    pub severity: IssueSeverity,
    pub evidence: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChapterQualityReport {
    pub chapter_title: String,
    pub overall_score: f32,
    pub fatal_issues: Vec<QualityIssue>,
    pub major_issues: Vec<QualityIssue>,
    pub metric_results: Vec<QualityMetricResult>,
    pub top_revision_targets: Vec<String>,
    pub no_fatal_issue: bool,
}
```

- [ ] **Step 3: Register craft_types in mod.rs**

In `agent-writer-backend/src/chapter_generation/mod.rs`, add after existing includes:

```rust
include!("craft_types.rs");
```

- [ ] **Step 4: Verify compilation**

```powershell
cargo check -p agent-writer
```

- [ ] **Step 5: Add deserialization test**

Append a test at the bottom of `craft_types.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserializes_craft_rule() {
        let json = r#"{
            "id": "test_rule",
            "category": "prose",
            "name": "测试规则",
            "applies_when": ["chapter_draft"],
            "instruction": "测试指令",
            "anti_patterns": ["反模式1"],
            "diagnostic_signals": ["signal1"],
            "revision_hint": "修改建议",
            "token_cost_hint": 100
        }"#;
        let rule: CraftRule = serde_json::from_str(json).expect("should deserialize");
        assert_eq!(rule.id, "test_rule");
        assert_eq!(rule.token_cost_hint, 100);
    }

    #[test]
    fn craft_library_json_is_valid() {
        let json = include_str!("../../../config/craft-library.json");
        let rules: Vec<CraftRule> = serde_json::from_str(json).expect("craft-library.json must be valid");
        assert_eq!(rules.len(), 8, "expected 8 craft rules");
        let ids: Vec<&str> = rules.iter().map(|r| r.id.as_str()).collect();
        assert!(ids.contains(&"scene_objective"));
        assert!(ids.contains(&"ending_hook"));
    }
}
```

- [ ] **Step 6: Run tests and commit**

```powershell
cargo test -p agent-writer --lib craft_types
```

```bash
git add config/craft-library.json agent-writer-backend/src/chapter_generation/craft_types.rs agent-writer-backend/src/chapter_generation/mod.rs
git commit -m "feat: add Craft Library JSON config and Rust types

8 core craft rules (scene_objective, conflict_pressure, dialogue_function,
setting_in_scene, emotion_externalized, promise_advance, ending_hook,
genre_pleasure) with CraftRule, CraftRuleSelection, EmpowermentPromptPacket,
SceneCraftPlan, ChapterQualityReport, and related types.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>"
```

---

### Task 2: Prompt Compiler + SceneCraftPlan Builder

**Files:**
- Create: `agent-writer-backend/src/chapter_generation/craft_prompt.rs`

- [ ] **Step 1: Create craft_prompt.rs with library loader**

```rust
use std::sync::OnceLock;
use crate::chapter_generation::craft_types::*;

fn craft_library() -> &'static Vec<CraftRule> {
    static LIBRARY: OnceLock<Vec<CraftRule>> = OnceLock::new();
    LIBRARY.get_or_init(|| {
        serde_json::from_str(include_str!("../../../config/craft-library.json"))
            .expect("craft-library.json must be valid")
    })
}

const DEFAULT_MAX_RULES: usize = 5;
const DEFAULT_MAX_PROMPT_CHARS: usize = 600;
```

- [ ] **Step 2: Implement scene type inference**

```rust
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

fn scene_type_tag(scene_type: &SceneType) -> &'static str {
    match scene_type {
        SceneType::ChapterDraft => "chapter_draft",
        SceneType::DialogueScene => "dialogue_scene",
        SceneType::ActionScene => "chapter_draft",
        SceneType::RevelationScene => "worldbuilding_reveal",
        SceneType::TurningPoint => "turning_point",
    }
}
```

- [ ] **Step 3: Implement compile_empowerment_prompt**

```rust
pub fn compile_empowerment_prompt(
    objective: &str,
    target_beat: &str,
    open_promise_count: usize,
    has_near_payoff: bool,
    max_rules: Option<usize>,
    max_prompt_chars: Option<usize>,
) -> EmpowermentPromptPacket {
    let max_rules = max_rules.unwrap_or(DEFAULT_MAX_RULES);
    let max_prompt_chars = max_prompt_chars.unwrap_or(DEFAULT_MAX_PROMPT_CHARS);
    let library = craft_library();
    let scene_type = infer_scene_type(objective, target_beat);
    let scene_tag = scene_type_tag(&scene_type);

    // Collect matching rules
    let mut candidates: Vec<&CraftRule> = library
        .iter()
        .filter(|rule| rule.applies_when.iter().any(|tag| tag == scene_tag || tag == "chapter_draft"))
        .collect();

    // Force-select promise_advance if near payoff
    if has_near_payoff {
        // Move promise_advance to front
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
        if chars_used + rule_chars > max_prompt_chars {
            break;
        }
        chars_used += rule_chars;
        selected.push(CraftRuleSelection {
            rule_id: rule.id.clone(),
            reason: format!("当前场景类型匹配: {}", rule.name),
            evidence_refs: vec![format!("scene_type:{}", scene_tag)],
            priority: if rule.id == "promise_advance" && has_near_payoff { 10 } else { 5 },
        });
    }

    // Build output
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

    EmpowermentPromptPacket {
        craft_rules: selected,
        chapter_discipline,
        must_avoid,
        self_checklist,
        total_token_estimate: chars_used,
    }
}
```

- [ ] **Step 4: Implement build_scene_craft_plan**

```rust
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
        .map(|s| {
            s.chars().take(80).collect::<String>()
        })
        .unwrap_or_default();

    SceneCraftPlan {
        scene_id: format!("scene-{}", chapter_title),
        chapter_title: chapter_title.to_string(),
        objective: objective_text,
        participants: participants.to_vec(),
        conflict_pressure: ConflictPressure::default(),
        character_choice: CharacterChoice::default(),
        information_release: Vec::new(),
        withheld_information: Vec::new(),
        emotional_curve: Vec::new(),
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
    }
}
```

- [ ] **Step 5: Implement prompt format helper**

```rust
pub fn format_craft_prompt_section(packet: &EmpowermentPromptPacket) -> String {
    let mut section = String::new();

    if !packet.chapter_discipline.is_empty() {
        section.push_str("\n\n## 本章写作纪律\n\n");
        for (i, d) in packet.chapter_discipline.iter().enumerate() {
            section.push_str(&format!("{}. {}\n", i + 1, d));
        }
    }

    if !packet.must_avoid.is_empty() {
        section.push_str("\n## 本章禁忌\n\n");
        for (i, m) in packet.must_avoid.iter().enumerate() {
            section.push_str(&format!("- {}\n", m));
        }
    }

    if !packet.self_checklist.is_empty() {
        section.push_str("\n## 写后自检\n\n");
        for item in &packet.self_checklist {
            section.push_str(&format!("- {}\n", item));
        }
    }

    section
}
```

- [ ] **Step 7: Add tests**

Append to craft_prompt.rs:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_context_returns_empty_packet() {
        let packet = compile_empowerment_prompt("", "", 0, false, None, None);
        assert!(packet.craft_rules.is_empty());
        assert!(packet.chapter_discipline.is_empty());
    }

    #[test]
    fn respects_max_rules_limit() {
        let packet = compile_empowerment_prompt(
            "本章继续推进主线剧情",
            "审讯场景", 3, true,
            Some(3), None,
        );
        assert!(packet.craft_rules.len() <= 3);
    }

    #[test]
    fn near_payoff_forces_promise_advance() {
        let packet = compile_empowerment_prompt(
            "本章揭开伏笔",
            "关键揭示", 2, true,
            Some(5), Some(2000),
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
            Some(5), Some(2000),
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
            total_token_estimate: 0,
        };
        let plan = build_scene_craft_plan(
            "test-chapter", "objective", &[], "beat", None, &[], &packet,
        );
        assert_eq!(plan.chapter_title, "test-chapter");
        assert!(plan.emotional_curve.is_empty());
        assert!(plan.ending_hook.question_left_open.is_empty());
    }
}
```

- [ ] **Step 8: Register craft_prompt in mod.rs**

In `agent-writer-backend/src/chapter_generation/mod.rs`, add:

```rust
include!("craft_prompt.rs");
```

- [ ] **Step 9: Run tests and commit**

```powershell
cargo test -p agent-writer --lib craft_prompt
cargo clippy -p agent-writer --all-targets -- -D warnings
```

```bash
git add agent-writer-backend/src/chapter_generation/craft_prompt.rs agent-writer-backend/src/chapter_generation/mod.rs
git commit -m "feat: add Prompt Compiler and SceneCraftPlan builder

Rule-based scene type inference, craft rule selection with token
budget, and SceneCraftPlan derivation from outline/context. Zero
LLM calls — pure heuristic matching.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>"
```

---

### Task 3: ChapterQualityReport Evaluator

**Files:**
- Create: `agent-writer-backend/src/chapter_generation/craft_quality.rs`

- [ ] **Step 1: Create craft_quality.rs**

Full implementation with 8 metrics and evidence gating:

```rust
use crate::chapter_generation::craft_types::*;

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
    let mut metric_results = Vec::new();

    metric_results.push(metric_length_compliance(chapter_text, target_min_chars, target_max_chars));
    metric_results.push(metric_dialogue_function(chapter_text));
    metric_results.push(metric_exposition_ratio(chapter_text));
    metric_results.push(metric_ending_hook(chapter_text));
    metric_results.push(metric_scene_causality(chapter_text));
    metric_results.push(metric_promise_progress(chapter_text, open_promise_keywords));
    // anchor_carry and style_drift require provider calls or pre-built snapshots;
    // for MVP, emit placeholder "insufficient evidence" results
    metric_results.push(gated_metric(
        "anchor_carry", 0.5, "", "anchor_carry.rs",
        "需要项目级锚点数据，本次评估跳过", "在完整写作项目中重新运行"
    ));
    metric_results.push(gated_metric(
        "style_drift", 0.5, "", "author_voice.rs",
        "需要作者风格快照，本次评估跳过", "在完整写作项目中重新运行"
    ));

    let overall_score: f32 = metric_results
        .iter()
        .map(|m| {
            let weight = OVERALL_WEIGHTS.iter().find(|(name, _)| *name == m.metric).map(|(_, w)| *w).unwrap_or(0.125);
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

    ChapterQualityReport {
        chapter_title: chapter_title.to_string(),
        overall_score,
        fatal_issues,
        major_issues,
        metric_results,
        top_revision_targets,
        no_fatal_issue: fatal_issues.is_empty(),
    }
}

fn gated_metric(
    metric: &str, score: f32, evidence: &str, rule_source: &str,
    reason: &str, revision_hint: &str,
) -> QualityMetricResult {
    if score >= 0.8 {
        QualityMetricResult {
            metric: metric.into(), score, severity: IssueSeverity::None,
            evidence_excerpt: String::new(), rule_source: rule_source.into(),
            reason: "该项表现良好".into(), revision_hint: String::new(),
        }
    } else if evidence.is_empty() {
        QualityMetricResult {
            metric: metric.into(), score: 0.5, severity: IssueSeverity::None,
            evidence_excerpt: String::new(), rule_source: rule_source.into(),
            reason: format!("证据不足，无法确定是否存在问题。{}", reason),
            revision_hint: "需要更多上下文或更大样本量后重新评估".into(),
        }
    } else {
        let severity = if score < 0.3 { IssueSeverity::Fatal }
            else if score < 0.5 { IssueSeverity::Major }
            else { IssueSeverity::Minor };
        QualityMetricResult {
            metric: metric.into(), score, severity,
            evidence_excerpt: evidence.to_string(), rule_source: rule_source.into(),
            reason: reason.to_string(), revision_hint: revision_hint.to_string(),
        }
    }
}

fn metric_length_compliance(text: &str, min_chars: usize, max_chars: usize) -> QualityMetricResult {
    let count = text.chars().count();
    if count >= min_chars && count <= max_chars {
        gated_metric("length_compliance", 1.0, &format!("{count} chars"), "chapter_contract",
            "字数合规", "")
    } else if count < min_chars {
        let ratio = count as f32 / min_chars as f32;
        gated_metric("length_compliance", ratio * 0.7,
            &format!("{count} chars < min {min_chars}"), "chapter_contract",
            &format!("正文字数 {count} 低于最低要求 {min_chars}"), "扩展场景或增加细节以达到最低字数")
    } else {
        let ratio = max_chars as f32 / count as f32;
        gated_metric("length_compliance", ratio * 0.7,
            &format!("{count} chars > max {max_chars}"), "chapter_contract",
            &format!("正文字数 {count} 超出上限 {max_chars}"), "精简冗余描写或拆分场景")
    }
}

fn metric_dialogue_function(text: &str) -> QualityMetricResult {
    let dialogue_markers = ["\"", "\u{201c}", "\u{201d}", "\u{300c}", "\u{300d}", "说", "问", "答", "道"];
    let function_signals = ["决定", "拒绝", "承认", "隐瞒", "威胁", "交换", "选择", "妥协", "逼问", "暗示", "试探", "回避"];

    let has_dialogue = dialogue_markers.iter().any(|m| text.contains(m));
    if !has_dialogue {
        return gated_metric("dialogue_function", 1.0, "", "craft:dialogue_function",
            "本章无对话场景，不适用", "");
    }

    let signal_count = function_signals.iter().filter(|s| text.contains(*s)).count();
    let score = (signal_count as f32 / 3.0).min(1.0);
    let evidence = if signal_count > 0 {
        function_signals.iter().filter(|s| text.contains(*s)).take(3).cloned().collect::<Vec<_>>().join(", ")
    } else {
        String::new()
    };

    gated_metric("dialogue_function", score, &evidence, "craft:dialogue_function",
        &format!("对话功能信号: {signal_count}/12"), "确保对话改变了权力、关系、信息或选择")
}

fn metric_exposition_ratio(text: &str) -> QualityMetricResult {
    let action_verbs = ["拔", "握", "推", "拉", "砍", "刺", "走", "跑", "跳", "拿", "放", "打", "挡", "追", "转",
        "翻", "看", "盯", "藏", "递", "交", "救", "抢", "护", "站起", "坐下", "点头", "摇头"];
    let dialogue_markers = ["\"", "\u{201c}", "\u{300c}", "说", "问", "答"];

    let paragraphs: Vec<&str> = text.split(|c| c == '\n').filter(|p| !p.trim().is_empty()).collect();
    if paragraphs.is_empty() {
        return gated_metric("exposition_ratio", 1.0, "", "craft:setting_in_scene", "无段落", "");
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

    let ratio = expo_para_count as f32 / paragraphs.len() as f32;
    let score = if ratio > 0.4 { 0.3 } else if ratio > 0.25 { 0.6 } else { 0.9 };
    let evidence = if expo_para_count > 0 {
        format!("{expo_para_count}/{} 段落为纯说明（>200chars 无动作/对话）", paragraphs.len())
    } else {
        String::new()
    };

    gated_metric("exposition_ratio", score, &evidence, "craft:setting_in_scene",
        &format!("说明性段落占比 {:.0}%", ratio * 100.0), "将说明段落中的信息改写成角色行动、误解或对话")
}

fn metric_ending_hook(text: &str) -> QualityMetricResult {
    let tail = text.chars().rev().take(300).collect::<String>().chars().rev().collect::<String>();

    let consequence_signals = ["后果", "代价", "变了", "不再", "从此", "已经", "终于", "失去", "获得", "明白", "知道", "决定"];
    let question_signals = ["但是", "然而", "不过", "还不知道", "没发现", "不知道", "选择", "怎么办", "要不要", "能不能"];

    let has_consequence = consequence_signals.iter().any(|s| tail.contains(s));
    let has_question = question_signals.iter().any(|s| tail.contains(s));

    let score = match (has_consequence, has_question) {
        (true, true) => 0.9,
        (true, false) => 0.5,
        (false, true) => 0.5,
        (false, false) => 0.2,
    };

    let evidence = tail.chars().rev().take(100).collect::<String>().chars().rev().collect::<String>();

    gated_metric("ending_hook", score, &evidence, "craft:ending_hook",
        &format!("后果信号={}, 未解信号={}", has_consequence, has_question),
        "章末加一个刚发生的后果和一个角色面临的选择")
}

fn metric_scene_causality(text: &str) -> QualityMetricResult {
    let causality_markers = ["因为", "所以", "因此", "于是", "导致", "逼得", "只好", "不得不", "结果", "后果"];
    let count: usize = causality_markers.iter().map(|m| text.matches(m).count()).sum();
    let char_count = text.chars().count().max(1);
    let density = count as f32 / char_count as f32 * 500.0;

    let score = if density >= 1.0 { 0.9 } else if density >= 0.5 { 0.6 } else { 0.3 };
    let evidence = if count > 0 {
        causality_markers.iter().filter(|m| text.contains(*m)).take(3).cloned().collect::<Vec<_>>().join(", ")
    } else {
        String::new()
    };

    gated_metric("scene_causality", score, &evidence, "craft:scene_causality",
        &format!("因果连接词密度: {:.2}/500chars", density), "增加事件间的因果连接，少用'然后'，多用'因此/于是/导致'")
}

fn metric_promise_progress(text: &str, keywords: &[String]) -> QualityMetricResult {
    if keywords.is_empty() {
        return gated_metric("promise_progress", 0.5, "", "craft:promise_advance",
            "无 open promise 关键词，跳过评估", "");
    }

    let matched: Vec<&String> = keywords.iter().filter(|kw| text.contains(kw.as_str())).collect();
    let score = if matched.is_empty() { 0.2 } else { (matched.len() as f32 / keywords.len() as f32).min(1.0) };
    let evidence = matched.iter().take(3).map(|s| s.as_str()).collect::<Vec<_>>().join(", ");

    gated_metric("promise_progress", score, &evidence, "craft:promise_advance",
        &format!("{}/{} promise keywords found in text", matched.len(), keywords.len()),
        "检查 open promises 是否在本章中被推进、误导或兑现")
}
```

- [ ] **Step 3: Add tests**

Append to craft_quality.rs:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_text_scores_low_but_no_panic() {
        let plan = SceneCraftPlan::default();
        let report = evaluate_chapter_quality("", "test-chapter", &plan, &[], 3000, 4000);
        assert!(report.overall_score <= 0.5);
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
        let text = "纯叙述段落，没有任何对话标记、动作动词或因果连接词。只是描述。更多的描述。依然是描述——没有选择，没有后果，没有改变。";
        let report = evaluate_chapter_quality(text, "test-chapter", &plan, &[], 0, 500);
        // exposition should gate to insufficient or low score
        let expo = report.metric_results.iter().find(|m| m.metric == "exposition_ratio").unwrap();
        assert!(expo.reason.contains("说明性段落占比") || expo.reason.contains("证据不足"));
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
}
```

- [ ] **Step 4: Register craft_quality in mod.rs**

In `agent-writer-backend/src/chapter_generation/mod.rs`, add:

```rust
include!("craft_quality.rs");
```

- [ ] **Step 5: Run tests and commit**

```powershell
cargo test -p agent-writer --lib craft_quality
cargo clippy -p agent-writer --all-targets -- -D warnings
```

```bash
git add agent-writer-backend/src/chapter_generation/craft_quality.rs agent-writer-backend/src/chapter_generation/mod.rs
git commit -m "feat: add ChapterQualityReport evaluator

8 heuristic metrics (length, dialogue, exposition, ending hook,
causality, promise progress, anchor carry placeholder, style drift
placeholder) with mandatory evidence gating.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>"
```

---

### Task 4: Pipeline Integration

**Files:**
- Modify: `agent-writer-backend/src/chapter_generation/draft_and_save.in.rs`

- [ ] **Step 1: Inject craft prompt into draft system prompt**

In `draft_and_save.in.rs`, find the `let system_prompt = format!(...)` block (around line 20-36). After it, append:

```rust
    // Append craft empowerment section
    // MVP: uses prompt_context summary + source count as context signal
    let summary_snippet: String = context
        .prompt_context
        .chars()
        .take(200)
        .collect();
    let open_promise_count = context
        .sources
        .iter()
        .filter(|s| s.source_type == "promise" || s.label.contains("promise"))
        .count();

    let craft_packet = crate::chapter_generation::craft_prompt::compile_empowerment_prompt(
        &summary_snippet,
        "", // target_beat — to be wired from preflight in future milestone
        open_promise_count,
        false, // has_near_payoff — to be wired from preflight in future milestone
        Some(5),
        Some(600),
    );

    let system_prompt = if !craft_packet.chapter_discipline.is_empty() {
        let craft_section =
            crate::chapter_generation::craft_prompt::format_craft_prompt_section(&craft_packet);
        format!("{}{}", system_prompt, craft_section)
    } else {
        system_prompt
    };
```

**NOTE:** `prompt_context` is `String` on `BuiltChapterContext` — the assembled context text. We take first 200 chars as a summary snippet for scene type inference. `open_promise_count` counts sources with "promise" in type/label. Future milestones will pass full `ChapterMission` + `PromiseLedger` through `BuildChapterContextInput` for richer matching.

- [ ] **Step 2: Verify integration**

```powershell
cargo check -p agent-writer
cargo test -p agent-writer --lib
cargo clippy -p agent-writer --all-targets -- -D warnings
```

- [ ] **Step 3: Commit**

```bash
git add agent-writer-backend/src/chapter_generation/draft_and_save.in.rs
git commit -m "feat: inject craft prompt into chapter draft system prompt

Appends writing craft discipline, must-avoid, and self-checklist
sections to the chapter generation system prompt when outline
context is available.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>"
```

---

### Task 5: Full Integration Verification

- [ ] **Step 1: Run complete test suite**

```powershell
cargo fmt --check
cargo check --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p agent-harness-core
cargo test -p agent-writer --lib
cargo test -p forge-agent-mcp
```

- [ ] **Step 2: Verify all tests pass and commit if any fixes needed**

No changes expected — this is verification only.

---

## Task Summary

| Task | Files | New Lines | Est. Time |
|------|-------|-----------|-----------|
| 1. Craft Library + Types | craft-library.json, craft_types.rs, mod.rs | ~250 | 30 min |
| 2. Prompt Compiler + SceneCraftPlan | craft_prompt.rs, mod.rs | ~250 | 40 min |
| 3. ChapterQualityReport | craft_quality.rs, mod.rs | ~250 | 40 min |
| 4. Pipeline Integration | draft_and_save.in.rs | ~30 | 20 min |
| 5. Integration Check | — | — | 10 min |
| **Total** | **6 files** | **~780** | **~2.5 hrs** |
