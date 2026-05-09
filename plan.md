# Forge Agent Kernel Plan

## 当前结论

Forge Agent 现在已经收敛为 Headless-only 写作 Agent Runtime：唯一支持入口是 `forge-agent-mcp` 的 MCP stdio server，`agent-writer` 作为 headless backend library 被调用，不再维护 Tauri 桌面运行时、renderer event、桌面命令层和双模式 feature 分支。

当前内核不是“弱模型包装器”，而是一个工程约束较强的垂直写作 Agent：MCP 工具面、写作项目存储、上下文组装、记忆账本、审批边界、章节生成安全检查和测试覆盖都已经成型。短板也很明确：它还不是 planner-first 的自主执行内核，任务执行仍以 LLM tool-call round 为主，路由偏规则化，token/成本估算偏静态，写作质量缺少固定回归评测。

一句话评估：

- 作为 Headless MCP 写作后端：偏强，约 8/10。
- 作为通用自主 Agent 内核：中等偏强，约 6/10。
- 作为长篇中文写作质量系统：工程基础强，质量评测闭环仍偏弱，约 6.5/10。

## 架构现状

### `agent-harness-core`

核心职责是通用 Agent 执行层：

- `agent_loop.rs`: provider 调用、streaming、tool-call round、context window guard、provider guard、compaction event。
- `tool_registry.rs` / `tool_executor.rs`: 工具注册、有效工具过滤、权限检查、doom-loop 检测、审计事件。
- `permission.rs`: read-only、workspace-write、danger-full-access 等权限策略。
- `task_packet.rs`: 任务目标、上下文要求、工具策略和验收条件的结构化载体。
- `compaction.rs`: water-level compaction、event-driven compaction、microcompact。
- `provider/*`: OpenAI-compatible provider 抽象、usage、streaming、模型上下文窗口信息。
- `context_pack.rs`、`context_window_guard.rs`、`prompt_cache.rs`、`run_trace.rs`: 上下文预算、窗口保护、prefix/cache 指标和 trace 基础设施。

这一层已经具备可测试的 AgentLoop 和工具安全边界，但还缺少真正的 step-level execution plan。`TaskPacket` 已经能描述任务，但还没有被转成可恢复、可暂停、可解释的执行步骤状态机。

### `agent-writer-backend`

核心职责是写作域内核：

- `headless.rs`: HeadlessBackend，提供 MCP 调用需要的所有 backend action。
- `writer_agent/kernel/run_loop.rs`: 写作任务 preflight、观察、上下文包、StoryImpact、TaskPacket、AgentLoop 准备与结果记录。
- `writer_agent/context/assembly.in.rs`: 按任务预算组装 Story OS 上下文。
- `writer_agent/provider_budget.rs`: 长任务 provider token/cost 预算和审批报告。
- `writer_agent/memory/*`: canon、promise、chapter mission、style preference、feedback、run events、trace 等写作记忆。
- `chapter_generation/*`: 章节生成、修复、保存、settlement 和质量约束。
- `brain_service/*`: Project Brain 索引、检索、图谱和外部研究入口。

这一层的写作域能力明显强于普通聊天式 agent：它知道章节、任务、伏笔、canon、读者补偿、保存确认和 proposal 生命周期。但执行层仍然偏“准备好上下文后让模型跑一轮工具循环”，没有把复杂写作任务拆成稳定的多步计划。

### `forge-agent-mcp`

核心职责是 MCP 协议边界：

- 提供 `initialize`、`tools/list`、`tools/call`。
- 暴露 `forge_backend_call` 和大量具体 `forge_*` 工具。
- 使用统一 `structuredContent` envelope：`ok/data/error`。
- 给工具标注 read-only、destructive、idempotent、open-world。
- 测试覆盖 backend action 与 specific tool surface 的一致性。

这一层现在比较健康。下一步重点不是增加更多工具，而是加强端到端 smoke、错误 kind 分类和工具 schema 约束质量。

## 性能评估

### 强项

1. Headless-only 后运行面更清晰
   - 删除桌面 runtime 后，构建目标、依赖图和调用路径更短。
   - MCP stdio server 是唯一入口，减少了 renderer 状态、Tauri command 和 backend action 双轨维护成本。

2. 工具安全边界较强
   - ToolRegistry、ToolFilter、PermissionPolicy、requires_approval、side-effect level 已经形成闭环。
   - 写操作、provider call、open-world 工具能被区分。
   - `generate_chapter_draft` 这类写敏感工具默认不会直接暴露给模型自由调用。

3. 写作域上下文能力强
   - ContextPack 已按任务区分 source priority 和 required budget。
   - Story Contract、Chapter Mission、Next Beat、CanonSlice、PromiseSlice、DecisionSlice、ReaderCompensation、StoryImpactRadius 已经进入上下文系统。
   - 对长篇写作来说，这比普通 RAG 更贴近实际创作状态。

4. 写入安全和项目状态保护较好
   - 章节保存有 revision 检查、dirty target 检查、备份和修复路径。
   - proposal、approval、durable save、ledger write 这些概念已经存在。

