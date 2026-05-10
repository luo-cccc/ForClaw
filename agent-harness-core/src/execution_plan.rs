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
    #[serde(default)]
    pub step_state: ExecutionStepState,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub recovery_action: Option<StepRecoveryAction>,
}

impl ExecutionStep {
    pub fn is_terminal(&self) -> bool {
        matches!(
            self.step_state,
            ExecutionStepState::Completed
                | ExecutionStepState::Failed
                | ExecutionStepState::Skipped
        ) || matches!(
            self.status,
            StepStatus::Completed { .. } | StepStatus::Failed { .. } | StepStatus::Skipped { .. }
        )
    }

    pub fn is_runnable(&self) -> bool {
        matches!(
            self.step_state,
            ExecutionStepState::Ready | ExecutionStepState::Planned
        ) || matches!(self.status, StepStatus::Ready | StepStatus::Planned)
    }

    pub fn is_blocked(&self) -> bool {
        matches!(self.step_state, ExecutionStepState::Blocked)
            || matches!(self.status, StepStatus::Blocked { .. })
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
            step_state: ExecutionStepState::default(),
            recovery_action: None,
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
/// Simple lifecycle state for an execution step, without payload data.
/// Kept in sync with `StepStatus` by `ExecutionPlan` methods.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum ExecutionStepState {
    #[default]
    Planned,
    Ready,
    Running,
    Blocked,
    Completed,
    Failed,
    Skipped,
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

/// Runtime-chosen recovery action when a step fails.
/// Distinct from `StepFailureAction` which is a compile-time policy in the plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StepRecoveryAction {
    Retry,
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
                step.step_state = ExecutionStepState::Ready;
            }
        }
    }

    /// Count steps by terminal status.
    pub fn summary(&self) -> (usize, usize, usize) {
        let completed = self
            .steps
            .iter()
            .filter(|s| {
                matches!(s.step_state, ExecutionStepState::Completed)
                    || matches!(s.status, StepStatus::Completed { .. })
            })
            .count();
        let failed = self
            .steps
            .iter()
            .filter(|s| {
                matches!(s.step_state, ExecutionStepState::Failed)
                    || matches!(s.status, StepStatus::Failed { .. })
            })
            .count();
        let skipped = self
            .steps
            .iter()
            .filter(|s| {
                matches!(s.step_state, ExecutionStepState::Skipped)
                    || matches!(s.status, StepStatus::Skipped { .. })
            })
            .count();
        (completed, failed, skipped)
    }

    /// Resume execution from a specific step index.
    /// Marks all non-terminal steps before `index` as `Skipped`,
    /// and the step at `index` as `Ready`. Steps after `index` are left unchanged.
    pub fn resume_from_step(&mut self, index: usize) {
        for step in &mut self.steps {
            if step.index < index {
                if !step.is_terminal() {
                    step.status = StepStatus::Skipped {
                        reason: "resumed from later step".to_string(),
                    };
                    step.step_state = ExecutionStepState::Skipped;
                    step.recovery_action = None;
                }
            } else if step.index == index {
                step.status = StepStatus::Ready;
                step.step_state = ExecutionStepState::Ready;
                step.recovery_action = None;
            }
        }
    }

    /// Mark the first `Running` step as `Failed` and attach a recovery action.
    /// Returns `true` if a Running step was found and marked, `false` otherwise.
    pub fn fail_current_step(&mut self, recovery: StepRecoveryAction) -> bool {
        for step in &mut self.steps {
            if step.step_state == ExecutionStepState::Running
                || matches!(step.status, StepStatus::Running)
            {
                let recovery_suggestion = match &recovery {
                    StepRecoveryAction::Retry => "attempt retry".to_string(),
                    StepRecoveryAction::Skip => "skip to next step".to_string(),
                    StepRecoveryAction::RequestContextSupplement { sources } => {
                        format!("supplement context from: {:?}", sources)
                    }
                    StepRecoveryAction::PauseForApproval => "await approval".to_string(),
                    StepRecoveryAction::Abort => "abort execution".to_string(),
                };
                step.status = StepStatus::Failed {
                    reason: format!("step failed with recovery: {:?}", recovery),
                    input_context_summary: vec![step.goal.clone()],
                    recovery_suggestion,
                };
                step.step_state = ExecutionStepState::Failed;
                step.recovery_action = Some(recovery);
                return true;
            }
        }
        false
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
#[test]
fn deserialize_old_format_without_new_fields_uses_defaults() {
    let old_json = r#"{
            "planId": "old-plan",
            "taskId": "old-task",
            "status": "pending",
            "createdAtMs": 1,
            "steps": [
                {
                    "stepId": "step-0",
                    "index": 0,
                    "goal": "old goal",
                    "requiredContext": [],
                    "allowedTools": [],
                    "maxSideEffect": "read",
                    "successSignals": [],
                    "onFailure": "stop",
                    "status": "planned"
                }
            ]
        }"#;
    let decoded: ExecutionPlan = serde_json::from_str(old_json).unwrap();
    assert_eq!(decoded.steps[0].step_state, ExecutionStepState::Planned);
    assert!(decoded.steps[0].recovery_action.is_none());
}

