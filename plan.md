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

### P0: 内核遥测与 MCP Smoke ⚠️（结构已就绪，缺 provider usage 校准端到端证据）

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

### P1: Planner-Aware AgentLoop ⚠️（ExecutionPlan/step event 已存在，缺真实中断恢复和步骤级工具约束证据）

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

### P2: 上下文质量与预算校准 ⚠️（taxonomy/action code/timing 字段已存在，缺 provider usage 回写和全链路并行检索启用）

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

### P3: 写作质量评测集 ⚠️（fixture/规则评分/趋势对比已存在，缺 LLM judge 辅助和更大负例矩阵）

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

### P4: 恢复策略和长任务鲁棒性 ✅（StepFailureAction/Retry/Skip/Stop 已存在，缺真实长任务中断后 resume 的端到端证据归入 P3）

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

13. Craft Rule 级趋势证据
   - `eval_trend.json` 新增 `craft_rule_trends`，按 ruleId 汇总 Craft Memory 接受/拒绝更新、样本写入、坏模式写入、prompt 注入样本和平均 score delta。
   - 跨运行 delta 新增 `craft_rule_trend_delta`，能看出某条 craft rule 的样本、反例、prompt 回流和平均改善是否增减。
   - 趋势数据直接从 eval 输出中的 `craft_memory_updates`、`examples`、`bad_patterns`、`memory_examples`、`memory_bad_patterns` 提取，避免另造不可追溯统计。

14. Eval fixture 覆盖 canon / planning / promise
   - `project.json` 从 3 个大纲节点扩展为 4 个，并新增 `canon` 与 `promises` 数据，覆盖寒影剑代价、青云宗仙器门规、寒影剑代价伏笔、执事堂审问伏笔。
   - `eval_tasks.jsonl` 从 10 个任务扩展为 13 个，新增 `canon_conflict`、`planning_review`、`promise_progression`。
   - `eval_runner` 新增三类规则评估：检测候选文本是否触发显式 canon forbidden pattern；验证第三章计划是否选中关键 craft rules、承载 open promise 并保留下一章钩子；验证第二章是否实际推进跨章节伏笔。
   - （历史快照）该阶段 writing eval 从 13 tasks 继续扩展到 48 tasks（3 profiles × 16 tasks），后续在 P5-P7 中完成。

15. OpenRouter 真实 provider usage 校准验证
   - 新增 gated 集成测试 `chat_text_usage_updates_budget_calibration`，只在 `FORGE_REAL_API_TESTS=1` 时调用真实 provider。
   - 使用 OpenRouter `https://openrouter.ai/api/v1` + `deepseek/deepseek-v4-flash` 实测通过；真实 usage 回写已覆盖 `prompt_tokens`、`completion_tokens`、`total_tokens`，并让校准置信度从无样本进入 `Low`。具体 token 数会随提示词和模型返回波动，不再作为完成度锚点。
   - 测试会调用 `chat_text_with_usage`，再把真实 usage 写入 `agent_harness_core::record_full_usage`，并验证 `estimate_with_confidence` 从无样本进入 `Low` 置信度。
   - 同轮实测通过 `/models` health check、普通中文 chat、JSON mode、profile smoke（ChapterDraft / GhostPreview / Analysis / ParallelDraft / ManualRewrite / ToolContinuation）。

16. OpenRouter 真实 API 全量写作链路验证
   - `llm_runtime` 对 chat completion 增加最多 3 次保守瞬态重试，线性退避 750ms / 1500ms / 2250ms；覆盖 request error、HTTP 429 / 5xx、以及 HTTP 200 但 body JSON decode 失败；HTTP 4xx 配置/鉴权错误不重试。
   - `real_author_session_three_chapter_smoke` 与 `real_author_session_thirty_chapter_gate` 复用同一套 anchor carry gate 修复逻辑：初稿锚点参与不足时允许一次完整重写，重写后仍按原 gate 判定，不降低阈值。
   - 使用 OpenRouter `deepseek/deepseek-v4-flash` 分拆执行 gated `api_integration_tests`，12 个真实 API 测试均已通过；单命令串行跑全套在 30 分钟超时边界内未稳定完成，因此不能再表述为“单命令全量通过”。
   - 三十章真实写作 gate 通过：`chapters=30`、`avg_chars=2150`、`min_carry_rate=0.60`、`avg_anchor_hit=0.92`；验证报告写入本地 `reports/real_author_session_thirty_chapter_gate.json`。
   - 三章真实 smoke 通过：最近样本 `chapters=3`、`min_carry_rate=0.60`、`avg_chars=720`。

### 当前完成度估算

| 范围 | 完成度 | 依据 |
| --- | ---: | --- |
| Headless MCP 写作后端底座 | 86% | MCP、存储、章节管理、记忆账本、预算、保存安全链路已经稳定；进程级 smoke 已加固临时目录隔离；仍缺部分长任务恢复策略。 |
| ForClaw 写作赋能 MVP | 98% | Craft Library、Prompt Compiler、SceneCraftPlan、ChapterQualityReport、Targeted Revision、RevisionReport、Craft Memory、Eval Harness 均已接入主链路；Craft Memory 已能沉淀自动修订和作者手改样本，回流进生成 prompt，并进入 rule 级趋势证据；eval trend 已暴露为 MCP 只读工具；真实三章/三十章写作 gate 已通过。 |
| 写作质量证据闭环 | 96% | 已有 before/after quality、target changes、句级语义 diff、文本片段映射、craft memory updates、好例/坏模式记忆、作者手动改稿回流、Craft Memory prompt 注入、66-task eval（3 profiles）、跨运行趋势报告和 craft rule 级趋势；fixture 已覆盖 canon 冲突、计划评审、跨章节伏笔推进和第三章负例矩阵；长链路质量报告已输出 `duplicatePreviewGroups`（按 120 字符前缀分组）、`repairRate`（修订未改善比例）、`minChars`/`maxChars`/`avgCarryRate` 及 `qualityWarnings`（fail/warning 分级）；OpenRouter 真实 thirty chapter gate 已产出 `avg_chars=2150`、`min_carry_rate=0.60`、`avg_anchor_hit=0.92` 的长链路证据。 |
| Context quality / preflight 可操作性 | 85% | `ContextSourceReport` 已具备 taxonomy、role、elapsed_ms、retrieval_status（已从全硬编码 "ok" 升级为基于实际数据判定的 "ok"/"not_found"，覆盖 instruction/outline/target_beat/previous/next/existing/lore/rag/profile 九个来源）；`action_codes_for_missing_sources` 已产出 `fetch_project_brain_anchor`、`refresh_prior_chapter_summary`、`reduce_low_value_lore` 等结构化 action code；preflight 已能按 Critical/Supplement 阻断或警告；provider usage 已有 OpenRouter 真实样本回写和真实 API 分拆通过证据。短板是 usage 校准仍需长期样本沉淀，以及 read-only retrieval 并行化只在结构层面就绪、未在所有调用点启用。 |
| plan.md 全量路线 | 88% | P4 Required Anchors ✅、P5 Writing Eval Matrix（66 tasks / 3 profiles）✅、P6 Sentence-Level Diff ✅、P7 Craft Trend CI ✅ 已完成；P8 Provider Telemetry（`LlmUsage` 已含 latency_ms/profile/input_chars/output_chars/repaired；phase timing 7 字段 + provider call count；retry 分类已落地）✅/⚠️，仍缺 p50/p90/p95 分位计算；P9 Read-Only Parallelism（existing+RAG 并行启用；9 来源 elapsed_ms 填充；retrieval_status 数据驱动判定）✅/⚠️，仍缺 source 失败隔离单测；P10 Long-Chain Dedup（scene_repetition 跨章检测 + 四类场景单测；plot_progression + new_information_density）✅/⚠️，仍缺负例 fixture；P11 Story State Delta（required_state_deltas 构建→prompt 注入→covered/weak/missing 分级→Strict gate revision）✅/⚠️，仍缺 delta 链 eval runner 验证；P12 Long-Chain Quality Report（duplicatePreviewGroups/repairRate/minChars/maxChars/avgCarryRate/qualityWarnings + Markdown 输出）✅/⚠️，仍缺 thirty chapter gate 字段输出确认；P13 Quality Mode Layering（Fast/Balanced/Strict + MCP 暴露 + sprint fallback；Fast 跳过 quality_report）✅/⚠️，仍缺各模式 provider 调用上限单测；P0 Provider Calibration 已有真实 OpenRouter usage 回写、真实 API 12 项分拆通过和三十章写作 gate 验证 ✅/⚠️；P1 Planner-Aware AgentLoop（`ExecutionPlan`/`compile_plan`/step event 已存在，缺真实中断恢复和步骤级工具约束证据）⚠️、P2 Context Quality（taxonomy/action code/timing/retrieval_status 字段和 provider usage smoke 已存在，缺全链路并行检索启用）⚠️、P3 LongTask Checkpoint Recovery（恢复动作结构已存在，缺真实长任务中断后 resume 的端到端证据）⚠️。 |

### 剩余真实缺口

- `anchor_carry` 和 `style_drift` 已接入真实信号，但锚点抽取仍是保守启发式；下一步应让 Project Brain / Story OS 明确产出“本章必须承载锚点”清单。
- eval fixture 已扩展到 66 tasks，并覆盖 canon 冲突、计划评审、跨章节伏笔推进和第三章负例矩阵，以及 scene_repetition 四类场景（完全重复/近义改写/合法呼应/必要 recap）和 state_delta_coverage 三态分级（covered/weak/missing）的单测；但仍属于规则回归集，不能完全代表长篇真实生成质量，下一步应增加更大负例矩阵（含"锚点承接合格但剧情没有推进"）和 LLM judge 辅助验证。
- Sentence-level semantic diff 已落地（Jaccard 对齐 + confidence 分级），复杂同义替换和语序大调仍可能标为 Low/Unaligned，这是轻量方案的设计权衡。
- Craft Memory 趋势已接入 headless dispatch 和 MCP 只读工具（`forge_eval_trend_summary`），Companion/CI 可直接消费；下一步应增加趋势可视化而非 API 扩展。
- Context quality taxonomy、action code、elapsed_ms、retrieval_status 字段和 preflight 绑定已就绪；provider usage 和真实写作 gate 已通过 OpenRouter 分拆验证，但仍缺长期、多任务、多模型样本沉淀；read-only retrieval 并行化只在结构层面就绪、未在所有调用点启用。

### 本轮验证

```powershell
cargo fmt --check
cargo check --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p agent-writer --lib
cargo test -p agent-writer --test writing_eval_test
cargo test -p forge-agent-mcp
scripts\run-writing-eval.cmd
FORGE_REAL_API_TESTS=1 cargo test -p agent-writer health_check_models_endpoint -- --nocapture
FORGE_REAL_API_TESTS=1 cargo test -p agent-writer chat_text_usage_updates_budget_calibration -- --nocapture
FORGE_REAL_API_TESTS=1 cargo test -p agent-writer chat_text_with_openrouter -- --nocapture
FORGE_REAL_API_TESTS=1 cargo test -p agent-writer chat_json_mode -- --nocapture
FORGE_REAL_API_TESTS=1 cargo test -p agent-writer profile_smoke_feature_text_calls -- --nocapture
FORGE_REAL_API_TESTS=1 cargo test -p agent-writer <api_integration_tests 单项测试名> -- --nocapture
```

当前 writing eval 结果：66 tasks（mystery 21 + scifi 21 + xianxia 24），66 pass，0 fail，无 regression。
当前真实 provider 验证结果：OpenRouter `/models`、中文 chat、JSON mode、streaming、embedding、profile smoke、usage calibration、三章真实写作 smoke、三十章真实写作 gate 均通过；`api_integration_tests` 真实 API 项已分拆验证 12/12 通过，单命令串行全量在 30 分钟超时边界内未稳定完成。usage calibration 置信度进入 Low；三十章 gate 结果为 `avg_chars=2150`、`min_carry_rate=0.60`、`avg_anchor_hit=0.92`。

### 2026-05-09 最新验证补充与计划变化判断

新测试数据要求更新 `plan.md` 的证据口径，但不要求推翻 P8-P13 路线：

- P5 证据增强：writing eval 已从 48 tasks 扩展到 66 tasks，且 66/66 通过；因此所有“48-task eval”只能作为历史快照，当前完成度估算应引用 66-task eval。
- P0 证据边界收紧：真实 API 不是“单命令全量稳定通过”，而是“12 个 gated 项分拆通过；单命令串行全量存在 30 分钟超时风险”。这说明链路能力通过，性能/编排仍需 P8/P13 处理。
- P8 状态变化：`llm_runtime` 的瞬态重试已完成一部分（最多 3 次、线性退避），P8 剩余重点应改为结构化 latency、call count、retry count、phase timing 和报告汇总。
- P10/P12 优先级不下降：最新三十章 gate 通过锚点承接，但报告 preview 仍能观察到重复开场和场景停滞，因此长篇推进去重与长链路质量报告仍是下一轮硬缺口。

## 2026-05-09 全量路线 69% 到 80% 提升计划（历史阶段）

### 目标边界

该段是上一阶段路线计划，当前已被 66-task eval、真实 API 分拆验证和三十章 gate 证据覆盖；保留为执行记录，不再作为当前完成度估算依据。当前完成度以本文件上方“当前完成度估算”和后续 P8-P13 为准。