5. 测试基础扎实
   - 最近一次完整验证通过：`cargo fmt --check`、`cargo check --workspace`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test -p agent-harness-core`、`cargo test -p agent-writer --lib`、`cargo test -p forge-agent-mcp`。
   - 单元测试覆盖了核心上下文、预算、工具、memory、chapter generation 和 MCP surface。

### 弱项

1. AgentLoop 仍是 reactive tool-call loop
   - 当前 loop 是“分类意图 -> 构造工具 -> provider streaming -> 执行 tool call -> 下一轮”。
   - 它还没有显式的计划步骤、步骤成功条件、步骤级恢复和步骤级 trace。
   - 复杂任务失败后，调度器很难知道应该从哪一步继续。

2. Router 偏规则化
   - `classify_intent` 适合 fast path，但对混合中文写作请求、模糊规划请求、修订+诊断组合请求不够稳。
   - 目前缺少 confidence、reason、fallback_rule 和可回放的 routing evidence。

3. Token 和成本估算仍偏静态
   - 估算主要依赖字符数、固定 overhead 和模型名字符串价格规则。
   - 没有把 provider 实际 usage 回写到模型级校准表。
   - `ttft_ms` 目前更接近 provider call duration，不是真正 first-token latency。

4. 上下文质量指标还不够可操作
   - ContextPack 有 source report 和 truncation reason，但还缺少统一质量分：source coverage、grounding quality、required source loss、truncation risk。
   - preflight 能报告 over budget 和 story impact truncation，但还不能明确告诉调度器“缺哪类故事证据会影响任务成功”。

5. 端到端 MCP smoke 不够硬
   - MCP surface 有单元测试，但仍需要启动二进制后真实走 initialize、tools/list、forge_status、forge_ask_agent mock provider、章节读写安全路径。
   - 这会比单测更早发现协议、stdio、环境变量和数据目录问题。

6. 写作质量评测缺口最大
   - 目前测试更偏工程正确性，不够衡量中文长篇质量。
   - 缺少固定 fixture 项目来回归：连续性、人物口吻、伏笔推进、章节任务命中、改写不越界、canon 冲突、重复风险。

## 下一阶段优先级

### P0: 内核遥测与 MCP Smoke

目标：让每次 agent run 都能被复盘，让 MCP 入口能被真实进程级测试覆盖。

任务：

1. 增加 MCP process smoke test
   - 启动 `forge-agent-mcp stdio`。
   - 覆盖 `initialize`、`tools/list`、`forge_status`、`forge_agent_kernel_status`。
   - 使用临时 `FORGE_AGENT_DATA_DIR`，不依赖真实用户数据。

2. 强化错误 envelope
   - 将 backend error 分类为 `backend`、`validation`、`provider`、`budget`、`permission`、`context_overflow`、`storage`。
   - `tool_error_result` 不只返回 `kind=backend`，应尽量保留结构化 kind 和 remediation。

3. 修正 provider latency 指标
   - 区分 `ttft_ms`、`provider_call_duration_ms`、`total_provider_duration_ms`。
   - streaming 回调第一次 TextDelta 时记录真实 TTFT。

4. 增强 run trace 关键事件
   - 记录 intent、tool inventory 摘要、provider guard 输入、context window guard 决策、usage、retry/compaction 原因。
   - trace 应能解释“为什么允许或阻止这次 provider call / tool call”。

验收：

```powershell
cargo fmt --check
cargo check --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p agent-harness-core
cargo test -p agent-writer --lib
cargo test -p forge-agent-mcp
```

### P1: Planner-Aware AgentLoop

目标：把 `TaskPacket` 从“任务说明/trace 结构”升级为“可执行计划”的输入。

任务：

1. 新增 `ExecutionPlan`
   - 从 TaskPacket 派生 step 列表。
   - 每个 step 声明目标、所需上下文、允许工具、最大副作用等级、成功信号、失败恢复建议。

2. AgentLoop 支持 step state
   - 事件里增加 `plan_started`、`step_started`、`step_completed`、`step_failed`、`plan_completed`。
   - 每轮 provider/tool call 归属到当前 step。

3. 写作任务使用计划
   - `ManualRequest` 可以是 read-context -> answer。
   - `PlanningReview` 可以是 gather -> diagnose -> plan artifact。
   - `ContinuityDiagnostic` 可以是 gather -> conflict scan -> diagnostic artifact。
   - `ChapterGeneration` 可以是 preflight -> draft -> validate -> save/settlement。

验收：

- `forge_trace` 能展示当前 run 的计划步骤和每步状态。
- max rounds 失败时能指出卡在哪一步。
- 计划步骤能约束工具暴露范围。

### P2: 上下文质量与预算校准

目标：让上下文和预算从“能用”变成“可解释、可校准、可优化”。

任务：

1. ContextPack 质量评分
   - 增加 `ContextQualityReport`。
   - 指标包含 required source coverage、truncation risk、story grounding quality、missing evidence、source diversity。
   - preflight 输出 top risks 和 next actions。

2. Provider usage 校准
   - 记录估算 token 与 provider usage 的差异。
   - 按 provider/model 维护 tokens-per-char、tool schema overhead、system overhead 的滚动校准值。
   - 预算审批使用校准后的估算。

3. Prompt cache / Context Spine 指标
   - 将 stable prefix、dynamic context、tool schema、user instruction 分段记录 churn。
   - trace 输出 cache hit/miss 原因。

4. 只读上下文检索并行化
   - Project Brain、memory ledgers、outline、chapter context 等只读来源可并行准备。
   - 写操作和 durable save 仍保持串行。

验收：

- preflight 能解释“为什么这次上下文质量不足”。
- 常见中文长上下文 token 估算误差可持续下降。
- trace 能看到各上下文来源耗时和预算消耗。

### P3: 写作质量评测集

目标：让内核能力提升有可重复证据，而不是靠主观感觉。

任务：

1. 建立 fixture 项目
   - 小型中文长篇项目，包含章节、outline、lorebook、canon、promises、reader compensation。
   - 不依赖真实用户数据。

2. 建立评测任务
   - ContinuityDiagnostic：检测 canon 冲突、未来信息泄漏、死角承诺。
   - PlanningReview：生成章节计划并覆盖任务约束。
   - ManualRequest：回答必须引用正确上下文，不编造。
   - ChapterGeneration：命中 mission、推进 promise、保持口吻、长度合规。

3. 建立自动评分
   - 先用规则评分：mission hit、canon conflict、length compliance、required evidence used。
   - 再加可选 LLM judge，但 judge 结果必须保存 evidence。

验收：

- 核心 prompt、context 策略、router、planner 改动都能跑回归评测。
- 每次评测输出 JSONL，能比较前后版本质量差异。

### P4: 恢复策略和长任务鲁棒性

目标：长任务失败后能继续，不需要完全重跑。

任务：

1. 标准化恢复动作
   - provider timeout、rate limit、JSON parse failure、tool denial、context overflow、revision mismatch 都要有 recovery action。

2. run-level recovery summary
   - max rounds、doom loop 或 provider 失败时，输出已完成步骤、卡点、可重试参数、建议缩小范围。

3. 长任务 checkpoint
   - ChapterGeneration、BatchGeneration、SupervisedSprint 记录阶段性 checkpoint。
   - 失败后可以从最近安全点恢复。

验收：

- 失败响应不只是字符串错误，而是结构化 failure bundle。
- 调度器能根据 failure bundle 决定 retry、ask approval、shrink context 或 stop。

## 建议改动入口

- `agent-harness-core/src/agent_loop.rs`: step-level plan、真实 TTFT、provider/tool telemetry。
- `agent-harness-core/src/task_packet.rs`: ExecutionPlan 输入结构和 success criteria。
- `agent-harness-core/src/router.rs`: intent confidence、reason、fallback rule。
- `agent-harness-core/src/tool_registry/defaults.in.rs`: plan step 对工具过滤的映射。
- `agent-writer-backend/src/writer_agent/kernel/run_loop.rs`: 写作任务计划化、preflight 与 plan 衔接。
- `agent-writer-backend/src/writer_agent/run_preflight.rs`: ContextQualityReport 和 failure bundle。
- `agent-writer-backend/src/writer_agent/context/assembly.in.rs`: source coverage、truncation risk、source timing。
- `agent-writer-backend/src/writer_agent/provider_budget.rs`: usage 校准和模型级预算。
- `agent-writer-backend/src/headless.rs`: backend action 错误 kind、MCP-facing report。
- `forge-agent-mcp/src/main.rs`: structured error envelope、process smoke、tool schema 一致性。

## 当前不建议做的事

1. 不恢复 Tauri 或桌面双模式。
   - 当前项目定位已经清晰：Headless MCP 后端。
   - 恢复桌面 runtime 会重新引入双入口维护成本。

2. 不急着增加更多 MCP 工具。
   - 当前工具面已经很大。
   - 优先提升工具质量、错误分类、计划约束和评测覆盖。

3. 不把 router 全部交给 LLM。
   - 规则 fast path 便宜、稳定、可测。
   - LLM router 应作为低置信度或混合意图时的 fallback。

4. 不并行化写操作。
   - 只读上下文检索可以并行。
   - write、approval、durable save、revision-sensitive 操作必须保持顺序。

## 推荐里程碑

### Milestone 1: Observability Baseline

范围：P0。

交付物：

- MCP process smoke test。
- structured error kind。
- 真实 TTFT / provider duration。
- trace 覆盖 provider guard、tool inventory、context window guard。

### Milestone 2: Executable Plan

范围：P1。

交付物：

- ExecutionPlan。
- step-level AgentLoop event。
- 写作任务的 plan templates。
- max-round failure recovery summary。

### Milestone 3: Context and Budget Calibration

范围：P2。

交付物：

- ContextQualityReport。
- provider usage calibration。
- context source timing。
- read-only retrieval parallelism。

### Milestone 4: Writing Eval Harness

范围：P3 和 P4 核心项。

交付物：

- 中文长篇 fixture 项目。
- JSONL eval runner。
- 自动评分报告。
- failure bundle 和 checkpoint recovery。

## 下一步

优先做 Milestone 1。原因很简单：当前内核已经有较多写作域逻辑，再继续加智能能力前，必须先把真实运行链路、错误分类和遥测打稳。随后做 ExecutionPlan，把“强上下文 + 强工具”变成“可恢复、可解释、可调度的执行过程”。最后再用固定中文写作评测集约束质量提升。

## ForClaw 写作赋能引擎计划

### 核心判断

当前项目已经具备较强的“防写错”能力：章节生成会组装上下文、尊重 canon、promise、chapter mission、字数合同、revision save 和 settlement；但“怎么写得更好”的能力还没有被产品化为稳定模块。

ForClaw 写作赋能引擎的目标不是继续堆工具，而是把写作智慧变成可选择、可注入、可诊断、可修订、可学习、可评测的闭环。

关键结论：

- “把写作智慧直接编进 prompt”是正确方向，符合当前 `generate_chapter_draft` 的真实调用路径。
- 但不能只做一次调用的超长提示词，否则很容易变成漂亮但不可验证的 prompt 包装。
- 最稳路线是：`Craft Library + Story Context -> Prompt Compiler -> Draft -> Quality Diagnosis -> Targeted Revision -> Feedback Memory -> Eval Harness`。

### 合并架构

推荐实现为七层，而不是固定四层线性链路。

```text
Layer 1: Craft Library
写作技法、类型技法、反模式、好例片段、适用条件、评分维度