#[test]
fn summary_counts_by_step_state_and_status() {
    let task = TaskPacket::new("t22", "summary", TaskScope::Chapter, 1);
    let mut plan = compile_plan(&task, "p22", 1);
    plan.steps[0].step_state = ExecutionStepState::Completed;
    plan.steps[0].status = StepStatus::Completed { evidence: vec![] };
    plan.steps[1].step_state = ExecutionStepState::Failed;
    plan.steps[1].status = StepStatus::Failed {
        reason: "e".into(),
        input_context_summary: vec![],
        recovery_suggestion: "none".into(),
    };
    plan.steps[2].step_state = ExecutionStepState::Skipped;
    plan.steps[2].status = StepStatus::Skipped {
        reason: "skipped".into(),
    };

    let (completed, failed, skipped) = plan.summary();
    assert_eq!(completed, 1);
    assert_eq!(failed, 1);
    assert_eq!(skipped, 1);
}

#[test]
fn is_terminal_checks_both_fields() {
    let mut step = ExecutionStep {
        step_state: ExecutionStepState::Completed,
        status: StepStatus::Running,
        ..Default::default()
    };
    assert!(step.is_terminal());

    step.step_state = ExecutionStepState::Running;
    step.status = StepStatus::Completed { evidence: vec![] };
    assert!(step.is_terminal());
}

#[test]
fn mark_ready_syncs_step_state() {
    let task = TaskPacket::new("t20", "mark ready sync", TaskScope::Chapter, 1);
    let mut plan = compile_plan(&task, "p20", 1);
    plan.mark_ready();

    for step in &plan.steps {
        assert_eq!(step.step_state, ExecutionStepState::Ready);
        assert_eq!(step.status, StepStatus::Ready);
    }
}

#[test]
fn recovery_action_serialization_roundtrip() {
    let actions = vec![
        StepRecoveryAction::Retry,
        StepRecoveryAction::Skip,
        StepRecoveryAction::RequestContextSupplement {
            sources: vec!["canon".to_string()],
        },
        StepRecoveryAction::PauseForApproval,
        StepRecoveryAction::Abort,
    ];
    for action in actions {
        let json = serde_json::to_string(&action).unwrap();
        let decoded: StepRecoveryAction = serde_json::from_str(&json).unwrap();
        assert_eq!(action, decoded);
    }
}

#[test]
fn serialization_roundtrip_with_new_fields() {
    let task = TaskPacket::new("t21", "serde new", TaskScope::Chapter, 1);
    let mut plan = compile_plan(&task, "p21", 1);
    plan.steps[1].step_state = ExecutionStepState::Running;
    plan.steps[1].status = StepStatus::Running;
    plan.steps[1].recovery_action = Some(StepRecoveryAction::Retry);

    let json = serde_json::to_string(&plan).unwrap();
    let decoded: ExecutionPlan = serde_json::from_str(&json).unwrap();

    assert_eq!(decoded.steps.len(), 4);
    assert_eq!(decoded.steps[1].step_state, ExecutionStepState::Running);
    assert_eq!(
        decoded.steps[1].recovery_action,
        Some(StepRecoveryAction::Retry)
    );
}