本计划只针对当时“全量路线完成度”从 69% 推到约 80% 的真实缺口，不重复 ForClaw MVP 已完成内容。判断依据来自当前仓库中已经存在的模块与记录：`agent-harness-core/src/budget_calibration.rs`、`agent-harness-core/src/agent_loop.rs`、`agent-harness-core/src/execution_plan.rs`、`agent-harness-core/src/context_quality.rs`、`agent-writer-backend/src/writer_agent/provider_budget.rs`、`agent-writer-backend/src/writer_agent/kernel/run_loop.rs`、`agent-writer-backend/src/writer_agent/run_preflight.rs`、`agent-writer-backend/src/bin/eval_runner.rs` 和 `fixtures/writing_eval/*`。

当时完成后预期：

- `plan.md 全量路线` 从 69% 提升到 78% 到 82%。
- `Context quality / preflight 可操作性` 从 72% 提升到 82% 左右。
- `写作质量证据闭环` 从 95% 提升到 96% 到 97%，主要来自更强 fixture 与语义化修订映射，而不是新增概念。

不在本轮承诺：

- 不引入新的主生成链路。
- 不把所有质量判断交给 LLM judge。
- 不追求大而全的技法库扩写。
- 不做 UI 大改版，除非为了展示已有趋势证据所需的最小入口。

### P0 Provider Usage Calibration ✅/⚠️

目标：让 provider 预算不再只靠静态估算，而是能用真实运行 usage 反校准输入 token、输出 token 和成本阈值。

涉及模块：

- `agent-harness-core/src/budget_calibration.rs`
- `agent-writer-backend/src/writer_agent/provider_budget.rs`
- `agent-writer-backend/src/chapter_generation/draft_and_save.in.rs`
- `agent-writer-backend/src/writer_agent/kernel/trace_recording/*`
- `agent-writer-backend/src/headless.rs`

交付物：

- 在 writer provider budget 层接入 `BudgetCalibrationStore` 或同等持久化记录。
- 每次 provider 调用完成后记录 estimated input、actual input、actual output、model、task、decision。
- `evaluate_provider_budget` 输出 calibrated estimate、confidence 和 fallback reason。
- trace event 和 headless response 中保留校准前后差异，方便复盘。
- 已新增真实 provider gated test：`chat_text_usage_updates_budget_calibration`，验证 OpenRouter 返回 usage 后能写入 `record_full_usage` 并改变 calibration confidence。

验收：

- 单测覆盖“无历史数据时走默认估算”“有历史数据时使用校准倍率”“异常 usage 不污染校准”。
- gated 真实 API 测试覆盖 OpenRouter chat usage 回写：真实 `prompt_tokens` / `completion_tokens` / `total_tokens` 非零，并让 confidence 进入 Low；具体 token 数按当次模型返回记录，不作为稳定验收条件。
- 章节生成、Project Brain query、ExternalResearch 至少三类任务能写入 usage calibration。
- `cargo test -p agent-harness-core` 和 `cargo test -p agent-writer --lib` 通过。

风险与非目标：

- 风险是不同 provider 返回 usage 字段不一致；本轮只做可选字段和保守 fallback。
- 当前真实样本只有 OpenRouter + DeepSeek 单模型单轮 smoke，足以证明链路可用，但不足以证明长期估算稳定。
- 非目标是精确计费系统；目标是降低预算误判和审批噪音。

### P1 Context Source Timing、Taxonomy Mapping 与只读检索并行

目标：让 context quality 从“能判断是否够用”升级为“知道慢在哪里、缺哪类源、下一步该补什么”，同时让只读上下文检索具备安全并行能力。

涉及模块：

- `agent-harness-core/src/context_pack.rs`
- `agent-harness-core/src/context_quality.rs`
- `agent-writer-backend/src/writer_agent/context/assembly.in.rs`
- `agent-writer-backend/src/writer_agent/kernel/context_pack.rs`
- `agent-writer-backend/src/writer_agent/kernel/run_loop.rs`
- `agent-writer-backend/src/writer_agent/run_preflight.rs`

交付物：

- `ContextSourceReport` 增加 source taxonomy、source role、elapsed_ms、retrieval_status。
- Story OS / Project Brain / lore / prior chapters / promises / canon 映射为稳定 taxonomy，而不是只靠字符串名称。
- context quality recommendation 增加 action code，例如 `fetch_project_brain_anchor`、`refresh_prior_chapter_summary`、`reduce_low_value_lore`。
- 只读 retrieval 阶段允许并行收集 Project Brain、设定集、前文摘要、promise/canon 状态；落盘和状态突变仍保持串行。

验收：

- context quality 测试覆盖“缺 canon”“缺 prior chapter”“低价值 lore 占比过高”“某类 source 超时”。
- preflight block/warning 能展示 action code，而不是只有自然语言原因。
- 并行检索测试证明 source report 顺序稳定，失败源不会吞掉其他成功源。

风险与非目标：

- 风险是并行读引入不稳定顺序；必须在输出层排序。
- 非目标是并行写入或并行保存章节。

### P2 Planner-Aware AgentLoop 可执行语义

目标：把当前存在的 `ExecutionPlan`、`compile_plan` 和 AgentLoop 计划字段推进到可恢复、可观测的执行语义，而不是只作为结构化描述。

涉及模块：

- `agent-harness-core/src/execution_plan.rs`
- `agent-harness-core/src/agent_loop.rs`
- `agent-writer-backend/src/writer_agent/kernel/run_loop.rs`
- `agent-writer-backend/src/writer_agent/kernel/run_loop_spine.rs`
- `agent-writer-backend/src/writer_agent/kernel/trace_recording/*`

交付物：

- 每个 execution step 具备明确 lifecycle：planned、ready、running、blocked、completed、failed、skipped。
- `StepFailureAction` 与真实恢复动作绑定，例如 retry、request_context_supplement、pause_for_approval、abort。
- run trace 记录 step transition、失败原因、输入上下文摘要和恢复建议。
- AgentLoop 可以从未完成 plan 恢复到第一个 ready / blocked step，而不是重新编译整条 plan。

验收：

- 单测覆盖 step 成功、失败重试、blocked 等待补充、恢复执行。
- trace 中能重建一次 run 的 step 时间线。
- 不破坏现有章节生成和 headless manual request。

风险与非目标：

- 风险是把 AgentLoop 做成复杂 workflow engine；本轮只实现写作任务需要的最小 step 状态机。
- 非目标是支持任意 DAG 调度。

### P3 长任务 Checkpoint Recovery

目标：把已有 sprint checkpoint 能力扩展为长任务通用恢复能力，减少生成、批量冲刺、Project Brain rebuild 中断后的重复成本。

涉及模块：

- `agent-writer-backend/src/writer_agent/supervised_sprint.rs`
- `agent-writer-backend/src/writer_agent/memory/sprint_methods.in.rs`
- `agent-writer-backend/src/writer_agent/kernel/run_loop.rs`
- `agent-writer-backend/src/headless.rs`
- `agent-writer-backend/src/writer_agent/memory/schema.in.rs`

交付物：

- 定义 `LongTaskCheckpoint`，记录 task_id、task_kind、current_step、safe_resume_payload、budget_spent、artifact_refs。
- 章节生成在 context built、draft produced、quality report produced、save prepared 后写 checkpoint。
- 批量冲刺恢复时能跳过已保存章节，只继续未完成章节。
- checkpoint 与 provider budget trace 关联，恢复后预算继续累计。

验收：

- 单测覆盖 sprint checkpoint 恢复、章节生成中断恢复、checkpoint 与 project id 不匹配时拒绝恢复。
- headless API 能查询 latest checkpoint 和 resume candidate。
- 恢复不会重复落盘同一章节版本。

风险与非目标：

- 风险是恢复点不安全导致重复写入；所有会写文件的恢复点必须重新走 save conflict 检查。
- 非目标是支持任意历史版本回滚。

### P4 Required Anchors 从 Project Brain / Story OS 明确产出

目标：解决当前锚点抽取仍偏启发式的问题，让质量报告知道“本章必须承载什么”，而不是只从文本中猜。

涉及模块：

- `agent-writer-backend/src/chapter_generation/context.in.rs`
- `agent-writer-backend/src/chapter_generation/craft_quality.rs`
- `agent-writer-backend/src/brain_service/*`
- `agent-writer-backend/src/writer_agent/story_impact/*`
- `fixtures/writing_eval/project.json`

交付物：

- BuiltChapterContext 增加 `required_story_anchors`，来源包括 outline beat、open promise、canon constraint、Project Brain cross reference。
- `anchor_carry` 优先使用 required anchors；没有 required anchors 时才 fallback 到现有启发式提取。
- quality report 区分 missing required anchor、weakly carried anchor、optional anchor。
- eval fixture 增加“明确要求锚点但正文没有承载”的负例。

验收：

- 单测证明 required anchor 缺失会降低 anchor_carry，并给出具体 anchor id/source。
- eval runner 新增任务验证 promise/canon/outline anchor 能进入 quality signals。
- 没有 required anchor 的旧项目仍可生成，不出现硬阻断。

风险与非目标：

- 风险是过度阻断创作自由；本轮只把 required anchor 用于评分和建议，是否阻断由 preflight/quality gate 配置决定。
- 非目标是让 Project Brain 自动发明新伏笔。

### P5 Writing Eval Matrix 扩展 ✅

目标：把 13-task 小型 fixture 扩成可证明质量不退化的矩阵，覆盖类型、章节跨度、负例与恢复路径。

涉及模块：

- `fixtures/writing_eval/project.json`
- `fixtures/writing_eval/eval_tasks.jsonl`
- `fixtures/writing_eval/README.md`
- `agent-writer-backend/src/bin/eval_runner.rs`
- `agent-writer-backend/tests/writing_eval_test.rs`

交付物：

- 至少三个项目 profile：仙侠长篇、悬疑调查、现代职场或科幻。
- 每个 profile 至少覆盖 outline planning、chapter quality、targeted revision、canon conflict、promise progression。
- 负例矩阵覆盖：缺锚点、风格漂移、canon 冲突、伏笔未推进、修订无文本变化、Craft Memory 注入错误。
- eval 输出按 profile、task kind、metric、craft rule 汇总趋势。

验收：

- eval task 数量从 13 提升到 30 以上。
- `scripts\run-writing-eval.cmd` 输出 profile 维度 summary。
- 任一 profile 出现核心指标回退时 runner 失败。

风险与非目标：

- 风险是 fixture 太大拖慢本地反馈；本轮保留 smoke 子集和 full matrix 两种模式。
- 非目标是用 fixture 代替人工文学判断；它只保证关键能力不退化。

### P6 Sentence-Level / Semantic Revision Diff ✅

目标：把 `RevisionTargetChange` 从片段级映射升级为句级变化解释，能说明“哪一句为了哪个修订目标发生了什么变化”。

涉及模块：

- `agent-writer-backend/src/chapter_generation/craft_quality.rs`
- `agent-writer-backend/src/chapter_generation/craft_types.rs`
- `agent-writer-backend/tests/writing_eval_test.rs`
- `agent-writer-backend/src/bin/eval_runner.rs`

交付物：

- 新增 sentence segmentation 和 normalized sentence alignment。
- `RevisionTargetChange` 增加 `sentence_changes`，包含 before sentence、after sentence、change_kind、target_metric、confidence。
- 对于无法可靠对齐的变化，明确标记 low confidence，不硬解释。
- eval runner 统计 improved target 中有句级映射的比例。

验收：

- 单测覆盖改写、插入、删除、移动、无法对齐。
- targeted revision eval 要求关键 target 至少有一条 high 或 medium confidence sentence change。
- 低置信度不会被写成确定性因果结论。

风险与非目标：

- 风险是中文分句和语义对齐误判；本轮先用规则分句加保守相似度，不引入重型 embedding 依赖。
- 非目标是生成完整文学批注系统。

### P7 Craft Trend 接入 Companion / CI 可见面 ✅

目标：让 Craft Memory 和 eval trend 不只停留在 JSON 文件里，而是进入日常开发和作者使用可见面。

涉及模块：

- `agent-writer-backend/src/bin/eval_runner.rs`
- `fixtures/writing_eval/eval_trend.json`
- `agent-writer-backend/src/headless.rs`
- `agent-writer-backend/src/writer_agent/kernel/trace_recording/*`
- Companion 相关入口以仓库实际 UI 模块为准，先做最小 API 暴露。

交付物：

- eval runner 输出 Markdown summary，列出 profile 回退、metric 回退、craft rule 回退。
- headless 增加查询 craft trend / eval trend 的只读方法。
- CI 可直接运行 writing eval full matrix，并在失败时输出最高风险项。
- Companion 或调用方可以读取趋势 summary，不需要解析完整 JSON。

验收：

- 本地运行 eval 后生成 JSONL、trend JSON、Markdown summary。
- CI 模式下 regression 会失败并打印 task id、metric、craft rule。
- headless trend API 不触发生成、不写入 Craft Memory。

风险与非目标：

- 风险是展示层过早复杂化；本轮只做可读 summary 和 API，不做复杂图表。
- 非目标是替代 plan.md 的人工审查。

### 执行顺序

1. 先做 P0 和 P1，因为它们直接影响预算判断、preflight 阻断和后续长任务恢复的可靠性。
2. 再做 P2 和 P3，把 AgentLoop 与 checkpoint 从“有结构”推进到“能恢复”。
3. 接着做 P4 和 P6，补强写作质量证据的精度。
4. 最后做 P5 和 P7，让证据矩阵和趋势展示支撑长期迭代。

### 里程碑验收

Milestone A：Context 与预算可信度提升。