Layer 2: Story Context
书态、设定集、角色卡、前文摘要、伏笔状态、章节任务、作者风格

Layer 3: Empowerment Prompt Compiler
根据故事上下文选择技法，不全量注入
产出：场景工艺计划、本章写作纪律、禁止事项、修订目标

Layer 4: Model Drafting
快写模式：一次生成
质量模式：计划 -> 正文

Layer 5: Quality Diagnosis
规则评分 + 可选 LLM judge
只基于证据指出问题，不硬批、不编造

Layer 6: Targeted Revision
只修低分项，不无差别重写

Layer 7: Feedback Memory / Eval Harness
把作者接受、拒绝和评测结果沉淀成项目写法记忆
```

### 与四层赋能架构的关系

原始四层架构：

```text
Craft Library -> Empowerment Prompt Engine -> Story Context -> Model Call
```

保留其核心思想，但调整实现顺序：

```text
Story Context + Craft Library -> Empowerment Prompt Compiler -> Model Call
```

原因：

- 技法选择必须依赖章节任务、人物状态、冲突类型、伏笔密度和作者风格。
- 同一种技法在不同场景下作用不同。例如“对话技法”在审讯、告白、交易、决裂、战前沉默中应注入不同规则。
- 如果先编 prompt 再塞上下文，技法库越大，模型越容易被写作教材淹没，反而忽略本章目标。

### 至高纪律

写作赋能层必须遵守三条红线：

1. 证据至上。
   - 质量批判、案例、建议必须来自项目上下文、真实输出、可验证规则或严密逻辑。
   - 不为显得专业而编造不存在的问题。

2. 没大问题绝不硬批。
   - 诊断没有发现实质问题时，明确输出“未发现致命弱点”。
   - 可以指出证据不足、目标不清、数据缺口，但不强行制造危机。

3. 一切服务最终效果。
   - 赋能层的价值不是输出漂亮分析，而是提高章节正文质量、连续性、读者期待和作者风格一致性。
   - 对质量无帮助的长篇说教不进入生成 prompt。

### 关键模块设计

#### 1. Craft Library

新增或集中维护结构化写作技法库。

每条技法建议包含：

```text
id: dialogue_subtext
name: 对话潜台词
category: dialogue
applies_when: 审讯 / 交易 / 隐瞒 / 关系试探
instruction: 对话必须改变权力、关系、信息或选择，不能只是解释设定
anti_patterns:
  - 角色轮流讲背景资料
  - 对话没有改变局面
diagnostic_signals:
  - dialogue_function
  - exposition_ratio
  - relationship_shift
