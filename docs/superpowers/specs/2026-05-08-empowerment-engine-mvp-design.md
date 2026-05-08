# Writing Empowerment Engine MVP Design

## Summary

为 Forge Agent 接入写作赋能引擎的最小可行版本。新增 Craft Library 技法库、Prompt Compiler 技法选择器、SceneCraftPlan 写前计划、ChapterQualityReport 结构化质量诊断。不改动保存安全链路——所有新增模块作为侧车挂在章节生成管线旁边。

## Decisions

| 决策 | 选择 |
|------|------|
| 范围 | MVP + 启发式诊断（不包含 Targeted Revision） |
| Craft Library 格式 | 纯 JSON 配置（`config/craft-library.json`） |
| Prompt Compiler 位置 | 独立模块 `chapter_generation/craft_prompt.rs` |
| SceneCraftPlan 生成 | 规则推导，不调 LLM |
| Quality Report 指标数 | 8 项（3 复用 + 5 新增） |

---

## Section 1: Data Layer

### `config/craft-library.json`

8 条核心技法，每条包含 id、category、name、applies_when、instruction、anti_patterns、diagnostic_signals、revision_hint、token_cost_hint。

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
    "revision_hint": "把解释性段落中 50% 的信息改成角色因不了解规则而犯错、或因利用规则而付出代价。",
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

### `agent-writer-backend/src/chapter_generation/craft_types.rs`

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

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
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

---

## Section 2: Prompt Compiler

### `agent-writer-backend/src/chapter_generation/craft_prompt.rs`

**入口函数：**

```rust
pub fn compile_empowerment_prompt(
    context: &BuiltChapterContext,
    mission: &ChapterMission,
    promises: &[Promise],
    craft_library: &[CraftRule],
    max_rules: usize,
    max_prompt_chars: usize,
) -> EmpowermentPromptPacket
```

**选择流程（4 步，纯规则）：**

1. **场景类型推断** — 从 chapter mission 的 objective + target beat 推断场景类型：
   - 含 "审讯"/"询问"/"逼问" → `dialogue_scene`
   - 含 "战斗"/"对决"/"出手" → `action_scene`
   - 含 "揭示"/"发现"/"真相" → `revelation_scene`
   - 含 "转折"/"决定"/"选择" → `turning_point`
   - 默认 → `chapter_draft`

2. **Promise 状态检查** — 遍历 open promises：
   - 有 promise 标记 `near_payoff` → 强制选中 `promise_advance`
   - 有 promise 标记 `overdue` → 强制选中 `promise_advance`，优先级最高

3. **技法匹配** — 对每条 CraftRule，检查 `applies_when` 是否匹配当前场景类型。匹配的按 `token_cost_hint` 排序，优先选 token 成本低的。

4. **Token 预算裁剪** — 从匹配的技法中贪心选取，直到 `max_rules` 条或 `max_prompt_chars` 字符预算耗尽。默认 max_rules=5, max_prompt_chars=600。

**输出构建：** 每条选中的技法生成：
- `chapter_discipline` ← `rule.instruction`
- `must_avoid` ← 每条 `rule.anti_patterns`
- `self_checklist` ← 从 `rule.diagnostic_signals` 推导（"检查对话是否改变了权力/关系/信息/选择"）

**注入格式（追加到 draft system prompt 末尾）：**

```text
## 本章写作纪律

[chapter_discipline 每条一行，编号]

## 本章禁忌

[must_avoid 每条一行]

## 写后自检

[self_checklist 每条一行]
```

---

## Section 3: SceneCraftPlan

**生成入口：** `agent-writer-backend/src/chapter_generation/craft_prompt.rs`

```rust
pub fn build_scene_craft_plan(
    context: &BuiltChapterContext,
    mission: &ChapterMission,
    promises: &[Promise],
    empowerment_packet: &EmpowermentPromptPacket,
) -> SceneCraftPlan
```

**规则推导逻辑：**

| 字段 | 推导来源 |
|------|---------|
| `objective` | `mission.objective` 或 outline summary 首句 |
| `participants` | 从 outline summary + lorebook 关键词匹配角色名 |
| `conflict_pressure.source` | 从 mission.constraints 提取冲突关键词 |
| `conflict_pressure.cost_or_consequence` | 从 promise 的 stakes 字段推导 |
| `promise_or_anchor_payoff` | 遍历 open promises，匹配本章标题/编号的 promise id |
| `ending_hook.question_left_open` | 下章 outline summary 首句反向推导 |
| `selected_craft_rules` | `empowerment_packet.craft_rules` 的 rule_id 列表 |
| `must_avoid` | `empowerment_packet.must_avoid` |
| `evidence_refs` | 各字段推导时记录的源引用 |

