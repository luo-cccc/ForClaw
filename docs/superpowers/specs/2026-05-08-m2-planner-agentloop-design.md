# M2: Planner-Aware AgentLoop Design

## Summary

将 `TaskPacket` 从"任务说明/trace 结构"升级为"可执行计划"的输入。新增 `ExecutionPlan` 数据结构、`AgentLoop::run_with_plan()` 方法、以及基于 TaskScope 的 plan 模板编译器。现有 `run()` 方法不变——`run_with_plan` 是新增并行入口。

## Decisions

| 决策 | 选择 |
|------|------|
| TaskPacket vs ExecutionPlan 关系 | 独立模块 —— TaskPacket 不变，ExecutionPlan 编译器读取 TaskPacket |
| Step 粒度 | 粗粒度（3-5 steps），每步允许多轮 LLM+tool call |
| 集成方式 | `run_with_plan()` 并行于 `run()`，不取代 |

---

## Section 1: ExecutionPlan Data Structures

**新文件：** `agent-harness-core/src/execution_plan.rs`

```rust
use serde::{Deserialize, Serialize};
use crate::tool_registry::ToolSideEffectLevel;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionPlan {
    pub plan_id: String,
    pub task_id: String,
    pub steps: Vec<ExecutionStep>,
    pub status: PlanStatus,
    pub created_at_ms: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionStep {
    pub step_id: String,
    pub index: usize,
    pub goal: String,
    pub required_context: Vec<String>,
    pub allowed_tools: Vec<String>,
    pub max_side_effect: ToolSideEffectLevel,
    pub success_signals: Vec<String>,
    pub on_failure: StepFailureAction,
    pub status: StepStatus,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanStatus {
    Pending,
    Running,
    Completed,
    Failed { failed_step: String, reason: String },
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepStatus {
    #[default]
    Pending,
    Active,
    Completed { evidence: Vec<String> },
    Failed { reason: String },
    Skipped { reason: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepFailureAction {
    Stop,
    Retry { max_retries: u32 },
    Skip,
}
```

**从 TaskPacket 编译 ExecutionPlan：**

```rust
pub fn compile_plan(task: &TaskPacket, plan_id: &str, now_ms: u64) -> ExecutionPlan {
    let steps = match task.scope {
        TaskScope::Chapter => chapter_plan_steps(),
        TaskScope::Selection => selection_plan_steps(),
        TaskScope::CursorWindow => cursor_plan_steps(),
        TaskScope::Project => project_plan_steps(),
        _ => default_plan_steps(),
    };

    ExecutionPlan {
        plan_id: plan_id.to_string(),
        task_id: task.id.clone(),
        steps,
        status: PlanStatus::Pending,
        created_at_ms: now_ms,
    }
}
```

**Step 模板（每种 TaskScope 对应的 steps）：**

| Scope | Steps |
|-------|-------|
| Chapter | preflight(Read,Stop) → draft(ProviderCall,Retry1) → validate(Read,Skip) → save(Write,Stop) |
| Selection | gather(Read,Stop) → process(ProviderCall,Skip) → apply(Write,Stop) |
| CursorWindow | observe(Read,Stop) → respond(ProviderCall,Skip) |
| Project | gather(Read,Stop) → analyze(ProviderCall,Retry1) → plan(Read,Skip) → execute(Write,Stop) |
| Book/Custom | gather(Read,Stop) → execute(Write,Stop) |

---

## Section 2: AgentLoop Integration

**改动文件：** `agent-harness-core/src/agent_loop.rs`

### New AgentLoopEvent variants

```rust
    #[serde(rename = "plan_started")]
    PlanStarted {
        plan_id: String,
        steps: Vec<String>,
    },
    #[serde(rename = "step_started")]
    StepStarted {
        step_id: String,
        index: usize,
        goal: String,
    },
    #[serde(rename = "step_completed")]
    StepCompleted {
        step_id: String,
        evidence: Vec<String>,
    },
    #[serde(rename = "step_failed")]
    StepFailed {
        step_id: String,
        reason: String,
        action: String,
    },
    #[serde(rename = "plan_completed")]
    PlanCompleted {
        plan_id: String,
        steps_completed: usize,
        steps_failed: usize,
    },
```