- P0、P1 完成。
- `cargo test -p agent-harness-core`、`cargo test -p agent-writer --lib` 通过。
- preflight report 能输出 calibrated budget、source timing、taxonomy action code。

Milestone B：执行恢复能力提升。

- P2、P3 完成。
- AgentLoop trace 能重建 step timeline。
- 长任务中断后能从安全 checkpoint 恢复，并且不会重复保存章节。

Milestone C：写作证据密度提升。

- P4、P5、P6 完成。
- writing eval 达到 30+ tasks，并覆盖多 profile 与负例矩阵。
- RevisionReport 能输出句级 target change，低置信度明确标记。

Milestone D：趋势进入日常工作流。

- P7 完成。
- eval trend 同时有 JSON 与 Markdown summary。
- CI / Companion 能消费趋势摘要，开发者不必手工翻 JSON。

### 完成度重估规则

只有满足以下条件才把全量路线从 69% 上调：

- P0 + P1 完成并有测试：上调到 73% 到 75%。
- P2 + P3 至少完成一个且可恢复测试通过：上调到 76% 到 78%。
- P4 + P5 + P6 至少完成两个且 eval matrix 达到 30+ tasks：上调到 80% 左右。
- P7 完成后不单独大幅上调核心完成度，但会提高长期维护可信度。

## 2026-05-09 性能瓶颈与写作能力提升计划

### 证据边界

本计划基于当前仓库实现、`plan.md` 已记录的真实 OpenRouter 分拆验证、以及本地 `reports/real_author_session_thirty_chapter_gate.json` 的三十章 gate 报告。已知证据：

- OpenRouter `deepseek/deepseek-v4-flash` gated `api_integration_tests` 已分拆验证 12/12 通过；单命令串行全量存在 30 分钟超时风险。
- 三十章真实写作 gate：`chapters=30`、`avg_chars=2150`、`min_carry_rate=0.60`、`avg_anchor_hit=0.92`。
- 三十章报告统计：`avg_carry=0.893`、`repaired_count=0`、`min_chars=1318`、`max_chars=3422`。
- 三十章 preview 仍能观察到重复开场或场景停滞风险，例如 10/11、12/13/14、25/26/27/28；但当前报告尚未输出结构化 `duplicatePreviewGroups`，因此只能作为人工可见风险，而不是自动 gate 证据。
- 结论：当前质量 gate 能约束锚点承接，但不能充分约束长篇推进、重复场景和新信息密度。

### 总体判断

性能瓶颈主要在真实 provider 串行生成与修复追加调用，不在本地 Rust 计算。写作能力已经具备“写作工艺注入、质量诊断、定向修订、Craft Memory 回流、eval 趋势”的闭环，但长篇连续创作仍缺少剧情状态推进约束。

### P8 Provider Latency 与调用链路遥测

目标：把“慢在哪里”从日志观察升级为结构化数据，区分 provider 延迟、context 装配、质量诊断、修订、保存和 checkpoint 开销。

当前状态：瞬态重试已经部分完成（最多 3 次、线性退避），但还缺结构化 latency、provider call count、retry count、phase timing 和长链路报告汇总。

涉及模块：

- `agent-writer-backend/src/llm_runtime.rs`
- `agent-writer-backend/src/chapter_generation/pipeline/main.in.rs`
- `agent-writer-backend/src/chapter_generation/draft_and_save.in.rs`
- `agent-writer-backend/src/writer_agent/kernel/trace_recording/*`
- `reports/*`

交付物：

- 每次 provider 调用记录 `profile`、`latency_ms`、`input_chars`、`output_chars`、`usage`、`retry_count`、`repaired`。
- 章节生成报告新增 phase timing：context_built、draft_produced、length_repair、quality_report、targeted_revision、save_prepared、saved。
- `real_author_session_thirty_chapter_gate.json` 或同类报告记录每章 provider 调用次数与总耗时。
- retry 事件区分 request error、429/5xx、JSON decode transient，不与质量失败混为一谈。

验收：

- 单测覆盖 telemetry builder 不泄露 API key。
- 真实 gated test 能输出每章 latency summary。
- 全量报告能计算 p50/p90/p95 chapter latency、平均 provider calls per chapter。

风险与非目标：

- 风险是 telemetry 太吵；报告默认输出 summary，详细事件留 JSON。
- 非目标是做精确计费系统，成本估算仍由 provider budget 模块负责。

### P9 只读上下文检索并行落地

目标：减少非 provider 等待时间，把 Project Brain、lore、前文摘要、promise/canon 状态等只读来源并行预取，再走确定性装配。

涉及模块：

- `agent-writer-backend/src/writer_agent/context/assembly.in.rs`
- `agent-writer-backend/src/chapter_generation/context.in.rs`
- `agent-writer-backend/src/writer_agent/kernel/run_loop.rs`
- `agent-harness-core/src/context_pack.rs`

交付物：

- 章节生成上下文构建改为 read-only source fetch 并行化，输出仍按固定 priority 排序。
- `ContextSourceReport` 的 `elapsed_ms` / `retrieval_status` 在真实调用点填充，不再主要停留在结构字段。
- 单个 source 失败时保留 status 和 action code，不吞掉其他成功来源。
- preflight 把慢 source 和缺 source 分开提示。

验收：

- 单测证明并行预取后 source order 稳定。
- 单测覆盖一个 source timeout/失败时其他 source 仍进入 context pack。
- 真实或模拟测试能看到 source 级 elapsed_ms 非零。

风险与非目标：

- 风险是并发读引入顺序不稳定；装配层必须排序。
- 非目标是并行写入、并行保存或并行修改 memory。

### P10 长篇推进去重 Gate

目标：解决真实三十章里出现的场景/段落重复问题。锚点承接合格不代表剧情推进合格，必须新增重复检测和推进检测。

涉及模块：

- `agent-writer-backend/src/chapter_generation/craft_quality.rs`
- `agent-writer-backend/src/chapter_generation/pipeline/main.in.rs`
- `agent-writer-backend/src/bin/eval_runner.rs`
- `fixtures/writing_eval/*/eval_tasks.jsonl`
- `reports/real_author_session_thirty_chapter_gate.json`

交付物：

- 新增 `scene_repetition` metric：检测相邻章节开头、场景动作、核心问答、关键意象是否高重复。
- 新增 `plot_progression` metric：检测本章是否改变至少一个剧情状态、关系状态、债务状态或信息状态。
- 新增 `new_information_density` metric：检测正文是否只复述前文，而没有新增证据、选择、后果或代价。
- 三十章 gate 报告加入重复组、重复章节号、重复片段摘要。

验收：

- 单测覆盖完全重复、轻微改写重复、合法呼应、必要 recap 四类情况。
- eval fixture 增加“锚点承接合格但剧情没有推进”的负例。
- 真实三十章 gate 中如果出现 3 章连续同场景重复，应进入 warning 或 fail，阈值由配置控制。

风险与非目标：

- 风险是把有意回环、复调、仪式化重复误判为失败；因此第一阶段只 warning，不直接硬阻断。
- 非目标是替代人工编辑判断，目标是捕捉明显机械重复。

### P11 Story State Delta 明确化

目标：让模型每章不只“带着锚点写”，还要知道本章必须改变什么状态。

涉及模块：

- `agent-writer-backend/src/chapter_generation/context.in.rs`
- `agent-writer-backend/src/chapter_generation/craft_prompt.rs`
- `agent-writer-backend/src/chapter_generation/craft_quality.rs`
- `agent-writer-backend/src/writer_agent/memory.rs`
- `agent-writer-backend/src/writer_agent/story_impact/*`

交付物：

- `BuiltChapterContext` 增加 `required_state_deltas`，来源包括 outline beat、open promise、chapter mission、canon constraint、previous chapter result。
- draft prompt 增加短约束：本章必须至少改变一个 state delta，不能只复述上一章压力。
- quality report 输出 `state_delta_coverage`：covered / weak / missing。
- Revision prompt 能针对 missing delta 要求补写行动、选择、代价或新证据。

验收：

- 单测证明有 required delta 但正文未改变状态时会降分。
- 单测证明正文只提到锚点但没有状态变化时，`anchor_carry` 可合格但 `state_delta_coverage` 不合格。
- eval runner 新增任务验证 promise/canon/mission delta 能进入 prompt 与 quality report。

风险与非目标：

- 风险是状态 delta 设计过细导致 prompt 僵硬；先限制为 1-3 条高优先级 delta。
- 非目标是自动规划整卷剧情，只做本章级推进约束。

### P12 长链路质量报告升级

目标：让真实三章/三十章 gate 不只看 `anchor_carry`，还看重复率、推进率、修复率和长度稳定性。

涉及模块：

- `agent-writer-backend/src/api_integration_tests.rs`
- `agent-writer-backend/src/chapter_generation/craft_quality.rs`
- `reports/*`
- `plan.md`

交付物：

- 三十章 gate 报告新增 `duplicatePreviewGroups`、`repairRate`、`minChars`、`maxChars`、`avgCarryRate`。
- gate summary 增加 `qualityWarnings`，区分 fail 与 warning。
- `plan.md` 的完成度估算以后必须引用长链路质量指标，而不是只引用 pass/fail。

验收：

- 真实 gated test 报告能列出重复章节组。
- 如果 `repairRate` 明显升高或 `duplicatePreviewGroups` 超阈值，报告至少 warning。
- 不影响默认非真实测试速度；真实长链路仍 gated。

风险与非目标：

- 风险是报告过度膨胀；正文 preview 继续截断，详细文本不默认写入。
- 非目标是把真实长链路测试放进普通 CI。

### P13 性能模式与质量模式分层

目标：明确 fast draft、balanced、quality gate 三种运行模式，避免所有场景都承受三十章级质量成本。

涉及模块：

- `agent-writer-backend/src/chapter_generation/pipeline/main.in.rs`
- `agent-writer-backend/src/headless.rs`
- `forge-agent-mcp/src/tools.rs`
- `agent-writer-backend/src/writer_agent/supervised_sprint.rs`

交付物：

- `generation_quality_mode`：`fast`、`balanced`、`strict`。
- fast：单次 draft + 基础长度校验 + 非阻断 warning。
- balanced：draft + quality report + 必要时一次 targeted revision。
- strict：balanced + repetition/state delta gate + 长链路报告。
- MCP/headless 参数暴露模式，默认 balanced。

验收：

- 单测覆盖三种模式触发的 provider 调用上限。
- 文档说明每种模式适用场景与成本。
- sprint 能按章节目标选择模式，例如关键章 strict、过渡章 balanced。

风险与非目标：

- 风险是模式过多造成心智负担；只暴露 3 档，不暴露内部细项。
- 非目标是让 fast 模式绕过保存安全和 revision conflict 检查。

### 执行顺序

1. P8 先做，因为没有 latency / call count / retry count，就无法精确优化性能。
2. P10 与 P12 并行推进，先把真实报告里的重复问题变成可见指标。
3. P11 接入 Story State Delta，让模型生成前就知道必须推进什么。
4. P9 在上下文路径稳定后落地，减少非 provider 等待。
5. P13 最后做模式分层，把性能和质量的取舍暴露给调用方。

### 新完成度重估规则

- P8 + P12 完成：性能可观测性从 6/10 提升到 7/10，写作长链路证据可信度提升。
- P10 完成并接入三十章 gate：长篇连续创作能力从 7/10 提升到 7.8/10。
- P11 完成并进入 prompt + quality report：长篇连续创作能力提升到 8.2/10 左右。
- P9 完成真实调用点并行预取：性能吞吐视项目上下文规模提升，预估从 6/10 到 6.8/10。
- P13 完成：产品可用性提升，因为用户能在速度和质量之间显式选择。

### 近期优先验收目标

下一轮最小闭环：

1. 三十章 gate 报告能输出重复章节组和长度波动。
2. `craft_quality` 能识别“锚点合格但场景重复”的负例。
3. draft prompt 能收到 1-3 条 required state delta。
4. `plan.md` 后续完成度估算引用 `duplicatePreviewGroups`、`repairRate`、`stateDeltaCoverage`，不再只引用 `anchor_carry`。

## 2026-05-10 通用复杂世界观能力强化计划

### 目标边界

本计划不为任何单一世界观做专项适配，不硬编码《九厄十二劫经》、修炼境界、厄劫术语或特定题材规则。复杂世界观文档只作为压力测试样本，用来验证系统是否能把任意高密度设定转成可检索、可注入、可校验、可追溯的写作约束。

核心目标：

- 从“能把设定塞进 prompt”升级为“能把设定编译成 Story OS 资产”。
- 从“模型记住世界观”升级为“系统持有规则、关系、状态和证据”。
- 从“写完后主观判断是否跑偏”升级为“生成前有章节合同，生成后有通用一致性验证”。

不做：

- 不为某部作品写专属 parser、专属规则、专属 metric。
- 不把 LLM 自动抽取结果直接当 canon；所有高风险规则必须保留 source evidence、confidence 和 author approval 状态。
- 不让复杂项目默认拖慢普通项目；严格能力最终应由 P13 的 `strict` 模式启用。

### 证据边界

当前项目已有可复用底座：

- Project Brain、跨引用、source revision compare/restore 已存在，适合作为世界观知识索引层。
- `ContextSourceReport`、context quality action code、preflight 阻断/建议已存在，适合作为缺失设定源的入口。
- `SceneCraftPlan`、`required_story_anchors`、`required_state_deltas`、`ChapterQualityReport`、`RevisionReport` 已接入章节链路。
- writing eval 已扩展到 66 tasks，能承载通用世界观能力的规则回归。

当前不足：