revision_hint: 将直白说明改成试探、回避、误导或带代价的承认
```

初期不需要大而全，先覆盖最影响长篇质量的技法：

- 场景目标：每场戏必须有即时目标和阻力。
- 冲突压力：冲突必须改变选择、代价或信息。
- 对话功能：对话要推进权力、关系、隐瞒或决策。
- 设定入戏：设定通过行动、误解、代价和后果进入正文。
- 情绪外化：少直接命名情绪，多用动作、停顿、身体反应和环境选择。
- 伏笔推进：伏笔要被推进、误导、兑现或明确延后。
- 章末钩子：结尾要有已发生后果和未解决问题。
- 类型快感：按类型提供读者期待，例如悬疑的信息差、仙侠的代价与境界压力、言情的关系位移。

#### 2. SceneCraftPlan

扩展当前 `ScenePlanEntry`，不要只记录 objective。

建议字段：

```text
scene_id
objective
participants
conflict_pressure
character_choice
information_release
withheld_information
emotional_curve
promise_or_anchor_payoff
ending_hook
craft_rules
must_avoid
```

用途：

- 作为章节生成前的中间计划。
- 作为 prompt compiler 的输入。
- 作为生成后质量诊断的对照物。
- 作为 runtime artifact 持久化，便于复盘。

#### 3. Empowerment Prompt Compiler

不要把 Craft Library 全量塞进 prompt，而是做选择。

输入：

- `BuiltChapterContext`
- `ChapterMission`
- `StoryContract`
- `PromiseLedger`
- `AuthorVoiceSnapshot`
- `SceneCraftPlan`
- 作者显式指令

输出：

```text
本章写作纪律
选中的技法规则
本章禁忌
质量自检表
必要锚点和伏笔承载要求
```

选择原则：

- 每次最多注入 5 到 8 条高相关技法。
- 技法必须绑定本章目标或上下文证据。
- 不注入泛泛而谈的写作教材。
- 对 token 成本敏感，优先短规则和项目特异性规则。

#### 4. Quality Diagnosis

章节草稿生成后增加质量诊断。

初期可先做启发式评分：

```text
mission_hit
scene_causality
character_choice
conflict_pressure
dialogue_function
exposition_ratio
anchor_carry
promise_progress
ending_hook
style_drift
length_compliance
canon_risk
```

已有基础可复用：

- `writer_agent/anchor_carry.rs`: 锚点是否只是提到，还是通过行动、对话、后果、兑现压力参与场景。
- `writer_agent/author_voice.rs`: 作者风格快照与 style drift。
- `chapter_generation` 的 contract validation: 字数和保存边界。

诊断输出必须带证据：

```text
score
severity
evidence_excerpt
reason
revision_hint
```

禁止输出无证据的“文笔不够好”“节奏差”“人物扁平”等空泛判断。

#### 5. Targeted Revision

低分项触发定向修订，不默认重写全章。

修订策略：

- `exposition_ratio` 高：把解释改成行动、阻碍、误解、代价。
- `dialogue_function` 低：让对话改变权力、关系、信息或选择。
- `anchor_carry` 低：让关键锚点参与行动或后果，而不是只被提名。
- `ending_hook` 弱：补充已发生后果和未解决问题。
- `style_drift` 高：按作者风格快照压回句式、语气和叙述密度。

定向修订必须保留：

- canon
- chapter mission
- promise 状态
- 已通过的强项
- 字数合同
- revision save 安全边界

#### 6. Feedback Memory

把作者反馈沉淀为“写法记忆”，而不是只存普通 style preference。

建议新增或扩展三类记忆：

```text
CraftRuleMemory
- key
- rule
- scope
- accepted_count
- rejected_count
- evidence_refs

GoodExampleMemory
- excerpt_ref
- reason
- reusable_pattern
- applicable_scene_types

