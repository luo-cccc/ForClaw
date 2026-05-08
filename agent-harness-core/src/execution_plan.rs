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

#[derive(Debug, Clone, Serialize, Deserialize)]
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

impl Default for ExecutionStep {
    fn default() -> Self {
        Self {
            step_id: String::new(),
            index: 0,
            goal: String::new(),
            required_context: Vec::new(),
            allowed_tools: Vec::new(),
            max_side_effect: ToolSideEffectLevel::Read,
            success_signals: Vec::new(),
            on_failure: StepFailureAction::Stop,
            status: StepStatus::default(),
        }
    }
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
            step(
                0,
                "preflight: assemble context and contracts",
                ToolSideEffectLevel::Read,
                StepFailureAction::Stop,
            ),
            step(
                1,
                "draft: generate chapter prose",
                ToolSideEffectLevel::ProviderCall,
                StepFailureAction::Retry { max_retries: 1 },
            ),
            step(
                2,
                "validate: run quality diagnostics",
                ToolSideEffectLevel::Read,
                StepFailureAction::Skip,
            ),
            step(
                3,
                "save: revision-safe chapter save",
                ToolSideEffectLevel::Write,
                StepFailureAction::Stop,
            ),
        ],
        TaskScope::Selection => vec![
            step(
                0,
                "gather: load selection context",
                ToolSideEffectLevel::Read,
                StepFailureAction::Stop,
            ),
            step(
                1,
                "process: apply revision",
                ToolSideEffectLevel::ProviderCall,
                StepFailureAction::Skip,
            ),
            step(
                2,
                "apply: commit changes",
                ToolSideEffectLevel::Write,
                StepFailureAction::Stop,
            ),
        ],
        TaskScope::CursorWindow => vec![
            step(
                0,
                "observe: collect editor state",
                ToolSideEffectLevel::Read,
                StepFailureAction::Stop,
            ),
            step(
                1,
                "respond: generate response",
                ToolSideEffectLevel::ProviderCall,
                StepFailureAction::Skip,
            ),
        ],
        TaskScope::Project => vec![
            step(
                0,
                "gather: load project context",
                ToolSideEffectLevel::Read,
                StepFailureAction::Stop,
            ),
            step(
                1,
                "analyze: examine project state",
                ToolSideEffectLevel::ProviderCall,
                StepFailureAction::Retry { max_retries: 1 },
            ),
            step(
                2,
                "plan: produce plan artifact",
                ToolSideEffectLevel::Read,
                StepFailureAction::Skip,
            ),
            step(
                3,
                "execute: apply operations",
                ToolSideEffectLevel::Write,
                StepFailureAction::Stop,
            ),
        ],
        _ => vec![
            step(
                0,
                "gather: load context",
                ToolSideEffectLevel::Read,
                StepFailureAction::Stop,
            ),
            step(
                1,
                "execute: perform task",
                ToolSideEffectLevel::Write,
                StepFailureAction::Stop,
            ),
        ],
    }
}

fn step(
    index: usize,
    goal: &str,
    max_side_effect: ToolSideEffectLevel,
    on_failure: StepFailureAction,
) -> ExecutionStep {
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
mod execution_plan_tests {
    use super::*;
    use crate::task_packet::TaskScope;

    #[test]
    fn chapter_plan_has_four_steps() {
        let task = TaskPacket::new("t1", "write chapter", TaskScope::Chapter, 1);
        let plan = compile_plan(&task, "p1", 1);
        assert_eq!(plan.steps.len(), 4);
        assert_eq!(
            plan.steps[0].goal,
            "preflight: assemble context and contracts"
        );
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
        assert_eq!(
            plan.steps[1].max_side_effect,
            ToolSideEffectLevel::ProviderCall
        );
        assert_eq!(plan.steps[3].max_side_effect, ToolSideEffectLevel::Write);
    }

    #[test]
    fn draft_step_has_retry_on_failure() {
        let task = TaskPacket::new("t4", "draft", TaskScope::Chapter, 1);
        let plan = compile_plan(&task, "p4", 1);
        assert_eq!(
            plan.steps[1].on_failure,
            StepFailureAction::Retry { max_retries: 1 }
        );
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