- 设定文档仍主要作为文本块检索，缺少 typed fact / rule / relation / constraint。
- canon 冲突检查已有雏形，但还不能通用表达“禁止动作、触发条件、例外、后果、严重级别”。
- 每章生成前的约束仍偏写作任务和锚点，缺少从世界观规则自动编译出的 scene contract。
- 生成后质量检查能看锚点、风格、长度、状态变化，但还缺通用设定一致性、术语错用、层级混淆、代价跳过、未授权新增设定检测。

### P14 World Bible Compiler

目标：把任意世界观文档从 Markdown / 文本编译成通用资产，而不是只做 chunk 检索。

涉及模块：

- `agent-writer-backend/src/brain_service/*`
- `agent-writer-backend/src/headless.rs`
- `forge-agent-mcp/src/tools.rs`
- 新增或扩展 world bible / canon 数据结构模块

通用 schema：

- `WorldEntity`：人物、势力、地点、资源、术语、制度、物件、能力。
- `WorldRule`：规则、禁忌、代价、触发条件、例外、后果、严重级别。
- `WorldRelation`：来源、隶属、克制、转化、冲突、等价、伪装、继承。
- `WorldHierarchy`：境界、职级、位格、技术阶段、权限等级。
- `WorldTimelineFact`：古史、现世事件、未来伏笔、断代、版本差异。

交付物：

- 文档 ingest 后产出 `world_bible_index`，每条结构化资产必须带 `source_ref`、原文 excerpt、confidence、approval_status。
- 支持 LLM-assisted extraction，但默认写入 `proposed` 状态，不直接污染 approved canon。
- 支持 author approve/reject/merge，并保留 source revision。
- Project Brain query 能同时返回 raw chunks 和 typed world assets。

验收：

- 单测覆盖 Markdown 标题、表格、列表、引用块的结构化抽取输入。
- 单测证明低置信度或无 source_ref 的规则不能进入 approved canon。
- MCP/headless 能列出某项目的 entities/rules/relations/hierarchies。
- eval fixture 新增一个复杂世界观 profile，用通用 schema 表达规则，不出现题材专属字段。

风险与非目标：

- 风险是自动抽取过度自信；必须把 author approval 作为硬边界。
- 非目标是一次性完美理解整本设定；第一阶段先做可追溯结构化草案。

### P15 Canon Constraint Engine

目标：把“设定资料”升级为生成前后都能执行的通用约束。

涉及模块：

- `agent-writer-backend/src/chapter_generation/context.in.rs`
- `agent-writer-backend/src/chapter_generation/craft_quality.rs`
- `agent-writer-backend/src/writer_agent/story_impact/*`
- `agent-harness-core/src/context_quality.rs`

通用约束模型：

- `required_fact`：本章必须承认的事实。
- `forbidden_claim`：不得写出的设定断言。
- `forbidden_action`：角色/势力/能力不得完成的动作。
- `required_cost`：触发某能力、规则或选择时必须支付的代价。
- `hierarchy_limit`：层级不足时不得越级获得能力、权限或信息。
- `exception_rule`：允许违反表象规则的已批准例外。

交付物：

- `CanonConstraint` 数据结构，带 severity、source_ref、applies_to、trigger、expected_consequence。
- preflight 能识别关键 canon 缺失，并输出 action code，例如 `approve_world_rule`、`fetch_canon_constraint`、`resolve_rule_conflict`。
- draft prompt 注入本章最相关的 3-8 条约束，而不是倾倒整份设定。
- quality report 输出 `canon_constraint_violations`，包括证据片段、违反规则、建议修订方向。

验收：

- 单测覆盖 forbidden claim、required cost、hierarchy limit、exception rule。
- eval runner 新增 canon constraint 任务：候选文本违反规则时必须 fail 或 warning。
- 无 approved source_ref 的规则只能作为 warning，不能硬阻断。

风险与非目标：

- 风险是硬规则过多导致创作僵硬；按 severity 和 relevance 限制注入数量。
- 非目标是替代作者判断；系统只给证据、冲突和建议。

### P16 Scene Contract Compiler

目标：每章生成前先编译“章节合同”，让模型知道本章必须推进什么、不能触碰什么、允许新增什么。

涉及模块：

- `agent-writer-backend/src/chapter_generation/context.in.rs`
- `agent-writer-backend/src/chapter_generation/craft_prompt.rs`
- `agent-writer-backend/src/chapter_generation/types_and_utils.in.rs`
- `agent-writer-backend/src/chapter_generation/pipeline/main.in.rs`

交付物：

- `SceneContract`：chapter mission、required facts、required state deltas、active constraints、allowed reveals、blocked reveals、required costs、continuity anchors。
- contract 来源包括 outline、previous chapter summary、promise ledger、approved world rules、Project Brain chunks、author instruction。
- draft prompt 只注入 compact contract；详细证据保留在 report 中。
- Revision prompt 根据 contract violation 定向修订，而不是泛泛要求“更符合设定”。

验收：

- 单测证明同一世界观文档可为不同章节生成不同 contract。
- 单测证明 blocked reveal 不会进入 draft prompt 的“可揭示信息”。
- eval runner 新增任务验证 mission、rule、cost、state delta 能同时进入 contract。

风险与非目标：

- 风险是 contract 太长吃掉正文预算；必须有 token/char budget 和 priority 排序。
- 非目标是自动写完整大纲；只做当前章节可执行合同。

### P17 Generic Consistency Validator

目标：生成后用通用规则检查复杂世界观跑偏，不依赖题材专属判断。

涉及模块：

- `agent-writer-backend/src/chapter_generation/craft_quality.rs`
- `agent-writer-backend/src/chapter_generation/pipeline/main.in.rs`
- `agent-writer-backend/src/bin/eval_runner.rs`

通用检查项：

- `unsupported_world_claim`：正文新增了未授权设定断言。
- `canon_violation`：违反 approved constraint。
- `hierarchy_confusion`：层级、位格、权限、阶段混淆。
- `cost_skipped`：触发能力或选择但没有支付代价。
- `term_misuse`：术语被换义、误用或混同。
- `state_regression`：前章已改变的状态被无解释回滚。
- `new_information_density`：本章大量复述，缺少新证据、新选择、新后果。

交付物：

- ChapterQualityReport 增加 generic world consistency metrics。
- violation report 必须引用正文片段和 source_ref，不能只给空泛评语。
- strict 模式下高严重级别 violation 可阻断保存或触发 targeted revision；balanced 模式默认 warning。

验收：

- 单测覆盖未授权新增设定、跳过代价、层级混淆、术语误用。
- eval fixture 覆盖至少 2 个题材 profile，证明 validator 不依赖玄幻术语。
- 报告能区分 hard violation、soft warning、insufficient evidence。

风险与非目标：

- 风险是假阳性影响写作流畅度；第一阶段 strict 才硬阻断，balanced 只提示。
- 非目标是判断文学价值；只判断设定一致性和状态连续性。

### P18 Evidence-Bound Retrieval 与 Prompt 注入

目标：任何世界观引用都能追溯，不允许模型把“似乎记得”的设定当事实。

涉及模块：

- `agent-writer-backend/src/chapter_generation/pipeline/context.in.rs`
- `agent-harness-core/src/context_pack.rs`
- `agent-harness-core/src/context_quality.rs`
- `forge-agent-mcp/src/tools.rs`

交付物：

- context pack 中区分 raw evidence、approved rule、proposed rule、author instruction。
- prompt 注入每条关键约束时携带短 source label，report 中保留完整 source_ref。
- preflight 能提示“当前章节需要某类证据但只有 proposed rule / raw chunk / no source”。
- 对冲突来源给出 conflict set，不让模型自行选边。

验收：

- 单测证明无 evidence 的关键规则不能进入 hard constraints。
- 单测证明 source conflict 会触发 preflight warning 或 block。
- MCP 工具可查询某条约束的来源、批准状态和使用章节。

风险与非目标：

- 风险是证据链过重；prompt 只带短标签，详细证据留 JSON/report。
- 非目标是全文引用原文；只保留必要 excerpt 和定位信息。

### P19 Story State Ledger 泛化

目标：把长篇连续创作的状态从“摘要文本”升级为可查询、可验证的状态账本。

涉及模块：

- `agent-writer-backend/src/writer_agent/memory.rs`
- `agent-writer-backend/src/writer_agent/story_impact/*`
- `agent-writer-backend/src/chapter_generation/context.in.rs`
- `agent-writer-backend/src/writer_agent/supervised_sprint.rs`

通用状态类型：

- character knowledge / belief / secret。
- relationship status。
- faction stance。
- resource ownership。
- rule triggered / cost paid。
- promise open / advanced / paid / contradicted。
- world situation escalated / stabilized / hidden。

交付物：

- 每章保存后可生成 `StateLedgerDelta`，进入下一章 context。
- draft 前从 ledger 编译 required_state_deltas 和 forbidden regressions。
- quality report 检查正文是否覆盖 required delta，是否无解释回滚状态。

验收：

- 单测证明状态变化能跨章节传递。
- 单测证明状态回滚需要解释，否则 warning。
- 三十章 gate 报告能统计 `stateDeltaCoverage`。

风险与非目标：

- 风险是状态过细导致维护成本高；只记录对剧情、设定、承诺有影响的状态。
- 非目标是替代完整数据库；先做章节级 ledger。

### P20 通用复杂世界观 Eval Matrix

目标：用压力测试证明系统处理复杂设定的能力，而不是只证明某个样例能写。

涉及模块：

- `fixtures/writing_eval/*`
- `agent-writer-backend/src/bin/eval_runner.rs`
- `agent-writer-backend/tests/writing_eval_test.rs`

交付物：

- 新增复杂世界观 fixture，至少覆盖两个不同题材，例如玄幻规则体系、科幻制度/技术体系。
- 每个 profile 包含 typed rules、relations、hierarchy、forbidden claims、required costs。
- eval tasks 覆盖抽取、contract 编译、canon violation、cost skipped、hierarchy confusion、state delta、unsupported claim。
- eval summary 增加 world consistency warning/fail 统计。

验收：

- full writing eval 增加 30+ 通用复杂世界观任务。
- 至少 2 个 profile 证明 schema 不依赖题材术语。
- 任一 hard canon violation 从 pass 变 fail 时，trend report 能捕捉 regression。

风险与非目标：

- 风险是 fixture 过拟合；每个任务必须引用 schema，而不是匹配固定词。
- 非目标是用 LLM judge 取代规则测试；LLM judge 可作为后续辅助，不作为第一阶段门槛。

### 执行顺序

1. P14 先做：没有 typed world assets，后续 canon、contract、validator 都只能继续吃文本块。
2. P15 与 P18 并行：规则必须可执行，也必须有证据链。
3. P16 接入章节主链路：把世界观资产转成当前章节可用的 SceneContract。
4. P17 接入 quality report 和 targeted revision：生成后能发现并修正通用设定跑偏。
5. P19 扩展长篇状态账本：让复杂规则和状态能跨章节延续。
6. P20 最后扩展 eval matrix：用多题材压力测试锁住通用能力。

### 完成度重估规则

- P14 完成：复杂世界观“可结构化”能力从 4/10 提升到 5.5/10。
- P14 + P15 + P18 完成：复杂世界观“可追溯、可约束”能力提升到 7/10。
- P16 + P17 完成：复杂世界观“可写作化、可校验”能力提升到 8/10。
- P19 完成并进入三十章 gate：长篇复杂世界观连续性提升到 8.5/10。
- P20 完成：该能力从项目经验判断变成可回归证据。

### 近期最小闭环

下一轮不要先做庞大全自动理解系统，先完成一个可验证薄切：

1. 新增通用 `WorldRule` / `WorldEntity` / `WorldRelation` / `WorldHierarchy` 草案结构。
2. 从任意 Markdown fixture 中手工或半自动生成 10-20 条 proposed world assets，全部带 source_ref。
3. approve 其中 5-8 条，编译成一个章节 `SceneContract`。
4. 让 draft prompt 注入 compact contract。
5. 写一个候选文本故意违反 approved rule，quality report 必须给出 violation、正文片段和 source_ref。

## 2026-05-10 通用能力强化首轮开发计划

### 核心判断

P14-P20 是完整路线，但首轮不应该直接做庞大的自动世界观理解系统。真正要先打通的是一个最小可验证闭环：

> EvidenceRef → WorldAsset → CanonConstraint → SceneContract → Draft Prompt → Consistency Violation Report

这条链路跑通以后，再扩展自动抽取、多题材 eval、长篇状态账本才有意义。否则容易变成“抽取了很多设定，但生成链路用不上；生成链路用了设定，但 violation 不可追溯”。

### 首轮目标

用通用数据结构证明系统能处理任意复杂世界观规则，不做题材适配：

- 规则必须有证据来源。
- 规则必须有批准状态。
- 章节生成前必须编译成 compact contract。
- 生成后必须能指出违反哪条规则、正文哪里违反、原文证据在哪里。

### D1 EvidenceRef 与 WorldAsset 最小模型

目标：先建立通用世界观资产的最小表达，不追求一次覆盖全部设定形态。

建议数据结构：

```rust
pub struct EvidenceRef {
    pub source_id: String,
    pub source_path: Option<String>,
    pub start_line: Option<u32>,
    pub end_line: Option<u32>,
    pub excerpt: String,
    pub confidence: f32,
}

pub enum ApprovalStatus {
    Proposed,
    Approved,
    Rejected,
}

pub enum WorldAssetKind {
    Entity,
    Rule,
    Relation,
    Hierarchy,
    TimelineFact,
}

pub struct WorldAsset {
    pub id: String,
    pub kind: WorldAssetKind,
    pub name: String,
    pub summary: String,
    pub evidence: Vec<EvidenceRef>,
    pub approval_status: ApprovalStatus,
    pub tags: Vec<String>,
}
```

