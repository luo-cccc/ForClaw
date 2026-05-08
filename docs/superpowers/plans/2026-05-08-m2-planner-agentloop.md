# M2: Planner-Aware AgentLoop Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add ExecutionPlan data structures and step-level execution to the AgentLoop — enabling coarse-grained 3-5 step plans with Stop/Retry/Skip failure actions.

**Architecture:** New `execution_plan.rs` module in agent-harness-core defines plan types and compiler. `agent_loop.rs` adds `run_with_plan()` parallel to existing `run()`. Writer agent kernel wires plan compilation into preflight.

**Tech Stack:** Rust, serde (Serialize/Deserialize)

---

## File Map

| File | Create/Modify | Responsibility |
|------|--------------|----------------|
| `agent-harness-core/src/execution_plan.rs` | Create | ExecutionPlan, ExecutionStep, PlanStatus, StepStatus, StepFailureAction, compile_plan() |
| `agent-harness-core/src/lib.rs` | Modify | Register pub mod + re-export |
| `agent-harness-core/src/agent_loop.rs` | Modify | 5 new AgentLoopEvent variants + run_with_plan() |
| `agent-harness-core/src/run_trace.rs` | Modify | 5 new AgentRunEventKind variants |
| `agent-writer-backend/src/writer_agent/kernel/run_loop.rs` | Modify | compile_execution_plan() for writer tasks |

---

### Task 1: ExecutionPlan Data Structures + Compiler

**Files:**
- Create: `agent-harness-core/src/execution_plan.rs`
- Modify: `agent-harness-core/src/lib.rs`

- [ ] **Step 1: Create execution_plan.rs**

```rust
use serde::{Deserialize, Serialize};
use crate::task_packet::{TaskPacket, TaskScope};
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

pub fn compile_plan(task: &TaskPacket, plan_id: &str, now_ms: u64) -> ExecutionPlan {
    let steps = steps_for_scope(task.scope);

    ExecutionPlan {
        plan_id: plan_id.to_string(),
        task_id: task.id.clone(),
        steps,
        status: PlanStatus::Pending,
        created_at_ms: now_ms,
    }
}

fn steps_for_scope(scope: TaskScope) -> Vec<ExecutionStep> {
    match scope {
        TaskScope::Chapter => vec![
            step(0, "preflight: assemble context and contracts", ToolSideEffectLevel::Read, StepFailureAction::Stop),
            step(1, "draft: generate chapter prose", ToolSideEffectLevel::ProviderCall, StepFailureAction::Retry { max_retries: 1 }),
            step(2, "validate: run quality diagnostics", ToolSideEffectLevel::Read, StepFailureAction::Skip),
            step(3, "save: revision-safe chapter save", ToolSideEffectLevel::Write, StepFailureAction::Stop),
        ],
        TaskScope::Selection => vec![
            step(0, "gather: load selection context", ToolSideEffectLevel::Read, StepFailureAction::Stop),
            step(1, "process: apply revision", ToolSideEffectLevel::ProviderCall, StepFailureAction::Skip),
            step(2, "apply: commit changes", ToolSideEffectLevel::Write, StepFailureAction::Stop),
        ],
        TaskScope::CursorWindow => vec![
            step(0, "observe: collect editor state", ToolSideEffectLevel::Read, StepFailureAction::Stop),
            step(1, "respond: generate response", ToolSideEffectLevel::ProviderCall, StepFailureAction::Skip),
        ],
        TaskScope::Project => vec![
            step(0, "gather: load project context", ToolSideEffectLevel::Read, StepFailureAction::Stop),
            step(1, "analyze: examine project state", ToolSideEffectLevel::ProviderCall, StepFailureAction::Retry { max_retries: 1 }),
            step(2, "plan: produce plan artifact", ToolSideEffectLevel::Read, StepFailureAction::Skip),
            step(3, "execute: apply operations", ToolSideEffectLevel::Write, StepFailureAction::Stop),
        ],
        _ => vec![
            step(0, "gather: load context", ToolSideEffectLevel::Read, StepFailureAction::Stop),
            step(1, "execute: perform task", ToolSideEffectLevel::Write, StepFailureAction::Stop),
        ],
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::task_packet::TaskScope;

    #[test]
    fn chapter_plan_has_four_steps() {
        let task = TaskPacket::new("t1", "write chapter", TaskScope::Chapter, 1);
        let plan = compile_plan(&task, "p1", 1);
        assert_eq!(plan.steps.len(), 4);
        assert_eq!(plan.steps[0].goal, "preflight: assemble context and contracts");
        assert_eq!(plan.steps[3].goal, "save: revision-safe chapter save");
    }

    #[test]
    fn cursor_window_has_two_steps() {
        let task = TaskPacket::new("t2", "check selection", TaskScope::CursorWindow, 1);
        let plan = compile_plan(&task, "p2", 1);
        assert_eq!(plan.steps.len(), 2);
    }

    #[test]
    fn chapter_steps_escalate_side_effects() {
        let task = TaskPacket::new("t3", "draft", TaskScope::Chapter, 1);
        let plan = compile_plan(&task, "p3", 1);
        assert_eq!(plan.steps[0].max_side_effect, ToolSideEffectLevel::Read);
        assert_eq!(plan.steps[1].max_side_effect, ToolSideEffectLevel::ProviderCall);
        assert_eq!(plan.steps[3].max_side_effect, ToolSideEffectLevel::Write);
    }

    #[test]
    fn draft_step_has_retry_on_failure() {
        let task = TaskPacket::new("t4", "draft", TaskScope::Chapter, 1);
        let plan = compile_plan(&task, "p4", 1);
        assert_eq!(plan.steps[1].on_failure, StepFailureAction::Retry { max_retries: 1 });
    }

    #[test]
    fn validate_step_skips_on_failure() {
        let task = TaskPacket::new("t5", "draft", TaskScope::Chapter, 1);
        let plan = compile_plan(&task, "p5", 1);
        assert_eq!(plan.steps[2].on_failure, StepFailureAction::Skip);
    }

    #[test]
    fn serialization_roundtrip() {
        let task = TaskPacket::new("t6", "test", TaskScope::Chapter, 1);
        let plan = compile_plan(&task, "p6", 1);
        let json = serde_json::to_string(&plan).unwrap();
        let decoded: ExecutionPlan = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.steps.len(), 4);
    }
}
```