置信度低的字段（无匹配 promise、无下章 outline、无角色信息）留空/`Vec::new()`/`Default::default()`。

**持久化：** `{request_id}.scene_craft_plan.json`

**Pipeline 插入点：**
```
context built → compile_empowerment_prompt() → build_scene_craft_plan() 
→ save artifact → inject craft prompt into draft system prompt → LLM draft
```

---

## Section 4: ChapterQualityReport

### `agent-writer-backend/src/chapter_generation/craft_quality.rs`（新文件）

**入口函数：**

```rust
pub fn evaluate_chapter_quality(
    chapter_text: &str,
    chapter_title: &str,
    scene_plan: &SceneCraftPlan,
    author_voice: &AuthorVoiceSnapshot,
    chapter_contract: &ChapterContract,
) -> ChapterQualityReport
```

**8 项指标实现：**

| 指标 | 权重 | 实现方式 |
|------|------|---------|
| `anchor_carry` | 0.15 | 复用 `anchor_carry::carry_score()` |
| `style_drift` | 0.10 | 复用 `author_voice::detect_drift()` |
| `length_compliance` | 0.10 | 复用 `ChapterContract::validate_length()` |
| `dialogue_function` | 0.15 | 检测对话段落中是否含决定/拒绝/承认/隐瞒/威胁/交换关键词。占比 <30% 降分 |
| `exposition_ratio` | 0.15 | 计算 >200 chars 无对话标记/动作动词的连续段落占比。>40% 降分 |
| `ending_hook` | 0.15 | 末 300 chars 中检测后果/选择/风险/未解问题关键词。缺失降分 |
| `scene_causality` | 0.10 | 检测 "因为/所以/因此/于是/导致/逼得" 连接词密度。密度 <1/500chars 降分 |
| `promise_progress` | 0.10 | 遍历 open promises 的关键词是否在正文中出现。未出现降分 |

**证据门控（三条红线落地）：**

```rust
fn gated_metric(
    metric: &str, score: f32, evidence: &str, rule_source: &str,
    reason: &str, revision_hint: &str
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
            reason: "证据不足，无法确定是否存在问题".into(),
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
```

**整体评分：** 8 项加权平均。`overall_score < 0.4` 或任何 `Fatal` → `no_fatal_issue = false`。

**Pipeline 插入点：**
```
draft complete → chapter_text + SceneCraftPlan → evaluate_chapter_quality()
→ save {request_id}.quality_report.json → emit ChapterGenerationEvent.qualityReport
```

---

## Files Summary

| 文件 | 操作 | 职责 |
|------|------|------|
| `config/craft-library.json` | 创建 | 8 条核心写作技法 |
| `agent-writer-backend/src/chapter_generation/craft_types.rs` | 创建 | CraftRule, EmpowermentPromptPacket, SceneCraftPlan, ChapterQualityReport 等 Rust 类型 |
| `agent-writer-backend/src/chapter_generation/craft_prompt.rs` | 创建 | Prompt Compiler + SceneCraftPlan 生成 |
| `agent-writer-backend/src/chapter_generation/craft_quality.rs` | 创建 | ChapterQualityReport 评估 |
| `agent-writer-backend/src/chapter_generation/mod.rs` | 修改 | 注册新模块 |
| `agent-writer-backend/src/chapter_generation/context.in.rs` | 修改 | Pipeline 中调用 compiler |
| `agent-writer-backend/src/chapter_generation/draft_and_save.in.rs` | 修改 | Draft system prompt 注入 craft prompt |

## Acceptance Criteria

```powershell
cargo fmt --check
cargo check --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p agent-harness-core
cargo test -p agent-writer --lib
cargo test -p forge-agent-mcp
```

- `config/craft-library.json` 能被成功解析为 `Vec<CraftRule>`
- `compile_empowerment_prompt` 输入空 context 返回空 packet
- `compile_empowerment_prompt` 在 max_rules=5 时最多返回 5 条
- `build_scene_craft_plan` 所有字段非 panic，缺失字段留空字符串
- `evaluate_chapter_quality` 对空文本返回低分但不 panic
- `evaluate_chapter_quality` 对无证据的低分项降级为 "证据不足"
- 所有已有测试不回退

## Out of Scope (Not in this MVP)

- Targeted Revision（定向修订）— 下一 milestone
- Craft Memory / Feedback Memory 持久化 — 下一 milestone
- Sprint Quality Gate — 下一 milestone
- Eval Harness / JSONL runner — 后续
- LLM judge 补充诊断 — 先用规则
- `promise_progress` 的完整实现（需要 open promises 和关键词匹配表）— 本次只做骨架