首轮只要求：

- `EvidenceRef.excerpt` 必填。
- `WorldAsset.approval_status` 必填。
- 没有 evidence 的 asset 不能成为 hard constraint。
- `Proposed` asset 只能用于提示和候选建议，不能用于阻断。

涉及模块：

- 可优先新增在 `agent-writer-backend/src/writer_agent/world_bible.rs` 或同等模块。
- `agent-writer-backend/src/writer_agent/mod.rs`
- 后续再接 Project Brain，不在首轮强依赖。

验收：

- 单测覆盖 asset 创建、approval 状态切换、无 evidence 不可 hard enforce。
- JSON serialization 稳定，方便后续写入项目存储。

### D2 CanonConstraint 最小模型

目标：从 approved `WorldAsset::Rule` 编译出可执行约束。

建议数据结构：

```rust
pub enum CanonConstraintKind {
    RequiredFact,
    ForbiddenClaim,
    ForbiddenAction,
    RequiredCost,
    HierarchyLimit,
    ExceptionRule,
}

pub enum ConstraintSeverity {
    Info,
    Warning,
    Hard,
}

pub struct CanonConstraint {
    pub id: String,
    pub kind: CanonConstraintKind,
    pub summary: String,
    pub trigger_terms: Vec<String>,
    pub forbidden_terms: Vec<String>,
    pub required_terms: Vec<String>,
    pub severity: ConstraintSeverity,
    pub source_asset_id: String,
    pub evidence: Vec<EvidenceRef>,
}
```

首轮不做复杂语义推理，先做可解释的启发式：

- `ForbiddenClaim`：正文命中 forbidden_terms 时报告 violation。
- `RequiredCost`：正文命中 trigger_terms 但缺 required_terms 时报告 violation。
- `HierarchyLimit`：正文同时命中越级动作和低层级身份时报告 warning/hard。

验收：

- 单测覆盖 forbidden claim、required cost、hierarchy limit。
- `Proposed` asset 编译出的 constraint 最高只能是 warning。
- `Approved + Hard` 才允许进入 strict 模式阻断。

### D3 SceneContract 最小模型

目标：把世界观约束转成当前章节的可执行合同，而不是把全量设定塞进 prompt。

建议数据结构：

```rust
pub struct SceneContract {
    pub chapter_id: String,
    pub mission: String,
    pub required_facts: Vec<CanonConstraint>,
    pub active_constraints: Vec<CanonConstraint>,
    pub required_state_deltas: Vec<StateDelta>,
    pub allowed_reveals: Vec<String>,
    pub blocked_reveals: Vec<String>,
    pub evidence_refs: Vec<EvidenceRef>,
}
```

首轮策略：

- 按 chapter mission / user instruction / outline keywords 选择最相关的 3-8 条 constraint。
- contract prompt 使用短摘要，不直接注入长 excerpt。
- report 保留完整 source evidence。

涉及模块：

- `agent-writer-backend/src/chapter_generation/context.in.rs`
- `agent-writer-backend/src/chapter_generation/craft_prompt.rs`
- `agent-writer-backend/src/chapter_generation/types_and_utils.in.rs`

验收：

- 单测证明同一组 world assets 能为不同 mission 选择不同 constraints。
- 单测证明 hard constraints 优先于 warning constraints。
- prompt snapshot 包含 compact contract，但不包含完整长文档。

### D4 Prompt 注入最小改造

目标：让模型在写作时看到“章节合同”，不是看到散乱设定。

Prompt 建议格式：

```text
【章节合同】
- 本章任务：...
- 必须承认：...
- 硬性规则：...
- 触发代价：...
- 禁止提前揭示：...
- 本章必须改变的状态：...
```

要求：

- 控制长度，默认只注入最高相关的 3-8 条。
- 每条规则带短 source label，例如 `[world:rule-001]`。
- 不在 prompt 中塞完整证据 excerpt，避免污染正文风格。

验收：

- 单测或 snapshot 证明 prompt 中出现 SceneContract。
- 旧项目无 SceneContract 时不影响原生成链路。
- contract 为空时不输出空标题噪音。

### D5 Consistency Validator 最小闭环

目标：写完后能给出可追溯 violation，而不是主观说“不符合设定”。

建议数据结构：

```rust
pub struct WorldConsistencyViolation {
    pub constraint_id: String,
    pub severity: ConstraintSeverity,
    pub kind: CanonConstraintKind,
    pub message: String,
    pub text_excerpt: String,
    pub evidence: Vec<EvidenceRef>,
    pub suggested_fix: String,
}
```

首轮检查：

- forbidden claim。
- required cost skipped。
- hierarchy limit warning。

接入位置：

- `agent-writer-backend/src/chapter_generation/craft_quality.rs`
- `ChapterQualityReport` 增加 `world_consistency_violations` 或等价字段。
- targeted revision 可先只读取 violation message，不必一次做复杂 rewrite。

验收：

- 候选文本故意违反 approved hard rule，report 必须包含 constraint id、正文片段、source excerpt。
- 候选文本只违反 proposed rule，report 只能 warning，不能 hard fail。
- 无 evidence 的 constraint 不参与 hard violation。

### D6 Eval 薄切

目标：用最小 fixture 锁住首轮能力，避免只靠人工观察。

新增任务类型建议：

- `world_asset_contract`：验证 approved assets 能编译进 SceneContract。
- `canon_forbidden_claim`：验证 forbidden claim 被抓出。
- `canon_required_cost`：验证触发能力但跳过代价被抓出。
- `canon_proposed_not_hard`：验证 proposed rule 不会 hard block。
- `scene_contract_prompt`：验证 compact contract 被注入 prompt。

Fixture 要求：

- 至少两个题材样本，每个 5-8 条 world assets。
- 不使用题材专属字段。
- 断言基于 asset id / constraint id / severity，不基于固定文案。

验收：

- 首轮新增 10-15 个 eval tasks。
- `scripts\run-writing-eval.cmd` 通过。
- trend report 能看到新增 world consistency 类任务。

### 首轮执行顺序

1. D1 数据结构和序列化。
2. D2 constraint 编译和启发式 validator。
3. D3 SceneContract 编译。
4. D4 prompt 注入。
5. D5 ChapterQualityReport violation 输出。
6. D6 eval fixture 和趋势报告接入。

### 首轮完成定义

只有同时满足以下条件，才算通用能力强化首轮完成：

- 一个 approved world rule 能从 evidence 进入 SceneContract。
- draft prompt 能看到 compact contract。
- 一个违规候选文本能被 validator 抓出。
- violation 报告包含 source evidence。
- proposed rule 不会被误当 hard canon。
- 至少两个不同题材 fixture 通过同一套逻辑。

### 不应提前做的事

- 不先做全自动长文档抽取。
- 不先做图数据库或复杂 UI。
- 不先做 LLM judge。
- 不先让所有生成默认 strict。
- 不把某个世界观里的术语写进代码。

### 与 P13 的关系

首轮能力默认只应在 `strict` 或测试路径中硬阻断；`balanced` 可以展示 warning；`fast` 不应承担世界观 validator 成本。这样可以避免复杂项目能力拖慢普通写作请求。

## 2026-05-10 Agent 底层能力强化计划

### 核心判断

当前底层不是空白：`AgentLoopEvent` 已有 intent、tool inventory、provider guard、context window、compaction、plan/step、failure bundle、complete/TTFT 等事件；`ExecutionPlan` 已有 step、side effect、failure action；工具层已有 permission/audit；context quality、provider budget、long task checkpoint、sprint checkpoint 也已存在。

真正缺口不是“再加一个业务功能”，而是把这些已有能力统一成可规划、可恢复、可审计、可约束的 agent runtime。

目标链路：

```text
TaskPacket
  -> ExecutionPlan
  -> StepContract
  -> Context/Tool/Provider Guards
  -> StepEvidence
  -> Durable Checkpoint
  -> Recovery Decision
  -> Trace / Eval
```

### 非目标

- 不重写整个 agent loop。
- 不引入与写作业务强绑定的底层抽象。
- 不用自然语言 plan 替代结构化 step contract。
- 不把所有失败都简单 retry。
- 不让底层 runtime 直接决定创作质量，只提供可靠执行、约束和证据。

### A1 StepContract 与真实 Step 调度

目标：把 `ExecutionPlan` 从“计划对象”升级成真实 step 调度合同。

当前基础：

- `ExecutionPlan` 已有 `ExecutionStep`、`allowed_tools`、`max_side_effect`、`success_signals`、`on_failure`。
- `AgentLoop` 已能发 `PlanStarted`、`StepStarted`、`StepCompleted`、`StepFailed`、`StepBlocked` 事件。

强化点：

- 新增或明确 `StepContract`：输入摘要、required context、allowed tools、max side effect、provider allowed、success evidence、failure policy。
- `allowed_tools` 必须真实约束工具调用，不能只依赖 `max_side_effect` 粗过滤。
- 每个 step 完成时必须生成 `StepEvidence`，包括 artifact refs、tool executions、provider usage、context refs。
- `success_signals` 至少支持规则化检查：是否产出指定 artifact、是否调用指定只读检查、是否获得 author approval。

涉及模块：

- `agent-harness-core/src/execution_plan.rs`
- `agent-harness-core/src/agent_loop.rs`
- `agent-harness-core/src/tool_registry.rs`
- `agent-harness-core/src/tool_executor.rs`
- `agent-writer-backend/src/writer_agent/kernel/run_loop.rs`

验收：

- 单测证明 step 级 `allowed_tools` 会隐藏未授权工具。
- 单测证明 write tool 在 read step 中不可见且不可执行。
- 单测证明缺少 required evidence 的 step 不能标记 completed。
- plan resume 时 terminal step 被跳过，running/blocked step 按 contract 恢复。

风险与非目标：

- 风险是 contract 过细导致旧任务适配成本高；先提供默认 contract，并逐步让关键任务显式化。
- 非目标是让模型自己决定 step contract；contract 由 runtime/task compiler 生成。

### A2 Tool Governance 与 Provider Governance 统一

目标：工具调用和 provider 调用进入同一套治理模型，统一看权限、成本、耗时、输入摘要、输出摘要和失败恢复。

当前基础：

- `ToolSideEffectLevel`、`ToolFilter`、`PermissionPolicy`、`ToolExecutionAuditSink` 已存在。
- provider budget guard、provider usage、TTFT/total duration 已存在。

交付物：

- `RuntimeCallRecord`：统一记录 tool/provider/context retrieval 调用。
- provider call 视为 `ProviderCall` side effect，进入 step evidence 和 trace。
- tool input 记录 redacted summary，避免泄露 key 或大文本。
- write/proposal/approval tool 必须绑定 approval context 或 proposal id。
- 失败时输出 remediation code：`refresh_inventory`、`request_approval`、`shrink_context`、`retry_transient`、`abort_unsafe_write`。

涉及模块：

- `agent-harness-core/src/tool_executor.rs`
- `agent-harness-core/src/provider/*`
- `agent-writer-backend/src/headless.rs`
- `agent-writer-backend/src/writer_agent/kernel/trace_recording/*`

验收：

- 单测证明 provider call 会产出 runtime call record。
- 单测证明 API key / secret 不进入 audit payload。
- 单测证明 approval-required tool 没有 approval context 时被拒绝，并给出 remediation。
- trace 中能按 step 聚合 tool count、provider count、duration、失败类型。

风险与非目标：

- 风险是 audit 数据太吵；默认 summary，详细 payload 只在 debug/report JSON 中保存。
- 非目标是完整 APM 系统；先满足 agent 调试和恢复。

### A3 Durable Checkpoint 统一语义

目标：把 checkpoint 从“存了某些长任务状态”升级成跨 AgentLoop、章节生成、sprint 的统一恢复机制。

当前基础：

- `long_task_checkpoints`、`supervised_sprint_checkpoints` 表已存在。
- headless 已有 latest checkpoint 和 resume candidates 查询。

统一 checkpoint 字段：

- `checkpoint_id`
- `task_id`
- `plan_id`
- `step_id`
- `phase`
- `input_hash`
- `context_hash`
- `artifact_refs`
- `tool_effects`
- `provider_usage`
- `budget_spent`
- `approval_refs`
- `resume_policy`

交付物：

- `AgentCheckpoint` 或等价类型，供 plan step 和 chapter generation 共用。
- 每个 step 边界写 checkpoint。
- provider call 前后可选写 checkpoint。
- save-prepared / write-before / write-after 三类写操作 checkpoint 明确区分。
- resume 时根据 `resume_policy` 判断 skip / rerun / require approval / abort。

涉及模块：

- `agent-harness-core/src/agent_loop.rs`
- `agent-writer-backend/src/writer_agent/memory/sprint_methods.in.rs`
- `agent-writer-backend/src/writer_agent/supervised_sprint.rs`
- `agent-writer-backend/src/chapter_generation/pipeline/main.in.rs`
- `agent-writer-backend/src/headless.rs`

验收：

- 单测证明 checkpoint roundtrip。
- 单测证明已完成 step resume 后不会重复执行。
- 单测证明 save-prepared checkpoint 恢复前会重新走 conflict check。
- 模拟 provider 中断后能从上一安全 step 恢复。

风险与非目标：