### New method: `run_with_plan()`

```rust
pub async fn run_with_plan(
    &mut self,
    plan: ExecutionPlan,
    user_message: &str,
) -> Result<String, String> {
    let step_goals: Vec<String> = plan.steps.iter().map(|s| s.goal.clone()).collect();
    self.emit(AgentLoopEvent::PlanStarted {
        plan_id: plan.plan_id.clone(),
        steps: step_goals,
    });

    let mut final_text = String::new();
    let mut steps_completed = 0usize;
    let mut steps_failed = 0usize;

    for mut step in plan.steps {
        step.status = StepStatus::Active;
        self.emit(AgentLoopEvent::StepStarted {
            step_id: step.step_id.clone(),
            index: step.index,
            goal: step.goal.clone(),
        });

        // Constrain tool filter to this step
        let saved_filter = self.config.tool_filter.clone();
        self.config.tool_filter = Some(ToolFilter {
            intent: self.config.tool_filter.as_ref().and_then(|f| f.intent.clone()),
            include_requires_approval: false,
            include_disabled: false,
            max_side_effect_level: Some(step.max_side_effect),
            required_tags: Vec::new(),
        });

        // Execute round with step-scoped tools
        let step_result = self.run(user_message, true, true).await;
        self.config.tool_filter = saved_filter;

        match step_result {
            Ok(text) => {
                steps_completed += 1;
                final_text = text;
                self.emit(AgentLoopEvent::StepCompleted {
                    step_id: step.step_id.clone(),
                    evidence: vec![],
                });
            }
            Err(e) => {
                steps_failed += 1;
                match step.on_failure {
                    StepFailureAction::Skip => {
                        self.emit(AgentLoopEvent::StepFailed {
                            step_id: step.step_id.clone(),
                            reason: e.clone(),
                            action: "skip".into(),
                        });
                        continue;
                    }
                    StepFailureAction::Stop => {
                        self.emit(AgentLoopEvent::StepFailed {
                            step_id: step.step_id.clone(),
                            reason: e.clone(),
                            action: "stop".into(),
                        });
                        self.emit(AgentLoopEvent::PlanCompleted {
                            plan_id: plan.plan_id.clone(),
                            steps_completed,
                            steps_failed,
                        });
                        return Err(e);
                    }
                    StepFailureAction::Retry { max_retries: _ } => {
                        // Retry once — if fails again, fall through to Stop
                        let retry_result = self.run(user_message, true, true).await;
                        if let Ok(text) = retry_result {
                            steps_completed += 1;
                            final_text = text;
                            self.emit(AgentLoopEvent::StepCompleted {
                                step_id: step.step_id.clone(),
                                evidence: vec![],
                            });
                            continue;
                        }
                        steps_failed += 1;
                        self.emit(AgentLoopEvent::StepFailed {
                            step_id: step.step_id.clone(),
                            reason: format!("Retry failed: {}", e),
                            action: "stop".into(),
                        });
                        self.emit(AgentLoopEvent::PlanCompleted {
                            plan_id: plan.plan_id.clone(),
                            steps_completed,
                            steps_failed,
                        });
                        return Err(e);
                    }
                }
            }
        }
    }

    self.emit(AgentLoopEvent::PlanCompleted {
        plan_id: plan.plan_id.clone(),
        steps_completed,
        steps_failed,
    });

    Ok(final_text)
}
```

### Existing `run()` — unchanged

`run()` 保持原有签名和行为。`run_with_plan()` 是新增入口。已有的无 plan 调用不受影响。

---

## Section 3: Writer Agent Plan Templates

**改动文件：** `agent-writer-backend/src/writer_agent/kernel/run_loop.rs`

**计划化改造：** 在写作任务 preflight 结束后，调用 `compile_plan(task)` 构建 ExecutionPlan，然后走 `agent_loop.run_with_plan()`。