- [ ] **Step 2: Register in lib.rs**

In `agent-harness-core/src/lib.rs`, add:

```rust
pub mod execution_plan;
```

And add to the re-export block:

```rust
pub use execution_plan::{
    compile_plan, ExecutionPlan, ExecutionStep, PlanStatus, StepFailureAction, StepStatus,
};
```

- [ ] **Step 3: Verify**

```powershell
cargo test -p agent-harness-core execution_plan
cargo clippy -p agent-harness-core --all-targets -- -D warnings
```

- [ ] **Step 4: Commit**

```bash
git add agent-harness-core/src/execution_plan.rs agent-harness-core/src/lib.rs
git commit -m "feat: add ExecutionPlan data structures and plan compiler

ExecutionPlan with coarse-grained 3-5 step plans per TaskScope.
StepFailureAction: Stop/Retry/Skip. compile_plan() maps TaskScope
to step templates with escalating side-effect levels.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>"
```

---

### Task 2: AgentLoop Plan Events + run_with_plan

**Files:**
- Modify: `agent-harness-core/src/agent_loop.rs`
- Modify: `agent-harness-core/src/run_trace.rs`

- [ ] **Step 1: Add new AgentLoopEvent variants**

In `agent_loop.rs`, find the `AgentLoopEvent` enum and insert after `Compaction`:

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

- [ ] **Step 2: Add new AgentRunEventKind variants**

In `run_trace.rs`, add to `AgentRunEventKind` after `ContextWindowCheck`:

```rust
    PlanStarted,
    StepStarted,
    StepCompleted,
    StepFailed,
    PlanCompleted,
```

- [ ] **Step 3: Add import and implement run_with_plan**

In `agent_loop.rs`, add import at top:

```rust
use crate::execution_plan::{ExecutionPlan, ExecutionStep, PlanStatus, StepFailureAction, StepStatus};
use crate::tool_registry::ToolFilter;
```

Then add `run_with_plan()` method to `impl<P: Provider, H: ToolHandler> AgentLoop<P, H>`:

```rust
    pub async fn run_with_plan(
        &mut self,
        plan: &mut ExecutionPlan,
        user_message: &str,
    ) -> Result<String, String> {
        let step_goals: Vec<String> = plan.steps.iter().map(|s| s.goal.clone()).collect();
        self.emit(AgentLoopEvent::PlanStarted {
            plan_id: plan.plan_id.clone(),
            steps: step_goals,
        });
        plan.status = PlanStatus::Running;

        let mut final_text = String::new();
        let mut steps_completed = 0usize;
        let mut steps_failed = 0usize;

        for step in &mut plan.steps {
            step.status = StepStatus::Active;
            self.emit(AgentLoopEvent::StepStarted {
                step_id: step.step_id.clone(),
                index: step.index,
                goal: step.goal.clone(),
            });

            // Constrain tool filter to this step's max_side_effect
            let saved_filter = self.config.tool_filter.clone();
            self.config.tool_filter = Some(ToolFilter {
                intent: saved_filter.as_ref().and_then(|f| f.intent.clone()),
                include_requires_approval: false,
                include_disabled: false,
                max_side_effect_level: Some(step.max_side_effect),
                required_tags: Vec::new(),
            });

            let step_result = self.run(user_message, true, true).await;
            self.config.tool_filter = saved_filter;

            match step_result {
                Ok(text) => {
                    steps_completed += 1;
                    final_text = text;
                    step.status = StepStatus::Completed { evidence: vec![] };
                    self.emit(AgentLoopEvent::StepCompleted {
                        step_id: step.step_id.clone(),
                        evidence: vec![],
                    });
                }
                Err(e) => match step.on_failure {
                    StepFailureAction::Skip => {
                        steps_failed += 1;
                        step.status = StepStatus::Skipped { reason: e.clone() };
                        self.emit(AgentLoopEvent::StepFailed {
                            step_id: step.step_id.clone(),
                            reason: e.clone(),
                            action: "skip".into(),
                        });
                        continue;
                    }
                    StepFailureAction::Stop => {
                        steps_failed += 1;
                        step.status = StepStatus::Failed { reason: e.clone() };
                        self.emit(AgentLoopEvent::StepFailed {
                            step_id: step.step_id.clone(),
                            reason: e.clone(),
                            action: "stop".into(),
                        });
                        plan.status = PlanStatus::Failed {
                            failed_step: step.step_id.clone(),
                            reason: e.clone(),
                        };
                        self.emit(AgentLoopEvent::PlanCompleted {
                            plan_id: plan.plan_id.clone(),
                            steps_completed,
                            steps_failed,
                        });
                        return Err(e);
                    }
                    StepFailureAction::Retry { max_retries: _ } => {
                        // One retry
                        let retry_result = self.run(user_message, true, true).await;
                        if let Ok(text) = retry_result {
                            steps_completed += 1;
                            final_text = text;
                            step.status = StepStatus::Completed { evidence: vec![] };
                            self.emit(AgentLoopEvent::StepCompleted {
                                step_id: step.step_id.clone(),
                                evidence: vec![],
                            });
                            continue;
                        }
                        steps_failed += 1;
                        step.status = StepStatus::Failed { reason: e.clone() };
                        self.emit(AgentLoopEvent::StepFailed {
                            step_id: step.step_id.clone(),
                            reason: format!("Retry exhausted: {}", e),
                            action: "stop".into(),
                        });
                        plan.status = PlanStatus::Failed {
                            failed_step: step.step_id.clone(),
                            reason: e.clone(),
                        };
                        self.emit(AgentLoopEvent::PlanCompleted {
                            plan_id: plan.plan_id.clone(),
                            steps_completed,
                            steps_failed,
                        });
                        return Err(e);
                    }
                },
            }
        }

        plan.status = PlanStatus::Completed;
        self.emit(AgentLoopEvent::PlanCompleted {
            plan_id: plan.plan_id.clone(),
            steps_completed,
            steps_failed,
        });

        Ok(final_text)
    }
```