- 风险是恢复点不安全导致重复写入；所有 write resume 必须重新做 conflict/approval 检查。
- 非目标是任意中间 token 级恢复；按 step/phase 恢复即可。

### A4 Context Runtime 强化

目标：上下文不再只是拼接文本，而是可诊断、可追溯、可复现的输入系统。

当前基础：

- `ContextSourceReport` 已有 taxonomy、role、elapsed_ms、retrieval_status 字段。
- `ContextQualityReport` 已能输出 coverage、truncation risk、grounding quality、action codes。
- preflight 已能按 Critical/Supplement 阻断或警告。

交付物：

- 所有真实 context source 填充 `elapsed_ms`、`retrieval_status`、taxonomy、role。
- context pack 产物带 deterministic ordering 和 source priority。
- context hash 进入 checkpoint 和 step evidence。
- 低价值 source 截断，高价值 source 保留证据链。
- context quality recommendation 直接映射 step failure action：Critical -> blocked，Supplement -> request context supplement。

涉及模块：

- `agent-harness-core/src/context_pack.rs`
- `agent-harness-core/src/context_quality.rs`
- `agent-writer-backend/src/chapter_generation/context.in.rs`
- `agent-writer-backend/src/writer_agent/kernel/run_loop.rs`

验收：

- 单测证明相同输入 context pack 顺序稳定。
- 单测证明 source timeout/失败不会吞掉其他 source。
- preflight critical 会阻止 provider call。
- trace 中能看到 source 级 elapsed_ms 和 retrieval_status。

风险与非目标：

- 风险是 source timing 在测试中不稳定；断言非零/状态存在，不断言具体耗时。
- 非目标是并行写入上下文源；只读检索可以并行，写操作不并行。

### A5 Failure Taxonomy 与 Recovery Engine

目标：失败不再只是错误字符串，而是能驱动恢复动作的结构化分类。

失败类型：

- `provider_transient`：retry with backoff。
- `provider_budget`：request approval / shrink context。
- `context_missing`：request supplement。
- `context_overflow`：compact / shrink。
- `tool_permission`：request approval / surface proposal。
- `tool_schema`：repair args / refresh inventory。
- `save_conflict`：recheck revision / ask user / create new revision。
- `quality_gate`：targeted revision / strict block。
- `doom_loop`：stop with failure bundle。
- `unsafe_write`：abort。

交付物：

- `FailureKind`、`RecoveryDecision`、`RecoveryBundle` 结构。
- `StepFailureAction` 与 recovery decision 绑定，不再只靠 step 静态配置。
- FailureBundle 包含 completed steps、failed step、input context summary、tool/provider events、suggested action。
- headless/MCP 能返回 recovery options，而不是只返回 error。

涉及模块：

- `agent-harness-core/src/recovery.rs`
- `agent-harness-core/src/agent_loop.rs`
- `agent-writer-backend/src/writer_agent/task_receipt.rs`
- `agent-writer-backend/src/headless.rs`

验收：

- 单测覆盖每类 failure kind 到 recovery decision 的映射。
- provider 429/5xx -> retry；401/403 -> abort config/auth。
- context missing -> blocked + supplement actions。
- save conflict -> 不自动覆盖，必须 surface user choice。

风险与非目标：

- 风险是分类错误导致错误恢复；保守策略优先 stop/ask，不冒险写入。
- 非目标是自动解决所有失败；目标是给出正确下一步。

### A6 Observability 与 Runtime Report

目标：让性能和质量问题可定位，不靠翻散乱日志。

交付物：

- `AgentRunReport`：plan summary、step summary、tool/provider call timeline、context quality、budget、failure/recovery。
- provider latency p50/p90/p95、TTFT、total provider duration。
- 每 step provider calls、tool calls、retry count、duration。
- context retrieval duration、source missing/truncated count。
- compaction events 和 tokens saved。
- report 可导出 JSON，summary 可供 MCP/headless 展示。

涉及模块：

- `agent-harness-core/src/run_trace.rs`
- `agent-harness-core/src/agent_loop.rs`
- `agent-writer-backend/src/writer_agent/kernel/trace_recording/*`
- `agent-writer-backend/src/headless.rs`

验收：

- 单测证明 report 不包含 API key。
- 单测证明 failed run 也能输出 partial report。
- 真实或模拟 agent run 能输出 step-level timing summary。

风险与非目标：

- 风险是报告太大；默认 summary，详细 timeline 留 JSON。
- 非目标是实时 dashboard；先保证数据完整。

### A7 Agent Runtime Eval Harness

目标：用固定测试证明底层 agent runtime 不退化。

新增 eval 维度：

- step 级工具越权。
- provider budget 阻断。
- context missing preflight 阻断。
- transient provider retry。
- save conflict 不覆盖。
- checkpoint resume 不重复执行。
- doom loop stop。
- approval-required tool 不带 approval 被拒绝。
- failed run 输出 recovery bundle。

涉及模块：

- `agent-harness-core` tests
- `agent-writer-backend` integration tests
- `scripts/*` 可选新增 runtime eval 命令

验收：

- 新增 15-25 个 runtime tests。
- `cargo test --workspace` 覆盖核心底层退化。
- runtime report / failure bundle 有 snapshot 或结构断言。

风险与非目标：

- 风险是测试过慢；大部分用 mock provider/mock tool，不调用真实 API。
- 非目标是把真实长链路测试放进普通 CI。

### 执行顺序

1. A1 StepContract：先让计划变成真实约束。
2. A2 Tool/Provider Governance：统一调用审计和权限。
3. A5 Failure Taxonomy：让失败能驱动恢复。
4. A3 Durable Checkpoint：把恢复动作落到安全恢复点。
5. A4 Context Runtime：补齐输入系统诊断和可复现性。
6. A6 Runtime Report：统一输出可观测报告。
7. A7 Eval Harness：用 mock 场景锁住底层能力。

### 完成度重估规则

- A1 + A2 完成：agent 可约束性从 6/10 提升到 7/10。
- A5 + A3 完成：agent 可恢复性从 5/10 提升到 7.5/10。
- A4 完成：复杂任务输入可靠性从 6.5/10 提升到 8/10。
- A6 完成：性能与失败定位能力从 6/10 提升到 8/10。
- A7 完成：底层能力从“已有功能”变成“可回归保证”。

### 近期最小闭环

首轮不要铺太大，先做一个端到端薄切：

1. 给 chapter plan 的 preflight/draft/validate/save 四步生成显式 `StepContract`。
2. 让 draft step 只能看到 provider-call 能力，save step 才能看到 write 能力。
3. 每步完成产生 `StepEvidence`。
4. draft step provider transient failure 触发 retry；save conflict 触发 blocked，不自动覆盖。
5. 每步边界写 mock checkpoint。
6. failed run 输出 FailureBundle + partial AgentRunReport。

这个闭环跑通后，再把 checkpoint 持久化、context runtime、runtime eval matrix 逐步补齐。

## 2026-05-10 写作 Agent 底层能力强化计划

### 核心判断

通用 agent runtime 解决“任务能否可靠执行”；写作 agent runtime 解决“写作过程能否持续产出有效章节”。两者不同：前者关心 step、工具、checkpoint、失败恢复；后者关心写作合同、故事状态、设定约束、质量诊断、定向修订和长篇连续性。

当前写作链路已有 Craft Library、SceneCraftPlan、ChapterQualityReport、RevisionReport、Craft Memory、required_story_anchors、required_state_deltas、anchor_carry、style_drift、eval trend 等基础。下一步不是继续堆技法，而是把它们统一成写作运行时：

```text
WritingRunContract
  -> StoryContext Compiler
  -> SceneContract
  -> Draft Planner
  -> Draft Generation
  -> Quality Validator
  -> Revision Controller
  -> Story Ledger Update
  -> WritingRunReport
```

### 非目标

- 不把“文采”硬编码进底层 runtime。
- 不为某个世界观或题材做专项逻辑。
- 不让每次写作都走最重 strict 链路。
- 不把质量判断全部交给 LLM judge。
- 不让修订变成泛泛“润色一下”。

### W1 WritingRunContract

目标：每次写作任务先形成运行合同，明确任务边界、输出类型、允许动作和成功标准。

合同字段：

- `run_id`
- `task_kind`：draft / continue / revise / diagnose / plan / sprint。
- `output_kind`：chapter_text / scene_plan / revision_report / diagnostic / summary。
- `quality_mode`：fast / balanced / strict。
- `allowed_actions`：read_context / provider_draft / quality_check / targeted_revision / save。
- `success_criteria`：mission hit、length compliance、canon consistency、state delta coverage、style floor。
- `stop_conditions`：budget exceeded、hard canon violation、save conflict、approval required。

涉及模块：

- `agent-writer-backend/src/chapter_generation/types_and_utils.in.rs`
- `agent-writer-backend/src/chapter_generation/pipeline/main.in.rs`
- `agent-writer-backend/src/headless.rs`
- `forge-agent-mcp/src/tools.rs`

验收：

- 单测覆盖 fast / balanced / strict 三种 mode 的默认合同。
- 无合同的旧调用走兼容默认值。
- contract 能进入 trace/report，方便复盘。

风险与非目标：

- 风险是调用参数膨胀；对外只暴露少量模式，内部展开合同。
- 非目标是让用户手写完整 contract。

### W2 StoryContext Compiler

目标：把上下文从“资料拼接”升级成“写作上下文包”，为生成和校验提供同一份可追溯输入。

上下文包字段：

- chapter mission。
- 前文摘要和上一章尾部状态。
- 角色当前状态。
- 世界观 / canon constraints。
- promise ledger / foreshadowing state。
- required story anchors。
- required state deltas。
- author voice snapshot。
- Craft Memory。
- blocked reveals / allowed reveals。

涉及模块：

- `agent-writer-backend/src/chapter_generation/context.in.rs`
- `agent-writer-backend/src/chapter_generation/pipeline/context.in.rs`
- `agent-writer-backend/src/writer_agent/story_impact/*`
- `agent-writer-backend/src/writer_agent/memory.rs`

验收：

- 单测证明同一章节上下文可复现，source order 稳定。
- 缺关键 source 时 preflight 阻断或 warning。
- context report 能列出每类 source 的来源、预算、是否截断。

风险与非目标：

- 风险是 context 包过大；必须按 priority 和 budget 裁剪。
- 非目标是把所有项目资料都塞进 prompt。

### W3 SceneContract 标准化

目标：每章生成前明确“本章必须推进什么、不能触碰什么、要付出什么代价”。

合同字段：

- scene objective。
- opening state。
- conflict pressure。
- required choice / action。
- required state deltas。
- active canon constraints。
- required cost / consequence。
- promise progress target。
- ending hook。
- blocked reveals。

涉及模块：

- `agent-writer-backend/src/chapter_generation/craft_types.rs`
- `agent-writer-backend/src/chapter_generation/craft_prompt.rs`
- `agent-writer-backend/src/chapter_generation/context.in.rs`

验收：

- 单测证明 required state delta、canon constraint、promise target 能进入 SceneContract。
- prompt snapshot 证明模型看到的是 compact contract，而不是散乱资料。
- strict 模式下没有 scene objective 或 required delta 时 preflight warning/block。

风险与非目标：

- 风险是 contract 太硬导致正文僵硬；只放 1-3 个关键 delta 和 3-8 条关键规则。
- 非目标是自动决定整卷剧情。

### W4 Draft Planner

目标：正文生成前先形成轻量 scene plan，降低散、拖、重复和设定乱飞。

计划字段：

- opening image / opening state。
- scene objective。
- opposition / pressure。
- action chain。
- reveal / discovery。
- cost / consequence。
- ending hook。

涉及模块：

- `agent-writer-backend/src/chapter_generation/craft_prompt.rs`
- `agent-writer-backend/src/chapter_generation/pipeline/main.in.rs`
- `agent-writer-backend/src/bin/eval_runner.rs`

验收：

- 单测或 fixture 证明 draft plan 覆盖 SceneContract 的关键项。
- 正文 prompt 必须引用 plan 的 action chain 和 consequence。
- plan 不保存为 canon，只保存为 run artifact。

风险与非目标：

- 风险是多一次 provider call 增加成本；fast 模式可跳过，balanced/strict 启用。
- 非目标是复杂大纲规划，只做本章 scene plan。

### W5 Writing Quality Validator 统一

目标：把已有质量信号统一成写作 runtime 的标准检查层。

指标：

- `mission_hit`
- `canon_consistency`
- `state_delta_coverage`
- `promise_progress`
- `anchor_carry`
- `scene_causality`
- `ending_hook`
- `repetition`
- `new_information_density`
- `style_drift`
- `length_compliance`

涉及模块：

- `agent-writer-backend/src/chapter_generation/craft_quality.rs`
- `agent-writer-backend/src/writer_agent/anchor_carry.rs`
- `agent-writer-backend/src/writer_agent/author_voice.rs`
- `agent-writer-backend/src/bin/eval_runner.rs`

验收：

- ChapterQualityReport 明确区分 pass / warning / hard violation。
- 每个 metric 必须提供 evidence excerpt 或 insufficient evidence。
- strict 模式下 hard violation 触发 revision 或阻断；balanced 模式 warning。
- eval fixture 覆盖至少 mission/state/canon/repetition/new information 五类指标。

风险与非目标：

- 风险是假阳性影响创作；第一阶段 hard gate 只限明确规则冲突和保存风险。
- 非目标是用单一分数替代作者判断。

### W6 Revision Controller

目标：修订必须根据具体失败点定向执行，而不是泛泛润色。

修订输入：