```rust
// 在 writer_agent/kernel/run_loop.rs 中
// preflight 完成后，根据 TaskPacket 编译 plan
let plan = compile_execution_plan(&task_packet, &run_id, now_ms);
let result = agent_loop.run_with_plan(plan, &user_instruction).await;
```

**`compile_execution_plan()` 函数：**

```rust
fn compile_execution_plan(task: &TaskPacket, run_id: &str, now_ms: u64) -> ExecutionPlan {
    use crate::router::Intent;
    use crate::task_packet::TaskScope;
    use crate::tool_registry::ToolSideEffectLevel;
    use agent_harness_core::execution_plan::*;

    let steps = match (task.intent.as_ref(), task.scope) {
        (Some(Intent::GenerateContent), TaskScope::Chapter) => vec![
            step(0, "preflight: assemble context and contracts", ToolSideEffectLevel::Read, StepFailureAction::Stop),
            step(1, "draft: generate chapter prose", ToolSideEffectLevel::ProviderCall, StepFailureAction::Retry { max_retries: 1 }),
            step(2, "validate: run quality diagnostics", ToolSideEffectLevel::Read, StepFailureAction::Skip),
            step(3, "save: revision-safe chapter save", ToolSideEffectLevel::Write, StepFailureAction::Stop),
        ],
        (Some(Intent::AnalyzeText), _) => vec![
            step(0, "gather: load context sources", ToolSideEffectLevel::Read, StepFailureAction::Stop),
            step(1, "diagnose: scan for issues", ToolSideEffectLevel::Read, StepFailureAction::Skip),
            step(2, "plan: produce review artifact", ToolSideEffectLevel::ProviderCall, StepFailureAction::Retry { max_retries: 1 }),
        ],
        _ => vec![
            step(0, "observe: collect editor state", ToolSideEffectLevel::Read, StepFailureAction::Stop),
            step(1, "respond: generate response", ToolSideEffectLevel::ProviderCall, StepFailureAction::Skip),
        ],
    };

    ExecutionPlan {
        plan_id: format!("plan-{}", run_id),
        task_id: task.id.clone(),
        steps,
        status: PlanStatus::Pending,
        created_at_ms: now_ms,
    }
}

fn step(index: usize, goal: &str, max_side_effect: ToolSideEffectLevel, on_failure: StepFailureAction) -> ExecutionStep {
    ExecutionStep {
        step_id: format!("step-{}", index),
        index,
        goal: goal.into(),
        max_side_effect,
        on_failure,
        ..Default::default()
    }
}
```

---

## Files Summary

| 文件 | 操作 | 职责 |
|------|------|------|
| `agent-harness-core/src/execution_plan.rs` | 创建 | ExecutionPlan, ExecutionStep, PlanStatus, StepStatus, StepFailureAction, compile_plan() |
| `agent-harness-core/src/lib.rs` | 修改 | 注册 `pub mod execution_plan` + 重导出 |
| `agent-harness-core/src/agent_loop.rs` | 修改 | 新增 5 个 AgentLoopEvent 变体 + `run_with_plan()` 方法 |
| `agent-harness-core/src/run_trace.rs` | 修改 | 新增 AgentRunEventKind 变体 |
| `agent-writer-backend/src/writer_agent/kernel/run_loop.rs` | 修改 | 新增 `compile_execution_plan()` + 写作任务走 plan |

## Acceptance Criteria

```powershell
cargo fmt --check
cargo check --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p agent-harness-core  # 不退化
cargo test -p agent-writer --lib   # 不退化
cargo test -p forge-agent-mcp      # 不退化
```

- `ExecutionPlan` 可序列化/反序列化
- `compile_plan()` 输入空 TaskPacket 不 panic，返回默认 plan
- `run_with_plan()` 步骤按序执行，StepStarted/Completed 事件正确 emit
- `run()` 行为不变，已有测试全通过
- Fail-Stop 步失败后 PlanCompleted 事件正确 emit，后续步骤不执行
- Fail-Skip 步失败后继续下一步

## Out of Scope

- 步骤间上下文传递和 compaction（M3）
- 从失败步骤恢复（M4）
- 写作任务 plan 的质量指标集成（已有 ChapterQualityReport，M3 接上）
