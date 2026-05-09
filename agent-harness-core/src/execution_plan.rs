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

impl ExecutionStep {
    pub fn is_terminal(&self) -> bool {
        matches!(
            self.status,
            StepStatus::Completed { .. } | StepStatus::Failed { .. } | StepStatus::Skipped { .. }
        )
    }

    pub fn is_runnable(&self) -> bool {
        matches!(self.status, StepStatus::Ready | StepStatus::Planned)
    }

    pub fn is_blocked(&self) -> bool {
        matches!(self.status, StepStatus::Blocked { .. })
    }
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
    Planned,
    Ready,
    Running,
    Blocked {
        reason: String,
        context_summary: Vec<String>,
        recovery_suggestion: String,
    },
    Completed {
        evidence: Vec<String>,
    },
    Failed {
        reason: String,
        input_context_summary: Vec<String>,
        recovery_suggestion: String,
    },
    Skipped {
        reason: String,
    },
    /// Legacy alias — deserialized as Running.
    Active,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepFailureAction {
    Stop,
    Retry { max_retries: u32 },
    Skip,
    RequestContextSupplement { sources: Vec<String> },
    PauseForApproval,
    Abort,
}

impl ExecutionPlan {
    /// Find the first step that is not in a terminal state.
    /// Returns `None` if all steps are terminal (Completed, Failed, or Skipped).
    pub fn first_runnable_step(&self) -> Option<&ExecutionStep> {
        self.steps.iter().find(|s| !s.is_terminal())
    }

    /// Initialize all non-terminal steps to `Ready`.
    pub fn mark_ready(&mut self) {
        for step in &mut self.steps {
            if matches!(step.status, StepStatus::Planned | StepStatus::Active) {
                step.status = StepStatus::Ready;
            }
        }
    }

    /// Count steps by terminal status.
    pub fn summary(&self) -> (usize, usize, usize) {
        let completed = self
            .steps
            .iter()
            .filter(|s| matches!(s.status, StepStatus::Completed { .. }))
            .count();
        let failed = self
            .steps
            .iter()
            .filter(|s| matches!(s.status, StepStatus::Failed { .. }))
            .count();
        let skipped = self
            .steps
            .iter()
            .filter(|s| matches!(s.status, StepStatus::Skipped { .. }))
            .count();
        (completed, failed, skipped)
    }
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

    #[test]
    fn new_steps_default_to_planned() {
        let task = TaskPacket::new("t7", "test", TaskScope::Chapter, 1);
        let plan = compile_plan(&task, "p7", 1);
        for step in &plan.steps {
            assert_eq!(step.status, StepStatus::Planned);
        }
    }

    #[test]
    fn first_runnable_step_skips_terminal() {
        let task = TaskPacket::new("t8", "test", TaskScope::Chapter, 1);
        let mut plan = compile_plan(&task, "p8", 1);
        plan.steps[0].status = StepStatus::Completed { evidence: vec![] };
        plan.steps[1].status = StepStatus::Completed { evidence: vec![] };
        let runnable = plan.first_runnable_step();
        assert!(runnable.is_some());
        assert_eq!(runnable.unwrap().index, 2);
    }

    #[test]
    fn resume_finds_no_runnable_when_all_terminal() {
        let task = TaskPacket::new("t9", "test", TaskScope::Chapter, 1);
        let mut plan = compile_plan(&task, "p9", 1);
        for step in &mut plan.steps {
            step.status = StepStatus::Completed { evidence: vec![] };
        }
        assert!(plan.first_runnable_step().is_none());
        assert_eq!(plan.summary(), (4, 0, 0));
    }

    #[test]
    fn blocked_step_is_not_terminal() {
        let step = ExecutionStep {
            status: StepStatus::Blocked {
                reason: "awaiting approval".to_string(),
                context_summary: vec![],
                recovery_suggestion: "wait".to_string(),
            },
            ..Default::default()
        };
        assert!(!step.is_terminal());
        assert!(step.is_blocked());
    }

    #[test]
    fn step_failure_action_serialization_roundtrip() {
        let actions = vec![
            StepFailureAction::Abort,
            StepFailureAction::PauseForApproval,
            StepFailureAction::RequestContextSupplement {
                sources: vec!["canon".to_string()],
            },
        ];
        for action in actions {
            let json = serde_json::to_string(&action).unwrap();
            let decoded: StepFailureAction = serde_json::from_str(&json).unwrap();
            assert_eq!(action, decoded);
        }
    }
}