- failed metrics。
- violation excerpts。
- source evidence。
- SceneContract。
- allowed edit scope。
- max revision attempts。

修订输出：

- `RevisionReport`
- 修订目标列表。
- before/after score。
- sentence-level target changes。
- 是否接受。
- 未解决问题。

涉及模块：

- `agent-writer-backend/src/chapter_generation/craft_quality.rs`
- `agent-writer-backend/src/chapter_generation/pipeline/main.in.rs`
- `agent-writer-backend/src/chapter_generation/craft_types.rs`

验收：

- 单测证明缺 state delta 时 revision prompt 要求补行动/选择/后果。
- 单测证明 canon violation 修订不引入新 unsupported claim。
- RevisionReport 记录 target -> text change 映射。
- 修订后若分数未提升，不自动覆盖原稿。

风险与非目标：

- 风险是循环修订；每类失败限制尝试次数，超过后输出报告给作者。
- 非目标是无限自动改到完美。

### W7 Story Ledger 持久化

目标：长篇连续写作不能只依赖摘要，必须记录跨章节状态变化。

Ledger 类型：

- character knowledge / belief / secret。
- character physical / emotional / resource state。
- relationship state。
- faction stance。
- promise open / advanced / paid / contradicted。
- canon rule triggered。
- cost paid / unpaid。
- world situation escalated / stabilized。

涉及模块：

- `agent-writer-backend/src/writer_agent/memory.rs`
- `agent-writer-backend/src/writer_agent/story_impact/*`
- `agent-writer-backend/src/chapter_generation/context.in.rs`
- `agent-writer-backend/src/writer_agent/supervised_sprint.rs`

验收：

- 每章保存后可生成 `StoryLedgerDelta`。
- 下一章 context 能读取上一章 delta。
- validator 能识别无解释状态回滚。
- 三十章 gate 报告能统计 state delta coverage。

风险与非目标：

- 风险是状态记录过多；只记录影响后续剧情/设定/承诺的状态。
- 非目标是完整知识图谱替代 Project Brain。

### W8 WritingRunReport

目标：每次写作 run 都可复盘，形成持续优化证据。

报告字段：

- WritingRunContract。
- StoryContext summary。
- SceneContract。
- DraftPlan。
- provider calls / tokens / latency。
- quality before/after。
- revision attempts。
- saved revision id。
- ledger updates。
- remaining warnings。

涉及模块：

- `agent-writer-backend/src/chapter_generation/types_and_utils.in.rs`
- `agent-writer-backend/src/chapter_generation/pipeline/main.in.rs`
- `agent-writer-backend/src/writer_agent/kernel/trace_recording/*`
- `reports/*`

验收：

- 单测证明报告不包含 API key。
- failed run 也能输出 partial WritingRunReport。
- 三章/三十章 gate 报告引用 WritingRunReport summary，而不是只给 pass/fail。

风险与非目标：

- 风险是报告过大；默认 summary，详细 artifact 分文件存储。
- 非目标是前端可视化，先保证结构数据完整。

### 执行顺序

1. W1 WritingRunContract：先明确每次写作运行边界。
2. W3 SceneContract：把章节目标、状态变化、规则约束标准化。
3. W2 StoryContext Compiler：让 context 稳定产出 SceneContract 所需输入。
4. W5 Quality Validator：把检查层统一起来。
5. W6 Revision Controller：让修订跟着失败点走。
6. W7 Story Ledger：把跨章节状态持久化。
7. W8 WritingRunReport：形成可复盘证据。
8. W4 Draft Planner：在 balanced/strict 中作为质量增强项接入。

### 完成度重估规则

- W1 + W3 完成：写作任务可控性从 6.5/10 提升到 7.5/10。
- W2 + W5 完成：章节质量可诊断性提升到 8/10。
- W6 完成：自动修订有效性提升到 8/10。
- W7 完成并进入长链路 gate：长篇连续性提升到 8.5/10。
- W8 完成：写作能力优化从主观判断转成可复盘证据。

### 近期最小闭环

首轮只做一个可验证薄切：

1. 为章节生成新增 `WritingRunContract`，默认 balanced。
2. 从现有 `required_story_anchors`、`required_state_deltas`、chapter mission 编译 `SceneContract`。
3. draft prompt 注入 compact SceneContract。
4. ChapterQualityReport 检查 mission hit、state delta coverage、anchor carry、length compliance。
5. Targeted revision 针对 state delta missing 或 anchor weak 做一次定向修订。
6. 输出 WritingRunReport summary，记录 context sources、quality before/after、revision decision。

这个闭环跑通后，再逐步接入 Draft Planner、Story Ledger、repetition/new information gate 和 strict 模式硬阻断。

## 2026-05-10 通用长篇故事工程能力计划

### 核心判断

最终目标不是让系统适配某一部作品，而是让系统掌握“长篇故事工程”的通用能力：吃下复杂项目资料，提取叙事发动机，拆成卷/篇章/单元，持续生成章节，维护状态账本，控制重复和漂移，并按卷审计推进。

高复杂故事样本只作为压力测试，不进入专属代码路径。系统不应写死任何题材术语、修炼体系、科幻制度、悬疑规则或具体作品名。

目标链路：

```text
Project Bible
  -> World / Story Assets
  -> Narrative Engine
  -> Series Architecture
  -> Unit Episode Pool
  -> SceneContract
  -> Draft / Validate / Revise
  -> Story State Ledger
  -> Volume Audit
  -> Long-Form Run Report
```

能力边界：

- 支持百万字级长篇：系统化写作辅助。
- 支持数百万字级项目：需要卷级审计和人工验收。
- 支持千万字潜力：系统负责项目管理、状态和生产辅助，不能承诺无人自动成书。

### L1 Project Bible Ingestion

目标：吃下任意项目资料，统一转成通用项目资产，而不是题材专属结构。

输入类型：

- 世界观。
- 故事核。
- 主角/反派设定。
- 势力结构。
- 升级体系或能力体系。
- 单元故事模板。
- 长线悬念。
- 主题母题。
- 卷级规划。
- 写作风格要求。

输出资产：

- `WorldAsset`
- `CharacterAsset`
- `FactionAsset`
- `RuleAsset`
- `ThemeAsset`
- `PlotEngineAsset`
- `SeriesArcAsset`
- `SuspenseAsset`
- `StyleAsset`

验收：

- 同一 ingest 流程能处理至少两个不同题材 fixture。
- 每条资产必须带 source evidence 和 approval status。
- 未 approved 的资产不能作为 hard canon。

风险与非目标：

- 风险是自动抽取误判；首轮允许半自动/人工批准。
- 非目标是一次性完美理解全项目。

### L2 Narrative Engine Extractor

目标：从故事核中提取“为什么这个故事可以持续生长”的发动机。

通用发动机类型：

- `unit_formula`：单元故事公式。
- `conflict_loop`：冲突循环。
- `reveal_ladder`：真相递进阶梯。
- `power_progression`：升级/能力递进。
- `cost_progression`：代价递进。
- `character_arc_loop`：人物弧光循环。
- `institutional_pressure`：制度/组织压力。
- `reader_reward_loop`：爽点/补偿循环。
- `long_suspense`：长线悬念。

交付物：

- `NarrativeEngine` 数据结构。
- 从 Project Bible assets 编译 engine。
- 每个 engine 绑定适用范围：全书、某卷、某角色、某类单元。
- SceneContract 可引用 engine，确保单章不是孤立生成。

验收：

- 单测证明 unit formula 能生成单元任务骨架。
- 单测证明 reveal ladder 能约束“本卷只揭示第 N 层真相”。
- eval fixture 覆盖至少两个题材的 narrative engine。

风险与非目标：

- 风险是 engine 变成模板化套路；每个 engine 只提供结构，不直接生成剧情细节。
- 非目标是自动替作者决定主题。

### L3 Series Architecture Manager

目标：管理百万字以上长篇结构，避免写到中后期散架。

结构层级：

- series / book。
- volume。
- arc。
- episode / case / mission / dungeon / campaign。
- chapter。

每层必须记录：

- narrative purpose。
- theme focus。
- main conflict。
- required reveal。
- character state target。
- world state target。
- open promises。
- closure requirements。

交付物：

- `SeriesArchitecture` 数据结构。
- 卷级路线和章节任务可互相追溯。
- 章节生成前能知道自己属于哪个 volume / arc / episode。
- 卷级审计能检查本卷是否完成 promised reveal 和状态变化。

验收：

- 单测证明 chapter mission 可追溯到 arc 和 volume purpose。
- 单测证明卷级 promise 未支付会进入 VolumeAudit warning。
- 长篇 fixture 至少包含 2 卷、4 arc、8 episode 的结构测试。

风险与非目标：

- 风险是结构管理过重；短篇/中篇项目可关闭 series architecture。
- 非目标是强制所有作品使用同一章法。

### L4 Unit Episode Pool

目标：把“大故事”拆成可生产、可轮换、可回流主线的单元任务库。

通用单元类型：

- case。
- dungeon。
- investigation。
- political crisis。
- battle campaign。
- relationship rupture。
- training breakthrough。
- discovery / reveal。
- rescue / escape。
- negotiation / trial。

每个单元记录：

- surface problem。
- hidden cause。
- active rule / system。
- key character choice。
- cost / consequence。
- mainline contribution。
- reader reward。
- follow-up hooks。

交付物：

- `EpisodeTemplate`
- `EpisodeInstance`
- episode -> chapter mission 编译。
- episode 完成后回写 Story State Ledger。

验收：

- 单测证明 episode 能拆成多个 chapter missions。
- 单测证明 episode 必须有 mainline contribution，否则 warning。
- repetition gate 能比较 episode 结构，识别连续重复单元。

风险与非目标：

- 风险是单元模板化；模板只控制功能，不生成固定情节。
- 非目标是自动批量生成整卷案件并直接采用。

### L5 Story State Ledger 三层化

目标：千万字潜力的核心不是更长上下文，而是多层状态账本。

三层 ledger：

- chapter ledger：本章改变了什么。
- arc / episode ledger：本单元解决了什么、欠下什么。
- volume ledger：本卷主题、真相、势力、人物状态如何变化。

记录类型：

- character knowledge。
- character loss / gain。
- relationship state。
- faction stance。
- resource ownership。
- rule triggered。
- cost paid / unpaid。
- promise status。
- reveal level。
- world situation。
- reader knowledge。

交付物：

- `StoryLedgerDelta`
- `EpisodeLedgerSummary`
- `VolumeLedgerSummary`
- ledger query API：按角色、势力、promise、rule、volume 查询。
- generation context 从 ledger 编译 required deltas 和 forbidden regressions。

验收：

- 单测证明同一状态能从 chapter delta 汇总到 volume summary。
- 单测证明状态回滚需要解释，否则 validator warning。
- 三十章/长链路 gate 统计 stateDeltaCoverage 和 unresolvedPromiseCount。

风险与非目标：

- 风险是账本过细；只记录影响后续剧情、设定、承诺的状态。
- 非目标是替代全文阅读，ledger 是导航和约束。

### L6 Repetition And Drift Gate

目标：长篇最怕重复和漂移，必须在结构层面检测，而不是只看文字相似。

检查项：

- opening repetition。
- conflict repetition。
- episode formula repetition。
- character reaction repetition。
- exposition repetition。
- reveal stagnation。
- theme overstatement。
- power progression stall。
- relationship arc stall。
- mainline drift。

交付物：

- `RepetitionDriftReport`
- chapter-level、episode-level、volume-level 三种视角。
- gate 按 quality mode 区分：fast 不启用，balanced warning，strict 可阻断。

验收：

- 单测覆盖文字不同但结构重复的单元。
- 单测覆盖必要呼应不误判为重复。
- 长链路报告能列出重复单元、重复冲突类型、主线停滞点。

风险与非目标：

- 风险是误伤有意复调；第一阶段只 warning，不自动改。
- 非目标是判断文学风格好坏。

### L7 Volume Audit

目标：每卷结束必须审计，防止百万字后主线、角色和设定债务失控。

审计内容：

- 本卷主题是否完成。
- 本卷主线推进了什么。
- 本卷认知翻转是否成立。
- 哪些 promise paid / advanced / still open。
- 角色状态如何改变。
- 势力格局如何改变。
- 世界规则是否新增或改写。
- 是否有重复单元过多。
- 下一卷驱动力是否明确。

交付物：

- `VolumeAuditReport`
- unresolved debt list。
- next volume setup。
- recommended recap / compression。
- canon conflict / promise debt / repetition summary。

验收：

- 单测证明未支付卷级 promise 会进入 debt list。
- 单测证明没有 next volume driver 会 warning。
- VolumeAuditReport 可被下一卷的 StoryContext Compiler 使用。

风险与非目标：

- 风险是审计结论主观；必须引用 ledger、promises、chapter reports。
- 非目标是自动重写整卷。

### L8 Long-Form Capacity Modes

目标：按作品规模启用不同治理强度，避免所有项目承担千万字级成本。

模式：

- `short_form`：30 万字以内，轻量 context + chapter quality。
- `standard_long_form`：30-150 万字，SceneContract + StoryLedger。
- `epic_long_form`：150-500 万字，SeriesArchitecture + VolumeAudit。
- `mega_series`：500 万字以上，三层 ledger + 强人工验收 + volume strict。

交付物：

- 项目级 `long_form_mode`。
- 不同模式默认启用不同 validator / audit / report。
- MCP/headless 暴露模式，但保留 conservative defaults。

验收：

