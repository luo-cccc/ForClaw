# Forge Agent Development Plan

## 目标

把当前 Forge Agent 从“工程约束强的写作垂直内核”推进到“更稳、更会规划、更容易扩展的长篇写作 Agent Runtime”。短期重点不是堆功能，而是补齐路由、规划、预算、恢复和评测闭环，让模型能力可以被稳定放大。

## 当前判断

当前内核的优势在于 MCP 暴露面、写作项目上下文组装、记忆账本、审批边界、章节生成安全检查和测试覆盖。主要短板在于意图路由偏规则化，AgentLoop 缺少任务级计划执行状态，模型成本估算偏粗，失败恢复策略还不够体系化，垂直写作质量缺少可重复评测集。

当前项目已收敛为 Headless-only 运行形态：MCP stdio server 是唯一支持入口，`agent-writer` 作为 headless backend library 被 `forge-agent-mcp` 调用，不再维护 Tauri 桌面运行时。

## 优先级

### P0: 稳定性与可观测性

1. 固化端到端健康检查
   - 覆盖 MCP initialize、tools/list、forge_status、forge_ask_agent 最小请求、章节读写安全路径。
   - 增加一个无需真实 provider 的 mock provider smoke test。
   - 验收：`cargo test -p agent-harness-core`、`cargo test -p agent-writer --lib`、`cargo test -p forge-agent-mcp` 持续通过。

2. 完善运行轨迹
   - 在 AgentLoop 事件里明确记录每轮 intent、tool inventory 摘要、上下文 token 估算、provider guard 决策。
   - 将 provider 失败分类、重试次数、压缩触发原因写入 trace。
   - 验收：`forge_trace` 能还原一次 agent run 的关键决策链。

3. 强化错误 envelope
   - 统一 backend/tool/provider/budget/permission/context_overflow 错误 kind。
   - 所有 write-sensitive 工具失败时必须包含下一步 remediation。
   - 验收：MCP structuredContent 中 `ok=false` 的错误可被调度器稳定分类。

### P1: 路由与任务计划

1. 升级 intent router
   - 保留当前关键词规则作为 fast path。
   - 增加可选 LLM router 或 schema-based classifier，用于混合意图、模糊写作请求和计划类任务。
   - 输出 intent、confidence、reason、fallback_rule。
   - 验收：复杂中文请求能被稳定分到 ManualRequest、PlanningReview、ContinuityDiagnostic、ChapterGeneration 等任务。

2. 引入任务级 ExecutionPlan
   - 将 `TaskPacket` 转换为可执行计划，不只作为 trace 结构。
   - 每个计划步骤声明所需上下文、允许工具、副作用等级、成功信号。
   - AgentLoop 执行时按 step 更新状态，而不是只有 round 计数。
   - 验收：一次 run 可以报告当前执行到哪一步、为何调用某个工具、是否满足成功标准。

3. 增加只读并行检索阶段
   - 对 Project Brain、lorebook、outline、chapter context 的只读检索并行化。
   - 写操作仍保持串行审批。
   - 验收：PlanningReview 和 ManualRequest 的上下文准备延迟下降，且 trace 中可见各来源耗时。

### P2: 上下文与预算模型

1. 改进 token 估算
   - 为不同 provider/model 配置 tokens-per-char、tool schema overhead、system prompt overhead。
   - 将估算值和 provider 实际 usage 做对比，形成校准指标。
   - 验收：常见中文长上下文请求的估算误差明显收敛，预算审批误报减少。

2. 上下文包质量评分
   - 为 ContextPack 增加 source coverage、truncation risk、missing required source、story grounding quality。
   - 对关键来源被截断或丢弃时给出明确 warning。
   - 验收：preflight 能告诉作者“为什么这次上下文不够好”，而不是只给字符数。

3. Prompt cache 与 Context Spine 优化
   - 扩展现有 Context Spine 指纹，区分 frozen/stable/dynamic prompt 区域。
   - 对重复 PlanningReview、ContinuityDiagnostic、ChapterGeneration 提供缓存命中统计。
   - 验收：trace 中能看到 prefix churn 来源和 cache hit/miss 原因。

### P3: 失败恢复与自我修正