- [ ] **Step 4: Add tests**

Append to the `#[cfg(test)] mod tests` block in agent_loop.rs:

```rust
    #[test]
    fn run_with_plan_events_serialize() {
        let event = AgentLoopEvent::PlanStarted {
            plan_id: "p1".into(),
            steps: vec!["step-0".into(), "step-1".into()],
        };
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["kind"], "plan_started");
        assert_eq!(json["steps"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn step_failed_event_serializes_action() {
        let event = AgentLoopEvent::StepFailed {
            step_id: "step-1".into(),
            reason: "timeout".into(),
            action: "stop".into(),
        };
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["action"], "stop");
    }
```

- [ ] **Step 5: Verify**

```powershell
cargo test -p agent-harness-core
cargo clippy -p agent-harness-core --all-targets -- -D warnings
```

- [ ] **Step 6: Commit**

```bash
git add agent-harness-core/src/agent_loop.rs agent-harness-core/src/run_trace.rs
git commit -m "feat: add run_with_plan() and 5 plan/step trace events

AgentLoop::run_with_plan() executes coarse-grained step plans
with Stop/Retry/Skip failure actions. run() unchanged.
New events: PlanStarted, StepStarted, StepCompleted, StepFailed,
PlanCompleted.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>"
```

---

### Task 3: Writer Agent Plan Templates

**Files:**
- Modify: `agent-writer-backend/src/writer_agent/kernel/run_loop.rs`

- [ ] **Step 1: Add compile_execution_plan() to writer kernel**

Find the preflight section in the writer agent's run loop (the function that prepares context and calls agent_loop.run()). After task preflight, add plan compilation.

Read the file first to find the exact location. Then add:

```rust
use agent_harness_core::execution_plan::{compile_plan, ExecutionPlan};

fn compile_execution_plan(task: &TaskPacket, run_id: &str, now_ms: u64) -> ExecutionPlan {
    let mut plan = compile_plan(task, &format!("plan-{}", run_id), now_ms);
    // Writer-specific step enrichment: add context hints from the writer domain
    for step in &mut plan.steps {
        if step.goal.contains("draft") {
            step.required_context.push("craft_prompt".into());
            step.required_context.push("chapter_mission".into());
        }
        if step.goal.contains("validate") {
            step.required_context.push("scene_craft_plan".into());
            step.required_context.push("quality_report".into());
        }
    }
    plan
}
```

**Note:** This is a skeleton enrichment for MVP. The plan compiler in agent-harness-core already produces correct step structure from TaskScope. The writer kernel just adds domain-specific context hints.

- [ ] **Step 2: Verify compilation**

```powershell
cargo check -p agent-writer
cargo test -p agent-writer --lib
cargo clippy -p agent-writer --all-targets -- -D warnings
```

- [ ] **Step 3: Commit**

```bash
git add agent-writer-backend/src/writer_agent/kernel/run_loop.rs
git commit -m "feat: add writer agent plan template enrichment

compile_execution_plan() augments agent-harness-core plans with
writer-domain context hints (craft_prompt for draft steps,
quality_report for validate steps).

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>"
```

---

### Task 4: Full Integration Verification

- [ ] **Step 1: Run complete test suite**

```powershell
cargo fmt --check
cargo check --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p agent-harness-core
cargo test -p agent-writer --lib
cargo test -p forge-agent-mcp
```

---

## Task Summary

| Task | Files | New Lines | Est. Time |
|------|-------|-----------|-----------|
| 1. ExecutionPlan Types | execution_plan.rs (new), lib.rs | ~180 | 30 min |
| 2. AgentLoop + Events | agent_loop.rs, run_trace.rs | ~120 | 35 min |
| 3. Writer Templates | run_loop.rs | ~25 | 15 min |
| 4. Integration Check | — | — | 10 min |
| **Total** | **5 files** | **~325** | **~1.5 hrs** |