- 单测覆盖不同 long_form_mode 的默认 gate 配置。
- fast/balanced/strict 与 long_form_mode 不冲突。
- mega_series 模式不会默认在普通章节中启用所有重检查，只在卷/单元边界启用强审计。

风险与非目标：

- 风险是模式过多；对用户只暴露清晰档位，内部细节默认。
- 非目标是承诺无人自动写千万字。

### 执行顺序

1. L1 Project Bible Ingestion：先有项目资产。
2. L2 Narrative Engine Extractor：明确故事如何持续生长。
3. L3 Series Architecture Manager：建立卷/篇章/单元层级。
4. L4 Unit Episode Pool：把长篇拆成可生产单元。
5. L5 Story State Ledger 三层化：保证长期连续性。
6. L6 Repetition And Drift Gate：防止重复和主线漂移。
7. L7 Volume Audit：卷级清账和下一卷驱动。
8. L8 Long-Form Capacity Modes：按规模控制成本。

### 完成度重估规则

- L1 + L2 完成：复杂项目资料可理解/可发动能力到 7/10。
- L3 + L4 完成：百万字长篇结构管理能力到 8/10。
- L5 完成：跨章节连续性到 8.5/10。
- L6 + L7 完成：数百万字级长篇可控性到 8/10。
- L8 完成：不同规模项目具备可配置治理强度。

### 规模判断

在该路线完成后，合理能力边界如下：

- 30-50 万字：系统可较稳辅助，balanced 为主。
- 100-200 万字：系统主战场，关键章和卷尾走 strict。
- 300-500 万字：可支撑，但必须有卷级审计、ledger 清账和人工验收。
- 500 万字以上：进入 mega_series 管理模式，系统负责结构、状态、审计和生产辅助。
- 千万字级：只能承诺具备项目管理与生产辅助潜力，不能承诺无人自动稳定成书。

### 近期最小闭环

首轮不要直接做千万字系统，先做一个通用长篇薄切：

1. 从两个题材 fixture 中抽取 Project Bible assets。
2. 为每个 fixture 定义 1 个 NarrativeEngine。
3. 建 1 个 volume、2 个 arc、4 个 episode。
4. 每个 episode 编译 2-3 个 chapter missions。
5. 生成 chapter-level ledger delta，并汇总成 volume summary。
6. VolumeAuditReport 能指出未支付 promise、重复 episode 类型和下一卷驱动力缺口。

## 2026-05-10 能力与性能对齐计划

### 核心风险

如果 P14-P20、Agent 底层强化、写作 Agent 底层强化、通用长篇工程能力全部完成，但性能工程没有同步升级，系统会进入“能力跟上了，性能没跟上”的阶段：能做复杂长篇治理，但每章耗时、调用次数、上下文构建和报告写入成本过高，不适合高频创作。

当前证据已经提示这个风险：真实 API 分拆验证能通过，但单命令串行全量存在 30 分钟超时风险；三十章 gate 能跑通，但不是高吞吐链路。

能力路线会增加：

- Project Bible ingestion。
- WorldAsset / CanonConstraint 编译。
- NarrativeEngine / SeriesArchitecture。
- SceneContract。
- DraftPlanner。
- QualityValidator。
- TargetedRevision。
- StoryLedger。
- VolumeAudit。
- WritingRunReport。

这些能力必须配套 provider 调用预算、增量缓存、后台任务、编译产物存储和性能报告，否则系统会强但慢。

### 非目标

- 不牺牲保存安全、授权、canon 校验来换速度。
- 不把 strict 质量链路默认套到所有章节。
- 不把真实长链路测试放进普通 CI。
- 不做不可解释的黑盒缓存；缓存必须有 input hash / source revision。

### Perf1 Provider Call Budget

目标：每种模式明确最多能调用几次模型，防止能力链路无限膨胀。

默认预算：

- `fast`：最多 1 次 provider call，draft only。
- `balanced`：最多 2 次 provider calls，draft + optional targeted revision。
- `strict`：最多 4 次 provider calls，planner + draft + validator/revision + final check。
- `volume_audit`：batch / async，不阻塞单章生成。
- `mega_series_audit`：后台任务，必须可暂停/恢复。

交付物：

- `ProviderCallBudget` 数据结构。
- WritingRunContract 引用 provider call budget。
- 超预算时进入 `quality_deferred` 或 `requires_approval`，而不是静默继续调用。
- WritingRunReport 记录 planned_calls / actual_calls / deferred_checks。

涉及模块：

- `agent-writer-backend/src/writer_agent/provider_budget.rs`
- `agent-writer-backend/src/chapter_generation/pipeline/main.in.rs`
- `agent-writer-backend/src/chapter_generation/types_and_utils.in.rs`
- `agent-writer-backend/src/headless.rs`

验收：

- 单测覆盖 fast/balanced/strict 的 provider call 上限。
- balanced 模式 revision 超预算时能降级为 warning/deferred。
- strict 模式超预算需要 approval 或明确失败。

风险与非目标：

- 风险是预算过紧压低质量；允许项目配置覆盖，但必须显式。
- 非目标是精确计费系统；目标是控制调用次数和延迟。

### Perf2 Incremental Context Cache

目标：每章只重算变化部分，不重复编译整份世界观、ledger 和项目结构。

缓存对象：

- approved WorldAssets。
- CanonConstraints。
- NarrativeEngine。
- SeriesArchitecture。
- StoryLedger snapshots。
- Volume summaries。
- promise ledger snapshots。
- author voice snapshot。
- Craft Memory prompt snippets。
- retrieval result sets。

缓存键：

- project_id。
- source_revision。
- asset_version。
- chapter_id / volume_id。
- input_hash。
- quality_mode。

交付物：

- `CompiledContextCache` 或等价缓存层。
- context build 先查缓存，miss 时增量重建。
- source revision 改变时只失效相关资产。
- report 记录 cache hit/miss 和 saved_ms 估算。

涉及模块：

- `agent-writer-backend/src/chapter_generation/context.in.rs`
- `agent-writer-backend/src/chapter_generation/pipeline/context.in.rs`
- `agent-writer-backend/src/brain_service/*`
- `agent-writer-backend/src/writer_agent/memory.rs`

验收：

- 单测证明相同 source_revision 二次构建命中缓存。
- 单测证明修改单个 source 只失效相关 compiled artifacts。
- context build report 能输出 cache hit/miss。

风险与非目标：

- 风险是脏缓存污染生成；所有缓存必须带 source revision 和 hash。
- 非目标是跨项目共享复杂缓存。

### Perf3 Async Background Jobs

目标：把重任务后台化，不阻塞每章生成。

后台任务：

- Project Bible ingestion。
- WorldAsset extraction / approval preparation。
- VolumeAudit。
- repetition scan。
- ledger compression。
- long-form capacity report。
- eval trend generation。
- Project Brain rebuild。

交付物：

- `BackgroundJob` 数据结构：job_id、kind、status、progress、checkpoint、result_ref、error。
- job 可暂停、恢复、取消。
- 单章生成只读取已完成的 compiled result；未完成时降级 warning。
- headless/MCP 能查询 job status。

涉及模块：

- `agent-writer-backend/src/headless.rs`
- `agent-writer-backend/src/writer_agent/memory.rs`
- `forge-agent-mcp/src/tools.rs`
- 可复用 long task checkpoint 存储。

验收：

- 单测证明后台任务 checkpoint roundtrip。
- 单测证明章节生成不会等待未完成 VolumeAudit。
- MCP/headless 能列出 pending/running/completed job。

风险与非目标：

- 风险是后台结果与当前 source revision 不一致；读取时必须校验 revision。
- 非目标是复杂分布式队列；本地持久任务即可。

### Perf4 Compiled Artifact Store

目标：把“每次运行临时编译”改为“编译产物可持久复用”。

编译产物：

- `CompiledWorldBible`
- `CompiledCanonConstraints`
- `CompiledNarrativeEngine`
- `CompiledSeriesArchitecture`
- `CompiledSceneContract`
- `CompiledLedgerSummary`
- `CompiledVolumeAudit`

交付物：

- artifact metadata：artifact_id、kind、input_refs、input_hash、created_at、expires_at、quality_mode。
- artifact body 存 JSON。
- artifact 可按 project/source/chapter/volume 查询。
- stale artifact 不硬用，只能作为 fallback warning。

涉及模块：

- `agent-writer-backend/src/writer_agent/memory.rs`
- `agent-writer-backend/src/chapter_generation/context.in.rs`
- `agent-writer-backend/src/headless.rs`

验收：

- 单测证明 artifact 写入/读取/失效。
- 单测证明 stale artifact 不进入 hard constraints。
- WritingRunReport 能引用 artifact ids，而不是内联所有大对象。

风险与非目标：

- 风险是存储膨胀；需要 TTL 或按 run/volume 清理策略。
- 非目标是独立对象存储系统。

### Perf5 Validator Cost Tiering

目标：质量检查分层，避免每章跑所有 validator。

分层：

- Tier 0：纯确定性检查，长度、空文本、保存安全。
- Tier 1：轻量启发式，anchor carry、state delta keyword、basic repetition。
- Tier 2：结构化规则，canon constraint、scene contract、ledger regression。
- Tier 3：LLM-assisted judge / deep critique，只在 strict 或审计任务启用。

模式映射：

- fast：Tier 0。
- balanced：Tier 0 + Tier 1 + 部分 Tier 2。
- strict：Tier 0 + Tier 1 + Tier 2，必要时 Tier 3。
- volume_audit：Tier 2 + Tier 3 batch。

交付物：

- `ValidatorTier` 标记。
- QualityValidator 根据 quality_mode 和 long_form_mode 选择检查项。
- report 标记 skipped / deferred validators。

验收：

- 单测证明 fast 不触发 LLM-assisted validator。
- 单测证明 strict 会启用 canon/state/repetition gate。
- report 能说明哪些 validator 被跳过以及原因。

风险与非目标：

- 风险是低模式漏检；低模式必须明确输出 deferred risk。
- 非目标是让 fast 模式保证长篇一致性。

### Perf6 Strict Mode Sampling

目标：长篇不需要每章 strict，但需要周期性抽检和关键点强检。

策略：

- 每 N 章做一次 strict sample。
- 卷首、卷尾、重大 reveal、状态大变更章节强制 strict。
- 连续 warning 超阈值时自动升级 strict。
- 作者可手动标记关键章 strict。

交付物：

- `StrictSamplingPolicy`
- chapter metadata 支持 critical flag。
- WritingRunContract 根据 sampling policy 自动选择 mode。
- VolumeAudit 使用 sampling 结果评估长期风险。

验收：

- 单测证明每 N 章触发 strict。
- 单测证明关键章强制 strict。
- 单测证明连续 warnings 会升级下一章 quality mode。

风险与非目标：

- 风险是抽检漏掉局部问题；关键章和 warning escalation 补足。
- 非目标是完全替代人工审稿。

### Perf7 Performance Report 与 SLO

目标：性能问题可定位，并给出明确 SLO。

报告字段：

- total_run_ms。
- context_build_ms。
- provider_calls。
- provider_latency_ms total / p50 / max。
- validator_ms。
- revision_ms。
- ledger_read_ms / ledger_write_ms。
- artifact_cache_hit_rate。
- report_write_ms。
- deferred_checks。

建议 SLO：

- fast 单章：1 provider call，低额外开销。
- balanced 单章：1-2 provider calls，context/cache 命中后本地开销低于 provider 时间的 20%。
- strict 单章：2-4 provider calls，报告完整但允许更慢。
- volume audit：后台执行，不阻塞普通生成。

交付物：

- `PerformanceReport`
- WritingRunReport 引用 PerformanceReport。
- 长链路 gate 输出 per-chapter performance summary。
- 如果本地开销超过 provider 时间 50%，报告 warning。

验收：

- 单测证明 PerformanceReport 不包含敏感信息。
- 模拟 run 能输出各 phase ms。
- 长链路报告能聚合 p50/p90 chapter duration。

风险与非目标：

- 风险是测试耗时不稳定；单测断言字段存在，不断言具体毫秒。
- 非目标是精确压测平台。

### 执行顺序

1. Perf1 Provider Call Budget：先控制最贵成本。
2. Perf5 Validator Cost Tiering：防止所有检查默认全开。
3. Perf7 Performance Report：先看清楚慢在哪里。
4. Perf2 Incremental Context Cache：降低每章重复编译成本。
5. Perf4 Compiled Artifact Store：让缓存可持久复用。
6. Perf3 Async Background Jobs：把卷级/长链路重任务后台化。
7. Perf6 Strict Mode Sampling：长篇按风险抽检和升级。

### 完成度重估规则

- Perf1 + Perf5 完成：单章成本可控性从 5/10 提升到 7/10。
- Perf7 完成：性能可观测性从 6/10 提升到 8/10。
- Perf2 + Perf4 完成：复杂项目 context 构建成本显著下降。
- Perf3 完成：卷级审计和长链路分析不再阻塞普通章节生成。
- Perf6 完成：长篇 strict 质量保障与吞吐之间形成可调平衡。

### 近期最小闭环

首轮只做能立刻防止“能力强但慢”的薄切：

1. WritingRunContract 增加 provider call budget。
2. QualityValidator 增加 tier，并按 fast/balanced/strict 选择。
3. WritingRunReport 增加 provider_calls、context_build_ms、validator_ms、revision_ms。
4. balanced 模式最多一次 draft + 一次必要 revision。
5. strict 模式允许 planner/checker，但必须记录 deferred/actual calls。
6. 长链路报告输出每章 provider call count 和本地 phase timing。