#[test]
fn fail_current_step_marks_running_as_failed_with_retry() {
    let task = TaskPacket::new("t13", "fail test", TaskScope::Chapter, 1);
    let mut plan = compile_plan(&task, "p13", 1);
    plan.steps[0].status = StepStatus::Running;
    plan.steps[0].step_state = ExecutionStepState::Running;

    let found = plan.fail_current_step(StepRecoveryAction::Retry);

    assert!(found);
    assert!(matches!(
        plan.steps[0].step_state,
        ExecutionStepState::Failed
    ));
    assert!(matches!(&plan.steps[0].status, StepStatus::Failed { .. }));
    assert_eq!(
        plan.steps[0].recovery_action,
        Some(StepRecoveryAction::Retry)
    );
}

#[test]
fn fail_current_step_with_skip_recovery() {
    let task = TaskPacket::new("t14", "fail skip", TaskScope::Chapter, 1);
    let mut plan = compile_plan(&task, "p14", 1);
    plan.steps[1].status = StepStatus::Running;
    plan.steps[1].step_state = ExecutionStepState::Running;

    let found = plan.fail_current_step(StepRecoveryAction::Skip);

    assert!(found);
    assert_eq!(plan.steps[1].step_state, ExecutionStepState::Failed);
    assert_eq!(
        plan.steps[1].recovery_action,
        Some(StepRecoveryAction::Skip)
    );
}

#[test]
fn fail_current_step_with_context_supplement() {
    let task = TaskPacket::new("t15", "fail supplement", TaskScope::Chapter, 1);
    let mut plan = compile_plan(&task, "p15", 1);
    plan.steps[0].status = StepStatus::Running;
    plan.steps[0].step_state = ExecutionStepState::Running;

    let found = plan.fail_current_step(StepRecoveryAction::RequestContextSupplement {
        sources: vec!["canon".to_string(), "promises".to_string()],
    });

    assert!(found);
    assert_eq!(
        plan.steps[0].recovery_action,
        Some(StepRecoveryAction::RequestContextSupplement {
            sources: vec!["canon".to_string(), "promises".to_string()],
        })
    );
}

#[test]
fn fail_current_step_with_pause_for_approval() {
    let task = TaskPacket::new("t16", "fail pause", TaskScope::Chapter, 1);
    let mut plan = compile_plan(&task, "p16", 1);
    plan.steps[0].status = StepStatus::Running;
    plan.steps[0].step_state = ExecutionStepState::Running;

    let found = plan.fail_current_step(StepRecoveryAction::PauseForApproval);

    assert!(found);
    assert_eq!(
        plan.steps[0].recovery_action,
        Some(StepRecoveryAction::PauseForApproval)
    );
}

#[test]
fn fail_current_step_with_abort() {
    let task = TaskPacket::new("t17", "fail abort", TaskScope::Chapter, 1);
    let mut plan = compile_plan(&task, "p17", 1);
    plan.steps[0].status = StepStatus::Running;
    plan.steps[0].step_state = ExecutionStepState::Running;

    let found = plan.fail_current_step(StepRecoveryAction::Abort);

    assert!(found);
    assert_eq!(
        plan.steps[0].recovery_action,
        Some(StepRecoveryAction::Abort)
    );
}

#[test]
fn fail_current_step_returns_false_when_no_running_step() {
    let task = TaskPacket::new("t18", "fail none", TaskScope::Chapter, 1);
    let mut plan = compile_plan(&task, "p18", 1);

    let found = plan.fail_current_step(StepRecoveryAction::Abort);

    assert!(!found);
}

#[test]
fn fail_current_step_finds_running_by_status_when_step_state_stale() {
    let task = TaskPacket::new("t19", "stale state", TaskScope::Chapter, 1);
    let mut plan = compile_plan(&task, "p19", 1);
    plan.steps[2].status = StepStatus::Running;
    plan.steps[2].step_state = ExecutionStepState::Planned;

    let found = plan.fail_current_step(StepRecoveryAction::Skip);

    assert!(found);
    assert_eq!(plan.steps[2].step_state, ExecutionStepState::Failed);
}