BadPatternMemory
- pattern
- correction
- rejected_count
- evidence_refs
```

这些记忆进入后续 prompt compiler 的技法选择，而不是直接无脑拼接进 prompt。

### 实施里程碑

#### Milestone A: Prompt Empowerment Baseline

目标：先把写作工艺规则以最小成本接入当前章节生成。

任务：

- 新增 Craft Library 的静态配置或 Rust 内置规则。
- 在 `build_chapter_context` 的 writing quality enrichment 后增加 craft rule selection。
- 在 `generate_chapter_draft` 的 system prompt 中加入短版写作工艺纪律。
- 保持一次调用模式，不改保存链路。

验收：

- 章节 prompt 中能看到与本章目标相关的技法规则。
- 不超过 5 到 8 条技法注入。
- 现有章节生成测试不回退。

#### Milestone B: SceneCraftPlan

目标：让模型先知道一场戏怎么成立，再写正文。

任务：

- 扩展 `ScenePlanEntry`。
- 根据 outline、chapter mission、promise、角色状态生成 `SceneCraftPlan`。
- 将 scene plan 写入 runtime artifacts。
- prompt compiler 使用 scene plan 选择技法。

验收：

- 每次章节生成都有可复盘的 scene craft artifact。
- 草稿能被诊断为是否命中 scene plan。

#### Milestone C: Quality Diagnosis

目标：写完后知道具体哪里好、哪里不好。

任务：

- 新增 `ChapterQualityReport`。
- 接入 `anchor_carry`、`style_drift`、length validation。
- 增加简单启发式：角色选择、冲突压力、对话功能、解释比例、结尾钩子。
- 把诊断结果写入 runtime artifacts 和 generation event。

验收：

- 每章生成后输出结构化质量报告。
- 所有批判必须带 evidence excerpt 或明确规则来源。
- 未发现实质问题时允许输出“未发现致命弱点”。

#### Milestone D: Targeted Revision

目标：只修真正影响质量的低分项。

任务：

- 新增 `revise_chapter_draft_for_quality`。
- 根据 `ChapterQualityReport` 选择最多 3 个修订目标。
- 修订后重新跑质量诊断和字数校验。
- 避免多轮无意义重写，最多一次自动修订。

验收：

- 低分项可触发定向修订。
- 修订不会破坏已通过的 canon、promise、mission 和字数合同。
- 修订前后质量报告可比较。

#### Milestone E: Feedback Memory and Eval Harness

目标：让系统越写越懂项目，而不是每章从零开始。

任务：

- 把作者接受、拒绝、手动修订沉淀为 CraftRuleMemory、GoodExampleMemory、BadPatternMemory。
- 建立中文长篇 fixture 项目。
- 建立 JSONL eval runner。
- 对比 prompt、context、craft rule 和 revision 改动前后的质量指标。

验收：

- 固定评测集能衡量章节质量变化。
- 作者反馈能改变后续技法选择。
- 质量提升有数据，不只靠主观感觉。

### 建议代码入口

- `agent-writer-backend/src/chapter_generation/context.in.rs`
  - 接入 Craft Library selection。
  - 生成 SceneCraftPlan。
  - 将工艺规则加入 prompt context。

- `agent-writer-backend/src/chapter_generation/types_and_utils.in.rs`
  - 定义 `SceneCraftPlan`、`ChapterQualityReport`、质量评分结构。

- `agent-writer-backend/src/chapter_generation/draft_and_save.in.rs`
  - 扩展章节生成 system prompt。
  - 增加定向修订模型调用。

- `agent-writer-backend/src/chapter_generation/pipeline/main.in.rs`
  - 在 draft 后插入 quality diagnosis。
  - 在必要时插入 targeted revision。
  - 将质量报告写入 events 和 artifacts。

- `agent-writer-backend/src/writer_agent/anchor_carry.rs`
  - 继续作为锚点承载评分基础。

- `agent-writer-backend/src/writer_agent/author_voice.rs`
  - 继续作为风格漂移诊断基础。

- `agent-writer-backend/src/writer_agent/memory/*`
  - 增加或扩展写法记忆。

- `config/llm-request-profiles.json`
  - 增加 quality diagnosis 和 targeted revision profile。

### 当前不建议

1. 不要一开始做庞大的写作技法大全。
   - 先覆盖最影响章节质量的 8 到 12 条核心规则。

2. 不要把所有技法都注入 prompt。
   - 必须按章节目标和上下文证据选择。

3. 不要把“一次调用”定义为最终质量模式。
   - 一次调用适合 fast draft；高质量模式必须允许诊断和定向修订。

4. 不要让 LLM judge 成为唯一质量来源。
   - 先用规则和证据约束，再加可选 judge。

5. 不要硬批。
   - 质量诊断没有证据时必须降级为“证据不足”或“未发现致命弱点”。

### 推荐下一步

优先做 Milestone A 和 Milestone B。

原因：当前代码已经有 prompt 注入、scene_plan artifact、anchor_carry、style_drift 和章节生成 pipeline。先把 Craft Library 和 SceneCraftPlan 接进去，成本低、收益直接，也不会破坏现有保存安全链路。

完成后再做 Quality Diagnosis 和 Targeted Revision，形成真正闭环。最后用中文 fixture 和 JSONL eval runner 证明质量变化。

## ForClaw 配套设施升级方案

### 当前能力成熟度

| 层级 | 已有能力 | 成熟度 |
| --- | --- | --- |
| 基础设施 | Rust MCP server、stdio 协议、存储抽象 | 高 |
| 项目管理 | 章节/大纲/卷宗/设定集/书态管理 | 高 |
| 生成流水线 | 上下文组装 -> 预算校验 -> 草稿 -> 修复 -> 落盘 | 中高 |
| 智能体状态 | 故事账本、提案队列、债务追踪、轨迹导出 | 中 |
| 知识图谱 | Project Brain 索引、跨引用、源版本对比 | 中 |
| 冲刺工具 | 监督式写作冲刺、预算记账、检查点 | 中 |

核心判断：

当前底座已经够用，不需要重建基础设施。系统已经不只是“结构化文件管理 + LLM 调用管道”：它已经具备 story ledger、proposal、promise、reader debt、Project Brain、sprint checkpoint 等写作状态设施。

更准确的问题是：写作工程底座和故事状态管理已经成型，但写作技法、质量判断、修订学习还没有成为一等公民。下一步应该补“写作智能侧车”，不要推翻现有高成熟度模块。

### 升级原则

1. 不重建底座。
   - MCP server、存储、章节管理、预算、落盘安全继续沿用。
   - 新增写作智能设施应挂在章节生成管线和 writer memory 旁边。

2. 不把技法库变成长 prompt。
   - 技法必须经过选择、压缩、证据绑定后再进入 prompt。
   - 每次生成只注入少量高相关技法。

3. 不让质量判断停留在主观评价。
   - 诊断必须结构化，包含 score、evidence、reason、revision_hint。
   - 没有证据时输出“证据不足”或“未发现致命弱点”，不硬批。

4. 不破坏安全落盘链路。
   - craft plan、quality report、revision report 都是生成前后的辅助 artifact。
   - 最终保存仍必须走 revision check、contract validation 和 save conflict 逻辑。

### 配套设施总览

```text
现有章节生成主链路：
context -> budget -> draft -> repair -> save -> settlement

新增写作智能侧车：
craft library
-> craft selection
-> scene craft plan
-> empowerment prompt packet
-> quality report
-> targeted revision
-> craft memory update
-> eval harness
```

侧车只增强写作质量，不替代现有工程安全边界。

### 设施 1: 写作智能数据层

目标：让“写法”成为可存储、可选择、可评估的结构化数据。

新增设施：

- `config/craft-library.json`
  - 存放内置技法、反模式、适用条件、诊断指标和修订建议。

- memory 表：
  - `craft_rules`
  - `craft_examples`
  - `craft_bad_patterns`
  - `craft_feedback_events`

- Rust 类型：
  - `CraftRule`
  - `CraftRuleSelection`
  - `CraftMemorySignal`
  - `CraftExample`
  - `CraftBadPattern`

建议数据结构：

```text
CraftRule
- id
- category
- name
- applies_when
- instruction
- anti_patterns
- diagnostic_signals
- revision_hint
- token_cost_hint

CraftRuleSelection
- rule_id
- reason
- evidence_refs
- priority
- prompt_text

CraftMemorySignal
- source
- action
- rule_id
- accepted
- rejected
- evidence_ref
- created_at
```

成熟度目标：

- 当前：低。
- Milestone A 后：中。
- 接入反馈学习后：中高。

### 设施 2: Empowerment Prompt Compiler

目标：把 Story Context 和 Craft Library 编译成短、准、可追溯的 prompt packet。

输入：

- `BuiltChapterContext`
- chapter mission
- story contract
- promise ledger
- author voice snapshot
- scene craft plan
- craft memory signals
- 作者显式指令

输出：

```text
EmpowermentPromptPacket
- craft_rules
- craft_rule_reasons
- evidence_refs
- chapter_discipline
- must_avoid
- self_checklist
- token_estimate
```

实现建议：

- 不放进 `build_chapter_context` 主函数里继续膨胀，建议独立模块：
  - `agent-writer-backend/src/chapter_generation/craft_prompt.rs`
  - 或 `agent-writer-backend/src/writer_agent/craft/`

- compiler 必须可单元测试：
  - 给定章节目标和 promise 状态，应选中哪些规则。
  - 给定无关技法，应不会注入。
  - 给定 token budget，应能降级为短版规则。

成熟度目标：

- 当前：低。
- 先做到中高，因为它是写作赋能的主入口。

### 设施 3: SceneCraftPlan

目标：让模型先知道“一场戏怎么成立”，再开始写正文。

当前 `ScenePlanEntry` 只有薄字段，需要升级为写前计划 artifact。

建议字段：

```text
SceneCraftPlan
- scene_id
- objective
- participants
- conflict_pressure
- character_choice
- information_release
- withheld_information
- emotional_curve
- promise_or_anchor_payoff
- ending_hook
- selected_craft_rules
- must_avoid
- evidence_refs
```

生成方式：

- 初期用规则从 outline、target beat、chapter mission、open promises、角色状态推导。
- 后续可加 LLM plan step，但计划输出必须结构化并可保存。

接入位置：

- context built 后，draft 前。
- runtime artifacts 中持久化。
- quality diagnosis 用它做对照。

成熟度目标：

- 当前：低。
- Milestone B 后：中。

### 设施 4: ChapterQualityReport

目标：写完后知道具体哪里好、哪里不好，并且所有判断有证据。

初期指标：

```text
mission_hit
scene_causality
character_choice
conflict_pressure
dialogue_function
exposition_ratio
anchor_carry
promise_progress
ending_hook
style_drift
length_compliance
canon_risk
```

输出结构：

```text
QualityMetricResult
- metric
- score
- severity
- evidence_excerpt
- rule_source
- reason
- revision_hint

ChapterQualityReport
- chapter_title
- overall_score
- fatal_issues
- major_issues
- metric_results
- top_revision_targets
- no_major_issue
```

实现策略：

- 先复用已有确定性能力：
  - `anchor_carry` 判断锚点是否真正参与行动、对话、后果或兑现压力。
  - `author_voice` 判断 style drift。
  - chapter contract 判断字数与保存边界。

- 再补简单启发式：
  - 对话功能：含对话但缺少决定、拒绝、承认、隐瞒、威胁、交换等信号时降分。
  - 解释比例：连续说明性段落过长时降分。
  - 结尾钩子：末段没有后果、选择、风险、未解问题时降分。
  - 角色选择：正文中没有选择、拒绝、放弃、承担、交易等动作信号时降分。

- 最后加可选 LLM judge：
  - judge 只能补充判断，不作为唯一来源。
  - judge 输出必须引用草稿证据。

成熟度目标：

- 当前：低。
- 复用现有启发式后：中。
- 加 fixture eval 后：中高。

### 设施 5: Targeted Revision

目标：只修真正影响质量的低分项，不无差别重写。

流程：

```text
draft
-> quality_report
-> select_top_issues(max=3)
-> revise_chapter_draft_for_quality
-> quality_report_after
-> length validation
-> save
```

修订规则：

- 最多一次自动修订。
- 最多处理 3 个问题。
- 保留已通过强项。
- 不改变 chapter mission、canon、promise 状态。
- 不绕过字数合同和 save conflict。

新增 profile：

- `quality_diagnosis`
- `chapter_targeted_revision`

建议加入 `config/llm-request-profiles.json`，便于独立控制温度、max tokens 和 reasoning。

成熟度目标：

- 当前：低。
- Milestone D 后：中。

### 设施 6: 事件与 Artifact 扩展

目标：让每次章节生成可复盘。

现有 `ChapterGenerationEvent` 已经能携带 context、scene_plan、settlement、length telemetry。需要扩展：

```text
craft_selection
empowerment_prompt_packet_summary
quality_report
revision_report
quality_before_after
craft_memory_updates
```

新增 runtime artifacts：

```text
{request_id}.craft_selection.json
{request_id}.scene_craft_plan.json
{request_id}.quality_report.before.json
{request_id}.quality_report.after.json
{request_id}.revision_report.json
```

价值：

- 可以解释“为什么这章注入这些技法”。
- 可以比较修订前后是否真的更好。
- 可以为后续 eval harness 提供数据。

成熟度目标：

- 当前：中。
- 扩展后：高。

### 设施 7: Sprint Quality Gate

目标：把监督式写作冲刺从“批量推进章节”升级为“按质量门槛推进章节”。

现有 sprint 已具备：

- 章节队列。
- pause/resume/cancel。
- checkpoint。
- budget ceiling。
- provider budget 记账。

建议新增：

```text
SprintQualityGate
- minimum_quality_score
- required_metrics
- allow_auto_revision
- stop_on_canon_risk
- stop_on_style_drift_high
- stop_on_unresolved_save_conflict
```

冲刺推进规则：

- 如果 quality report 低于门槛，当前章节进入 `needs_revision`。
- 如果允许自动修订，执行一次 targeted revision。
- 如果修订后仍失败，暂停 sprint 并输出证据。
- 如果 canon risk 或 save conflict 高，直接暂停，不自动推进。

成熟度目标：

- 当前：中。
- 加 quality gate 后：中高。

### 设施 8: Eval Harness

目标：证明写作赋能是否真的提高质量。

新增内容：

- `fixtures/writing_eval/`
  - 小型中文长篇项目。
  - outline、lorebook、chapters、memory seed。

- `scripts/run-writing-eval.*`
  - 运行固定任务。
  - 输出 JSONL。

- eval 输出：

```text
run_id
git_revision
model
task
prompt_profile
quality_report
length_report
anchor_carry_report
style_drift_report
revision_applied
before_after_delta
```

评测任务：

- chapter generation。
- continuity diagnostic。
- planning review。
- targeted revision。

成熟度目标：

- 当前：低。
- 有固定 fixture 后：中。
- 能做 before/after 对比后：中高。

### 分阶段实施路线

#### Phase 0: 数据结构和边界

目标：先定义结构，不急着让模型多跑。

任务：

- 定义 CraftRule、CraftRuleSelection、EmpowermentPromptPacket。
- 定义 SceneCraftPlan。
- 定义 ChapterQualityReport。
- 定义 RevisionReport。
- 扩展 ChapterGenerationEvent 字段。

验收：

- 类型可序列化。
- runtime artifacts 路径明确。
- 不改变现有章节生成行为。

#### Phase 1: Craft Prompt Baseline

目标：让章节生成真正带上少量写作工艺。

任务：

- 新增 `config/craft-library.json`。
- 实现 craft rule selection。
- 实现 prompt compiler。
- 在 draft prompt 注入短版 craft discipline。

验收：

- 每次生成最多注入 5 到 8 条技法。
- 每条技法有 reason 和 evidence_refs。
- 原有生成、保存、预算链路不变。

#### Phase 2: SceneCraftPlan Artifact

目标：让写前计划可复盘。

任务：

- 从 target beat、mission、promise、角色状态生成 SceneCraftPlan。
- 写入 runtime artifact。
- draft prompt 使用 scene craft plan。

验收：

- 每次生成都有 scene craft plan。
- quality report 能引用 scene craft plan 作为目标证据。

#### Phase 3: Quality Report

目标：让质量判断结构化。

任务：

- 接入 anchor carry、style drift、length compliance。
- 实现 mission hit、dialogue function、exposition ratio、ending hook 等启发式。
- 输出 before quality report。

验收：

- 每个低分项都有 evidence_excerpt。
- 没有证据时不硬批。
- report 能被前端或 MCP caller 消费。

#### Phase 4: Targeted Revision

目标：用质量报告驱动一次定向修订。

任务：

- 实现 revise_chapter_draft_for_quality。
- 只传 top 3 issues。
- 修订后重新跑 quality report。
- 重新跑 length validation。

验收：

- 修订前后可比较。
- 自动修订最多一次。
- 保存安全链路不变。

#### Phase 5: Craft Memory and Sprint Quality Gate

目标：把反馈和冲刺纳入写作智能闭环。

任务：

- 新增 craft memory 表和方法。
- 作者接受/拒绝、手动修订写入 craft feedback。
- sprint 增加 quality gate。
- 低质量章节阻止 sprint 自动推进。

验收：

- 作者反馈能影响下一次 craft selection。
- sprint 能因质量门槛暂停并给出证据。

#### Phase 6: Writing Eval Harness

目标：让质量提升可回归。

任务：

- 建立 fixture 项目。
- 建立 eval runner。
- 输出 JSONL。
- 比较 prompt/compiler/revision 改动前后的质量指标。

验收：

- 改动前后有可比较报告。
- 质量指标不再完全依赖主观判断。

### 建议优先级

P0：

- CraftRule / CraftSelection / PromptCompiler。
- SceneCraftPlan 类型和 artifact。
- 不改变保存链路。

P1：

- ChapterQualityReport。
- 复用 anchor_carry、style_drift、length validation。
- 增加 generation event 和 runtime artifact。

P2：

- Targeted Revision。
- quality_diagnosis 和 chapter_targeted_revision profile。
- before/after report。

P3：

- Craft Memory。
- Sprint Quality Gate。

P4：

- Writing Eval Harness。

### 不建议事项

1. 不要把写作智能做成新主链路。
   - 它应作为侧车增强当前章节生成管线，避免破坏稳定性。

2. 不要先做大规模技法库。
   - 初期 8 到 12 条核心技法足够验证方向。

3. 不要为了质量诊断引入大量 provider 调用。
   - 先规则评分，再可选 judge。

4. 不要让冲刺工具默认自动越过质量门槛。
   - 低质量、canon risk、save conflict 应暂停并要求复核。

5. 不要把作者反馈只写成 style preference。
   - 接受/拒绝背后对应的是 craft rule、example 和 bad pattern，应进入专门写法记忆。

### 最小可行落地方案

第一版只做四件事：

1. 新增 `config/craft-library.json`，内置 8 到 12 条核心技法。
2. 新增 Prompt Compiler，每章选择最多 5 条技法并注入 draft prompt。
3. 扩展 SceneCraftPlan，并作为 runtime artifact 保存。
4. 新增 ChapterQualityReport，先接入 anchor carry、style drift、length compliance 三项。

这四件事完成后，系统就从“能组织上下文调用模型”迈向“能带着写作工艺调用模型，并知道结果是否有效”。

## 2026-05-09 执行完成度重估

### 本轮已补齐的问题

1. Craft Memory 反馈粒度
   - 已从“整章修订成功就接受所有规则”改为按 `CraftRule.diagnostic_signals` 匹配质量指标。
   - `RevisionReport` 会记录 `craft_memory_updates`，包含 rule、matched metrics、before/after score、decision、evidence ref 和 reason。
   - memory 层新增 `craft_feedback_events`，保留每次接受/拒绝的指标证据。

2. Eval runner 证据强度
   - `fixtures/writing_eval/project.json` 从 2 个大纲节点、1 章正文、3 条 lore 扩展为 3 个大纲节点、2 章正文、5 条 lore。
   - `eval_tasks.jsonl` 从 3 个任务扩展为 5 个任务，新增第二章质量评估和第二章生成任务。
   - eval 输出增加 per-metric delta 和 revision target changes，避免只看 overall score。

3. RevisionReport 修订目标映射
   - 新增 `RevisionTargetChange`：metric、revision hint、score before/after、delta、status、before/after evidence、text change summary。
   - 修订被跳过、未尝试、未观察、改善、持平、回退都会进入结构化状态，而不是只记录 accepted bool。

4. Context quality 与 preflight 绑定
   - 章节生成管线中，`ContextQualityRecommendation::Critical` 会阻断生成，`Supplement` 会进入 warning。
   - WriterAgent 通用 preflight 输出 `context_quality` summary，并把 critical/supplement 转成 block/warning 和 next action。

5. 章节质量报告去占位化
   - 新增 `ChapterQualitySignals` 和 `evaluate_chapter_quality_with_signals`，保留旧 `evaluate_chapter_quality` 兼容入口。
   - `anchor_carry` 已调用 `writer_agent/anchor_carry.rs::score_anchor_carry`，按锚点是否参与行动、对话、后果、兑现压力评分。
   - `style_drift` 已调用 `writer_agent/author_voice.rs::compute_style_drift`，按作者风格快照输出漂移证据和扣分。
   - 章节生成管线会从 lore、Project Brain、上下文源和明确故事锚点中提取质量锚点，并从 WriterMemory 构建作者风格快照。

6. RevisionReport 文本变化映射
   - `RevisionTargetChange` 新增 `changed_excerpt_before` / `changed_excerpt_after`。
   - 新增 `build_revision_target_changes_with_text`，能把修订目标映射到修订前后文本片段，而不只比较 metric evidence。

7. Eval runner 继续扩展
   - `eval_tasks.jsonl` 从 5 个任务扩展为 7 个任务。
   - 新增 `quality_signals` 任务，验证 `anchor_carry` / `style_drift` 使用真实输入，不再输出占位式“证据不足”原因。
   - 新增 `targeted_revision` 任务，验证修订报告能记录目标到文本变化的映射。

8. Craft Memory 样本化
   - 复用并补强 `craft_examples` / `craft_bad_patterns` 表，新增结构化写入与查询 API。
   - 章节修订反馈 accepted 时写入 `CraftExampleMemory`，rejected 时写入 `CraftBadPatternMemory`，并把 refs 回填到 `CraftMemoryUpdate`。
   - `eval_tasks.jsonl` 新增 `craft_memory` 任务，验证好例和坏模式能持久化并累计 rejected_count。

9. 作者手动改稿回流
   - 新增 `ManualCraftEditFeedbackRequest` / `ManualCraftEditFeedbackResult`，专门记录作者手动修改前后文本。
   - 新增 `record_manual_craft_edit_feedback`，按 before/after 质量报告生成 `RevisionTargetChange`，只在指标确实改善且存在文本片段映射时写入 Craft Memory。
   - 作者改后的片段写入 `CraftExampleMemory`，被替换掉的 before 片段写入 `CraftBadPatternMemory`，并保留 source ref、metric、score delta 和 excerpt mapping。
   - Headless/MCP 新增 `record_manual_craft_edit_feedback` / `forge_record_manual_craft_edit_feedback`，要求 `authorApproved=true` 才写入。
   - `craft_memory_stats` 已返回近期 examples / badPatterns，不再只给粗粒度接受率。
   - `eval_tasks.jsonl` 新增 `manual_craft_edit` 任务，验证作者手改能产生好例、坏模式和文本映射。

10. MCP smoke 稳定性
   - 进程级 smoke test 的临时数据目录从按 process id 共享，改为按 process id + thread id 隔离。
   - 避免并发 smoke 用同一个 `FORGE_AGENT_DATA_DIR` 互相清理目录导致 EOF 误报。

11. Craft Memory 回流到生成 prompt
   - `EmpowermentPromptPacket` 新增 `memory_examples` / `memory_bad_patterns`，把 Craft Memory 从统计数据升级为生成时可用的写法样本。
   - 章节 context 构建时从 `craft_examples` / `craft_bad_patterns` 读取每条 craft rule 的近期好例和坏例，写入 `craft_memory_prompt_samples`。
   - draft 阶段改用 `compile_empowerment_prompt_with_memory`，只把当前选中 craft rules 对应的少量作者认可写法和已拒绝写法注入系统 prompt。
   - `format_craft_prompt_section` 新增“项目写法记忆”段落，明确区分“可借鉴的作者认可写法”和“必须避开的已拒绝写法”。
   - `eval_tasks.jsonl` 新增 `craft_memory_prompt` 任务，验证 Craft Memory 好例/坏例确实进入写作提示。

12. Writing Eval 跨运行趋势报告
   - `eval_runner` 在覆盖 `eval_output.jsonl` 前读取上一次输出，生成当前 run 与上一 run 的趋势对比。
   - 新增生成文件 `fixtures/writing_eval/eval_trend.json`，记录 task pass/fail、平均 after score、平均 score delta、metric 均值和回退项。
   - 当 task 从 pass 变 fail，或平均分 / metric 均值下降超过阈值时，runner 将报告 regression 并以失败退出。
   - `.gitignore` 忽略 `eval_trend.json`，避免本地趋势输出污染版本库。

### 当前完成度估算

| 范围 | 完成度 | 依据 |
| --- | ---: | --- |
| Headless MCP 写作后端底座 | 86% | MCP、存储、章节管理、记忆账本、预算、保存安全链路已经稳定；进程级 smoke 已加固临时目录隔离；仍缺部分长任务恢复策略。 |
| ForClaw 写作赋能 MVP | 95% | Craft Library、Prompt Compiler、SceneCraftPlan、ChapterQualityReport、Targeted Revision、RevisionReport、Craft Memory、Eval Harness 均已接入主链路；Craft Memory 已能沉淀自动修订和作者手改样本，并回流进生成 prompt。 |
| 写作质量证据闭环 | 93% | 已有 before/after quality、target changes、文本片段映射、craft memory updates、好例/坏模式记忆、作者手动改稿回流、Craft Memory prompt 注入、10-task eval 和跨运行趋势报告；但 fixture 仍偏小。 |
| Context quality / preflight 可操作性 | 72% | 已能查询、阻断和建议动作，并进入章节生成 warning/block；但 source taxonomy 与 Story OS source 的映射仍偏规则化，缺来源耗时和 provider usage 校准。 |
| plan.md 全量路线 | 67% | ForClaw 侧车核心已成型且质量闭环更实；Planner-Aware AgentLoop、provider usage 校准、read-only 并行检索、长任务 checkpoint recovery 仍未完整完成。 |

### 剩余真实缺口

- `anchor_carry` 和 `style_drift` 已接入真实信号，但锚点抽取仍是保守启发式；下一步应让 Project Brain / Story OS 明确产出“本章必须承载锚点”清单。
- eval fixture 已变强，并已有跨运行趋势报告；但仍只能算小样本回归，下一步至少要覆盖 canon 冲突、计划评审和跨章节伏笔推进。
- Revision target change 已能记录文本片段变化，但还不是严格语义 diff；如果要解释“哪一句为何改成哪一句”，需要引入更稳的句级 diff / 语义对齐。
- Craft Memory 已记录指标级反馈、自动修订样本和作者手动改稿 before/after 样本，并已回流进 prompt；下一步应扩展趋势报告维度，证明这些样本长期提升哪些 craft rule。
- Context quality 已进入 preflight，但还没有 provider usage 校准、source timing 和 read-only retrieval parallelism。

### 本轮验证

```powershell
cargo fmt --check
cargo check --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p agent-writer --lib
cargo test -p agent-writer --test writing_eval_test
cargo test -p forge-agent-mcp
scripts\run-writing-eval.cmd
```

当前 writing eval 结果：10 tasks，10 pass，0 fail。