1. 标准化恢复策略
   - 为 provider timeout、rate limit、JSON parse failure、tool denial、context overflow 定义恢复动作。
   - 恢复动作包括缩小上下文、降低输出预算、切换只读计划、请求预算审批、生成失败证据包。
   - 验收：常见失败不会只返回字符串错误，而是返回可执行恢复建议。

2. 增加 run-level recovery mode
   - 在 AgentLoop 达到 max rounds 或 tool doom loop 时，触发一次总结型恢复响应。
   - 输出已完成事项、阻塞点、可重试参数。
   - 验收：调度器可以从失败 run 继续，而不是重头开始。

3. 强化写操作落盘确认
   - 所有 TextInsert/TextReplace/ledger write 都要明确 Proposed -> Approved -> Applied -> DurablySaved。
   - 对缺少 durable save 的成功响应降级为 pending。
   - 验收：用户不会误以为未保存操作已经写入项目。

### P4: 写作质量评测

1. 建立中文长篇评测集
   - 覆盖连续性、人物口吻、伏笔推进、章节任务达成、改写不越界、设定冲突。
   - 使用小型 fixture 项目，不依赖真实用户数据。
   - 验收：每次核心 prompt 或上下文策略变更都可跑回归评测。

2. 增加章节生成质量指标
   - 统计 mission hit、promise advanced、canon conflict、length compliance、repetition risk、settlement quality。
   - 与 post-write diagnostics 和 settlement delta 对齐。
   - 验收：章节生成不是只看长度和保存成功，而能看到故事层面的结果。

3. 加入人工反馈闭环
   - 将用户接受、拒绝、编辑、忽略 proposal 的行为转为 memory reliability 信号。
   - 区分风格偏好、canon 候选、剧情承诺、正文改写的反馈权重。
   - 验收：重复被拒绝的建议会被抑制，反复接受的偏好会进入稳定上下文。

## 里程碑

### Milestone 1: Kernel Observability

范围：P0 全部。

验收命令：

```powershell
cargo fmt --check
cargo test -p agent-harness-core
cargo test -p agent-writer --lib
cargo test -p forge-agent-mcp
```

交付物：

- 更完整的 run trace。
- 更稳定的错误分类。
- MCP smoke test。

### Milestone 2: Planner-Aware AgentLoop

范围：P1 的 router、ExecutionPlan、只读并行检索。

验收标准：

- Agent run 能展示计划步骤和当前状态。
- 复杂中文请求有置信度和路由解释。
- 只读上下文检索具备耗时统计。

### Milestone 3: Budget and Context Quality

范围：P2 全部。

验收标准：

- provider budget 估算有模型级校准配置。
- preflight 能报告上下文质量风险。
- Context Spine/cache 指标可在 trace 中查看。

### Milestone 4: Recovery and Evaluation

范围：P3 与 P4 核心项。

验收标准：

- 常见失败具备 structured remediation。
- 有一套可重复运行的中文写作质量 fixture。
- 章节生成质量能被自动诊断和回归比较。

## 建议改动入口

- `agent-harness-core/src/router.rs`: intent router 升级入口。
- `agent-harness-core/src/agent_loop.rs`: 计划执行、恢复策略、事件增强入口。
- `agent-harness-core/src/provider/openai_compat.rs`: provider usage、token 估算、重试遥测入口。
- `agent-writer-backend/src/writer_agent/kernel/run_loop.rs`: preflight、TaskPacket、写作任务准备入口。
- `agent-writer-backend/src/writer_agent/context/assembly.in.rs`: ContextPack 质量评分与预算报告入口。
- `forge-agent-mcp/src/main.rs`: MCP 工具契约、错误 envelope、smoke test 入口。

## 风险

1. 路由升级如果完全依赖 LLM，会增加延迟和成本；需要保留规则 fast path。
2. 并行检索只能用于只读阶段，写操作并行会破坏审批和 revision safety。
3. 质量评测如果没有固定 fixture，很容易退化成主观判断。
4. 预算模型如果只做静态估算，不和真实 usage 校准，会继续误报。

## 下一步

优先从 Milestone 1 开始。先增强 trace 和错误分类，再改 router 和 planner。这样后续每一次智能增强都有可观测证据，不会靠感觉判断效果。