#[test]
fn resume_from_step_skips_earlier_and_readies_target() {
    let task = TaskPacket::new("t10", "resume test", TaskScope::Chapter, 1);
    let mut plan = compile_plan(&task, "p10", 1);
    plan.steps[0].status = StepStatus::Running;
    plan.steps[0].step_state = ExecutionStepState::Running;

    plan.resume_from_step(2);

    assert!(matches!(
        plan.steps[0].step_state,
        ExecutionStepState::Skipped
    ));
    assert!(matches!(&plan.steps[0].status, StepStatus::Skipped { .. }));
    assert!(matches!(
        plan.steps[1].step_state,
        ExecutionStepState::Skipped
    ));
    assert_eq!(plan.steps[2].step_state, ExecutionStepState::Ready);
    assert_eq!(plan.steps[2].status, StepStatus::Ready);
    assert_eq!(plan.steps[3].step_state, ExecutionStepState::Planned);
}

#[test]
fn resume_from_step_does_not_reopen_completed_steps() {
    let task = TaskPacket::new("t11", "resume completed", TaskScope::Chapter, 1);
    let mut plan = compile_plan(&task, "p11", 1);
    plan.steps[0].status = StepStatus::Completed { evidence: vec![] };
    plan.steps[0].step_state = ExecutionStepState::Completed;

    plan.resume_from_step(1);

    assert!(matches!(
        plan.steps[0].step_state,
        ExecutionStepState::Completed
    ));
    assert_eq!(plan.steps[1].step_state, ExecutionStepState::Ready);
}

#[test]
fn resume_from_step_zero_readies_first_step() {
    let task = TaskPacket::new("t12", "resume zero", TaskScope::Chapter, 1);
    let mut plan = compile_plan(&task, "p12", 1);

    plan.resume_from_step(0);

    assert_eq!(plan.steps[0].step_state, ExecutionStepState::Ready);
    assert_eq!(plan.steps[0].status, StepStatus::Ready);
    assert_eq!(plan.steps[1].step_state, ExecutionStepState::Planned);
}

// --- New tests for step state lifecycle and recovery ---

#[test]
fn step_state_defaults_to_planned() {
    let step = ExecutionStep::default();
    assert_eq!(step.step_state, ExecutionStepState::Planned);
    assert!(step.recovery_action.is_none());
}

#[test]
fn lifecycle_planned_to_ready_to_running_to_completed() {
    let mut step = ExecutionStep::default();
    assert_eq!(step.step_state, ExecutionStepState::Planned);

    step.step_state = ExecutionStepState::Ready;
    step.status = StepStatus::Ready;
    assert!(step.is_runnable());
    assert!(!step.is_terminal());
    assert!(!step.is_blocked());

    step.step_state = ExecutionStepState::Running;
    step.status = StepStatus::Running;
    assert!(!step.is_runnable());
    assert!(!step.is_terminal());

    step.step_state = ExecutionStepState::Completed;
    step.status = StepStatus::Completed {
        evidence: vec!["ok".to_string()],
    };
    assert!(step.is_terminal());
    assert!(!step.is_runnable());
}

#[test]
fn lifecycle_planned_to_ready_to_running_to_failed_with_retry() {
    let mut step = ExecutionStep {
        step_state: ExecutionStepState::Ready,
        status: StepStatus::Ready,
        ..Default::default()
    };
    step.step_state = ExecutionStepState::Running;
    step.status = StepStatus::Running;
    step.step_state = ExecutionStepState::Failed;
    step.status = StepStatus::Failed {
        reason: "timeout".to_string(),
        input_context_summary: vec![],
        recovery_suggestion: "retry".to_string(),
    };
    step.recovery_action = Some(StepRecoveryAction::Retry);
    assert!(step.is_terminal());
    assert!(!step.is_runnable());
    assert_eq!(step.recovery_action, Some(StepRecoveryAction::Retry));
}
