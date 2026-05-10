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
    /// ID of the most recently written checkpoint for this plan.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub last_checkpoint_id: Option<String>,
}

/// A structured success signal that can be evaluated against step evidence.
/// Replaces free-form string signals with rule-based checks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StepSignal {
    /// Check that the step produced an artifact matching the given name/pattern.
    ArtifactProduced { artifact_name: String },
    /// Check that the step called a specific read-only validation tool.
    ReadOnlyCheckCalled { tool_name: String },
    /// Check that the step received explicit author approval.
    AuthorApproval { approval_ref: String },
    /// Custom signal checked by name match against evidence.
    Custom { name: String },
}

impl StepSignal {
    /// Evaluate this signal against the provided step evidence.
    /// Returns `true` if the signal is satisfied by the evidence.
    pub fn evaluate(&self, evidence: &StepEvidence) -> bool {
        match self {
            StepSignal::ArtifactProduced { artifact_name } => evidence
                .artifact_refs
                .iter()
                .any(|a| a.contains(artifact_name)),
            StepSignal::ReadOnlyCheckCalled { tool_name } => evidence
                .tool_executions
                .iter()
                .any(|t| t.contains(tool_name)),
            StepSignal::AuthorApproval { approval_ref } => evidence
                .artifact_refs
                .iter()
                .any(|a| a.contains(approval_ref)),
            StepSignal::Custom { name } => {
                evidence.artifact_refs.iter().any(|a| a.contains(name))
                    || evidence.tool_executions.iter().any(|t| t.contains(name))
                    || evidence.context_refs.iter().any(|c| c.contains(name))
            }
        }
    }

    /// Human-readable description of what this signal checks.
    pub fn description(&self) -> String {
        match self {
            StepSignal::ArtifactProduced { artifact_name } => {
                format!("artifact '{}' produced", artifact_name)
            }
            StepSignal::ReadOnlyCheckCalled { tool_name } => {
                format!("read-only check '{}' called", tool_name)
            }
            StepSignal::AuthorApproval { approval_ref } => {
                format!("author approval '{}' received", approval_ref)
            }
            StepSignal::Custom { name } => format!("custom signal '{}' satisfied", name),
        }
    }
}

/// Evaluate a list of success signals against step evidence.
/// Returns `Ok(())` if all signals are satisfied, or `Err` with missing signal descriptions.
pub fn evaluate_success_signals(
    signals: &[StepSignal],
    evidence: &StepEvidence,
) -> Result<(), Vec<String>> {
    let missing: Vec<String> = signals
        .iter()
        .filter(|s| !s.evaluate(evidence))
        .map(|s| s.description())
        .collect();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(missing)
    }
}

/// Contract that defines the expected inputs, allowed tools, and success criteria
/// for a single execution step. Enforced at runtime by the agent loop.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StepContract {
    pub step_id: String,
    pub input_summary: String,
    pub required_context: Vec<String>,
    pub allowed_tools: Vec<String>,
    pub max_side_effect: ToolSideEffectLevel,
    pub provider_allowed: bool,
    pub success_evidence_required: Vec<String>,
    /// Structured success signals evaluated against step evidence.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub success_signals: Vec<StepSignal>,
    pub failure_policy: StepFailureAction,
}

/// Evidence collected when a step completes, proving the step fulfilled its contract.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StepEvidence {
    pub step_id: String,
    pub artifact_refs: Vec<String>,
    pub tool_executions: Vec<String>,
    pub provider_usage: Option<ProviderUsageSummary>,
    pub context_refs: Vec<String>,
    pub completion_time_ms: u64,
    /// Deterministic hash of the context pack used for this step.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub context_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderUsageSummary {
    pub model: String,
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
    pub duration_ms: u64,
}

/// Unified durable checkpoint for agent execution.
/// Written at step boundaries and key operation points to enable
/// resume after interruption.
///
/// Supersedes `LongTaskCheckpoint` from `agent-writer-backend`. Backward
/// compatibility is maintained via `From`/`Into` conversions.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCheckpoint {
    pub checkpoint_id: String,
    pub task_id: String,
    pub plan_id: String,
    pub step_id: String,
    pub phase: CheckpointPhase,
    pub input_hash: String,
    pub context_hash: String,
    pub artifact_refs: Vec<String>,
    pub tool_effects: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub provider_usage: Option<ProviderUsageSummary>,
    pub budget_spent: u64,
    pub approval_refs: Vec<String>,
    pub resume_policy: ResumePolicy,
    /// Backward-compat: task kind for legacy `LongTaskCheckpoint` mapping.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub task_kind: Option<String>,
    /// Backward-compat: safe-resume payload for legacy `LongTaskCheckpoint` mapping.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub safe_resume_payload: Option<serde_json::Value>,
    /// Source of the checkpoint (e.g. "agent_loop", "pipeline").
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub source: Option<String>,
    /// Timestamp when the checkpoint was created (ms since epoch).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub created_at_ms: Option<u64>,
}

/// Phase of execution at which a checkpoint was captured.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckpointPhase {
    StepStarted,
    StepCompleted,
    ProviderCallBefore,
    ProviderCallAfter,
    SavePrepared,
    WriteBefore,
    WriteAfter,
}

/// Policy dictating how to resume from a checkpoint.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResumePolicy {
    /// Step was completed; resume should skip it.
    Skip,
    /// Step needs to be re-executed.
    Rerun,
    /// Resume requires explicit approval before proceeding.
    RequireApproval,
    /// Checkpoint is not recoverable.
    Abort,
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
    /// Structured success signals for rule-based step completion checks.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub success_signals: Vec<StepSignal>,
    pub on_failure: StepFailureAction,
    pub status: StepStatus,
    #[serde(default)]
    pub step_state: ExecutionStepState,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub recovery_action: Option<StepRecoveryAction>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub contract: Option<StepContract>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub evidence: Option<StepEvidence>,
    /// Whether a checkpoint was written for this step during execution.
    #[serde(default)]
    pub checkpoint_written: bool,
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
            contract: None,
            evidence: None,
            checkpoint_written: false,
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

    /// Resume an incomplete plan, recovering running/blocked steps according to their contract.
    ///
    /// - Terminal steps (Completed, Failed, Skipped) are left as-is.
    /// - Running steps are reset to Ready (will be re-executed).
    /// - Blocked steps are reset to Ready if their contract allows recovery,
    ///   otherwise they remain Blocked.
    /// - Planned steps are left as Planned.
    pub fn resume(&mut self) {
        for step in &mut self.steps {
            if step.is_terminal() {
                continue;
            }
            match step.step_state {
                ExecutionStepState::Running => {
                    step.status = StepStatus::Ready;
                    step.step_state = ExecutionStepState::Ready;
                }
                ExecutionStepState::Blocked => {
                    // Determine recovery policy from contract or step-level failure action
                    let can_recover = step
                        .contract
                        .as_ref()
                        .map(|c| {
                            !matches!(
                                c.failure_policy,
                                StepFailureAction::Abort | StepFailureAction::Stop
                            )
                        })
                        .unwrap_or_else(|| {
                            !matches!(
                                step.on_failure,
                                StepFailureAction::Abort | StepFailureAction::Stop
                            )
                        });
                    if can_recover {
                        step.status = StepStatus::Ready;
                        step.step_state = ExecutionStepState::Ready;
                    }
                    // else: remains blocked
                }
                ExecutionStepState::Ready | ExecutionStepState::Planned => {
                    // Already in a resumable state
                }
                _ => {}
            }
        }
        // Reset plan status if it was failed
        if matches!(self.status, PlanStatus::Failed { .. }) {
            self.status = PlanStatus::Running;
        }
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
        last_checkpoint_id: None,
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
fn agent_checkpoint_serialization_roundtrip() {
    let cp = AgentCheckpoint {
        checkpoint_id: "cp-1".to_string(),
        task_id: "task-1".to_string(),
        plan_id: "plan-1".to_string(),
        step_id: "step-0".to_string(),
        phase: CheckpointPhase::StepCompleted,
        input_hash: "hash-a".to_string(),
        context_hash: "hash-b".to_string(),
        artifact_refs: vec!["draft.txt".to_string()],
        tool_effects: vec!["tool:read".to_string()],
        provider_usage: Some(ProviderUsageSummary {
            model: "gpt-4".to_string(),
            prompt_tokens: 100,
            completion_tokens: 50,
            total_tokens: 150,
            duration_ms: 1200,
        }),
        budget_spent: 2500,
        approval_refs: vec!["approval-1".to_string()],
        resume_policy: ResumePolicy::Skip,
        task_kind: Some("chapter_generation".to_string()),
        safe_resume_payload: Some(serde_json::json!({"step": "draft"})),
        source: Some("pipeline".to_string()),
        created_at_ms: Some(1_700_000_000_000),
    };
    let json = serde_json::to_string(&cp).unwrap();
    let decoded: AgentCheckpoint = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded.checkpoint_id, cp.checkpoint_id);
    assert_eq!(decoded.phase, CheckpointPhase::StepCompleted);
    assert_eq!(decoded.resume_policy, ResumePolicy::Skip);
    assert_eq!(decoded.budget_spent, 2500);
    assert_eq!(decoded.artifact_refs, vec!["draft.txt"]);
    assert_eq!(decoded.approval_refs, vec!["approval-1"]);
    assert_eq!(decoded.task_kind, Some("chapter_generation".to_string()));
    assert_eq!(decoded.source, Some("pipeline".to_string()));
    assert_eq!(decoded.created_at_ms, Some(1_700_000_000_000));
}

#[test]
fn checkpoint_phase_serialization_roundtrip() {
    let phases = vec![
        CheckpointPhase::StepStarted,
        CheckpointPhase::StepCompleted,
        CheckpointPhase::ProviderCallBefore,
        CheckpointPhase::ProviderCallAfter,
        CheckpointPhase::SavePrepared,
        CheckpointPhase::WriteBefore,
        CheckpointPhase::WriteAfter,
    ];
    for phase in phases {
        let json = serde_json::to_string(&phase).unwrap();
        let decoded: CheckpointPhase = serde_json::from_str(&json).unwrap();
        assert_eq!(phase, decoded);
    }
}

#[test]
fn resume_policy_serialization_roundtrip() {
    let policies = vec![
        ResumePolicy::Skip,
        ResumePolicy::Rerun,
        ResumePolicy::RequireApproval,
        ResumePolicy::Abort,
    ];
    for policy in policies {
        let json = serde_json::to_string(&policy).unwrap();
        let decoded: ResumePolicy = serde_json::from_str(&json).unwrap();
        assert_eq!(policy, decoded);
    }
}

#[test]
fn agent_checkpoint_deserialize_old_format_uses_defaults() {
    let old_json = r#"{
        "checkpointId": "cp-old",
        "taskId": "task-old",
        "planId": "plan-old",
        "stepId": "step-0",
        "phase": "step_started",
        "inputHash": "",
        "contextHash": "",
        "artifactRefs": [],
        "toolEffects": [],
        "budgetSpent": 0,
        "approvalRefs": [],
        "resumePolicy": "rerun"
    }"#;
    let decoded: AgentCheckpoint = serde_json::from_str(old_json).unwrap();
    assert_eq!(decoded.checkpoint_id, "cp-old");
    assert_eq!(decoded.phase, CheckpointPhase::StepStarted);
    assert_eq!(decoded.resume_policy, ResumePolicy::Rerun);
    assert!(decoded.provider_usage.is_none());
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
    assert!(!decoded.steps[0].checkpoint_written);
    assert!(decoded.last_checkpoint_id.is_none());
}

#[test]
fn execution_step_checkpoint_written_field_defaults_false() {
    let step = ExecutionStep::default();
    assert!(!step.checkpoint_written);
}

#[test]
fn execution_plan_last_checkpoint_id_roundtrip() {
    let task = TaskPacket::new("t-cp", "test", TaskScope::Chapter, 1);
    let mut plan = compile_plan(&task, "p-cp", 1);
    plan.last_checkpoint_id = Some("cp-123".to_string());
    let json = serde_json::to_string(&plan).unwrap();
    let decoded: ExecutionPlan = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded.last_checkpoint_id, Some("cp-123".to_string()));
}

#[test]
fn execution_step_checkpoint_written_roundtrip() {
    let step = ExecutionStep {
        checkpoint_written: true,
        ..Default::default()
    };
    let json = serde_json::to_string(&step).unwrap();
    let decoded: ExecutionStep = serde_json::from_str(&json).unwrap();
    assert!(decoded.checkpoint_written);
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

// --- Tests for StepContract and StepEvidence ---

#[test]
fn step_contract_defaults_to_none() {
    let step = ExecutionStep::default();
    assert!(step.contract.is_none());
    assert!(step.evidence.is_none());
}

#[test]
fn step_contract_serialization_roundtrip() {
    let contract = StepContract {
        step_id: "step-1".to_string(),
        input_summary: "load context".to_string(),
        required_context: vec!["outline".to_string()],
        allowed_tools: vec![
            "load_current_chapter".to_string(),
            "search_lorebook".to_string(),
        ],
        max_side_effect: ToolSideEffectLevel::Read,
        provider_allowed: false,
        success_evidence_required: vec!["chapter_text".to_string()],
        success_signals: vec![],
        failure_policy: StepFailureAction::Stop,
    };
    let json = serde_json::to_string(&contract).unwrap();
    let decoded: StepContract = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded.step_id, "step-1");
    assert_eq!(
        decoded.allowed_tools,
        vec!["load_current_chapter", "search_lorebook"]
    );
    assert_eq!(decoded.success_evidence_required, vec!["chapter_text"]);
}

#[test]
fn step_evidence_serialization_roundtrip() {
    let evidence = StepEvidence {
        step_id: "step-1".to_string(),
        artifact_refs: vec!["draft.txt".to_string()],
        tool_executions: vec!["load_current_chapter".to_string()],
        provider_usage: Some(ProviderUsageSummary {
            model: "gpt-4".to_string(),
            prompt_tokens: 100,
            completion_tokens: 50,
            total_tokens: 150,
            duration_ms: 1200,
        }),
        context_refs: vec!["outline".to_string()],
        completion_time_ms: 1200,
        context_hash: String::new(),
    };
    let json = serde_json::to_string(&evidence).unwrap();
    let decoded: StepEvidence = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded.artifact_refs, vec!["draft.txt"]);
    assert_eq!(decoded.provider_usage.as_ref().unwrap().model, "gpt-4");
    assert_eq!(decoded.provider_usage.as_ref().unwrap().total_tokens, 150);
}

#[test]
fn execution_step_with_contract_and_evidence() {
    let mut step = ExecutionStep {
        step_id: "step-0".to_string(),
        index: 0,
        goal: "test".to_string(),
        contract: Some(StepContract {
            step_id: "step-0".to_string(),
            input_summary: "context".to_string(),
            required_context: vec![],
            allowed_tools: vec!["read_tool".to_string()],
            max_side_effect: ToolSideEffectLevel::Read,
            provider_allowed: false,
            success_evidence_required: vec!["output".to_string()],
            success_signals: vec![],
            failure_policy: StepFailureAction::Stop,
        }),
        ..Default::default()
    };
    assert!(step.contract.is_some());
    assert_eq!(
        step.contract.as_ref().unwrap().allowed_tools,
        vec!["read_tool"]
    );

    step.evidence = Some(StepEvidence {
        step_id: "step-0".to_string(),
        artifact_refs: vec!["result".to_string()],
        tool_executions: vec![],
        provider_usage: None,
        context_refs: vec![],
        completion_time_ms: 100,
        context_hash: String::new(),
    });
    assert!(step.evidence.is_some());
}

#[test]
fn plan_resume_skips_terminal_steps_with_contract() {
    let task = TaskPacket::new("t-resume-contract", "test", TaskScope::Chapter, 1);
    let mut plan = compile_plan(&task, "p-resume-contract", 1);

    // Step 0: completed with contract
    plan.steps[0].status = StepStatus::Completed {
        evidence: vec!["ok".to_string()],
    };
    plan.steps[0].step_state = ExecutionStepState::Completed;
    plan.steps[0].contract = Some(StepContract {
        step_id: "step-0".to_string(),
        input_summary: "preflight".to_string(),
        required_context: vec![],
        allowed_tools: vec!["read_tool".to_string()],
        max_side_effect: ToolSideEffectLevel::Read,
        provider_allowed: false,
        success_evidence_required: vec![],
        success_signals: vec![],
        failure_policy: StepFailureAction::Stop,
    });

    // Step 1: blocked with contract
    plan.steps[1].status = StepStatus::Blocked {
        reason: "waiting".to_string(),
        context_summary: vec![],
        recovery_suggestion: "wait".to_string(),
    };
    plan.steps[1].step_state = ExecutionStepState::Blocked;
    plan.steps[1].contract = Some(StepContract {
        step_id: "step-1".to_string(),
        input_summary: "draft".to_string(),
        required_context: vec![],
        allowed_tools: vec!["generate_bounded_continuation".to_string()],
        max_side_effect: ToolSideEffectLevel::ProviderCall,
        provider_allowed: true,
        success_evidence_required: vec!["draft".to_string()],
        success_signals: vec![],
        failure_policy: StepFailureAction::Retry { max_retries: 1 },
    });

    // Resume from step 1
    plan.resume_from_step(1);

    // Step 0 stays completed (terminal, not reopened)
    assert_eq!(plan.steps[0].step_state, ExecutionStepState::Completed);
    // Step 1 is readied (was blocked, now resumed)
    assert_eq!(plan.steps[1].step_state, ExecutionStepState::Ready);
    assert_eq!(plan.steps[1].status, StepStatus::Ready);
    // Contract is preserved
    assert!(plan.steps[1].contract.is_some());
    assert_eq!(
        plan.steps[1].contract.as_ref().unwrap().allowed_tools,
        vec!["generate_bounded_continuation"]
    );
}

#[test]
fn deserialize_old_format_without_contract_and_evidence() {
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
    assert!(decoded.steps[0].contract.is_none());
    assert!(decoded.steps[0].evidence.is_none());
}

// --- A1: StepSignal and evaluate_success_signals tests ---

#[test]
fn step_signal_artifact_produced_matches() {
    let evidence = StepEvidence {
        step_id: "s1".to_string(),
        artifact_refs: vec!["draft.txt".to_string(), "outline.json".to_string()],
        tool_executions: vec![],
        provider_usage: None,
        context_refs: vec![],
        completion_time_ms: 100,
        context_hash: String::new(),
    };
    let signal = StepSignal::ArtifactProduced {
        artifact_name: "draft".to_string(),
    };
    assert!(signal.evaluate(&evidence));
}

#[test]
fn step_signal_artifact_produced_no_match() {
    let evidence = StepEvidence {
        step_id: "s1".to_string(),
        artifact_refs: vec!["outline.json".to_string()],
        tool_executions: vec![],
        provider_usage: None,
        context_refs: vec![],
        completion_time_ms: 100,
        context_hash: String::new(),
    };
    let signal = StepSignal::ArtifactProduced {
        artifact_name: "draft".to_string(),
    };
    assert!(!signal.evaluate(&evidence));
}

#[test]
fn step_signal_read_only_check_called_matches() {
    let evidence = StepEvidence {
        step_id: "s1".to_string(),
        artifact_refs: vec![],
        tool_executions: vec!["run_quality_diagnostics".to_string()],
        provider_usage: None,
        context_refs: vec![],
        completion_time_ms: 100,
        context_hash: String::new(),
    };
    let signal = StepSignal::ReadOnlyCheckCalled {
        tool_name: "quality".to_string(),
    };
    assert!(signal.evaluate(&evidence));
}

#[test]
fn step_signal_author_approval_matches() {
    let evidence = StepEvidence {
        step_id: "s1".to_string(),
        artifact_refs: vec!["approval_chapter_3".to_string()],
        tool_executions: vec![],
        provider_usage: None,
        context_refs: vec![],
        completion_time_ms: 100,
        context_hash: String::new(),
    };
    let signal = StepSignal::AuthorApproval {
        approval_ref: "chapter_3".to_string(),
    };
    assert!(signal.evaluate(&evidence));
}

#[test]
fn step_signal_custom_matches_any_field() {
    let evidence = StepEvidence {
        step_id: "s1".to_string(),
        artifact_refs: vec![],
        tool_executions: vec![],
        provider_usage: None,
        context_refs: vec!["canon_ref".to_string()],
        completion_time_ms: 100,
        context_hash: String::new(),
    };
    let signal = StepSignal::Custom {
        name: "canon".to_string(),
    };
    assert!(signal.evaluate(&evidence));
}

#[test]
fn evaluate_success_signals_all_pass() {
    let evidence = StepEvidence {
        step_id: "s1".to_string(),
        artifact_refs: vec!["draft.txt".to_string()],
        tool_executions: vec!["run_quality_diagnostics".to_string()],
        provider_usage: None,
        context_refs: vec![],
        completion_time_ms: 100,
        context_hash: String::new(),
    };
    let signals = vec![
        StepSignal::ArtifactProduced {
            artifact_name: "draft".to_string(),
        },
        StepSignal::ReadOnlyCheckCalled {
            tool_name: "quality".to_string(),
        },
    ];
    assert!(evaluate_success_signals(&signals, &evidence).is_ok());
}

#[test]
fn evaluate_success_signals_reports_missing() {
    let evidence = StepEvidence {
        step_id: "s1".to_string(),
        artifact_refs: vec!["draft.txt".to_string()],
        tool_executions: vec![],
        provider_usage: None,
        context_refs: vec![],
        completion_time_ms: 100,
        context_hash: String::new(),
    };
    let signals = vec![
        StepSignal::ArtifactProduced {
            artifact_name: "draft".to_string(),
        },
        StepSignal::ReadOnlyCheckCalled {
            tool_name: "quality".to_string(),
        },
    ];
    let result = evaluate_success_signals(&signals, &evidence);
    assert!(result.is_err());
    let missing = result.unwrap_err();
    assert_eq!(missing.len(), 1);
    assert!(missing[0].contains("quality"));
}

#[test]
fn step_signal_description_readable() {
    assert_eq!(
        StepSignal::ArtifactProduced {
            artifact_name: "draft".to_string(),
        }
        .description(),
        "artifact 'draft' produced"
    );
    assert_eq!(
        StepSignal::ReadOnlyCheckCalled {
            tool_name: "lint".to_string(),
        }
        .description(),
        "read-only check 'lint' called"
    );
    assert_eq!(
        StepSignal::AuthorApproval {
            approval_ref: "ch3".to_string(),
        }
        .description(),
        "author approval 'ch3' received"
    );
}

#[test]
fn step_signal_serialization_roundtrip() {
    let signals = vec![
        StepSignal::ArtifactProduced {
            artifact_name: "draft.txt".to_string(),
        },
        StepSignal::ReadOnlyCheckCalled {
            tool_name: "validate".to_string(),
        },
        StepSignal::AuthorApproval {
            approval_ref: "author-ok".to_string(),
        },
        StepSignal::Custom {
            name: "my_signal".to_string(),
        },
    ];
    for signal in &signals {
        let json = serde_json::to_string(signal).unwrap();
        let decoded: StepSignal = serde_json::from_str(&json).unwrap();
        assert_eq!(*signal, decoded);
    }
}

#[test]
fn plan_resume_resets_running_steps() {
    let task = TaskPacket::new("t-resume-running", "test", TaskScope::Chapter, 1);
    let mut plan = compile_plan(&task, "p-resume-running", 1);

    // Step 0: completed
    plan.steps[0].status = StepStatus::Completed { evidence: vec![] };
    plan.steps[0].step_state = ExecutionStepState::Completed;

    // Step 1: running (interrupted)
    plan.steps[1].status = StepStatus::Running;
    plan.steps[1].step_state = ExecutionStepState::Running;

    plan.status = PlanStatus::Failed {
        failed_step: "step-1".to_string(),
        reason: "interrupted".to_string(),
    };

    plan.resume();

    // Step 0 stays completed
    assert_eq!(plan.steps[0].step_state, ExecutionStepState::Completed);
    // Step 1 reset to Ready
    assert_eq!(plan.steps[1].step_state, ExecutionStepState::Ready);
    assert_eq!(plan.steps[1].status, StepStatus::Ready);
    // Plan status reset to Running
    assert_eq!(plan.status, PlanStatus::Running);
}

#[test]
fn plan_resume_recover_blocked_step_with_retry_policy() {
    let task = TaskPacket::new("t-resume-blocked", "test", TaskScope::Chapter, 1);
    let mut plan = compile_plan(&task, "p-resume-blocked", 1);

    // Step 0: completed
    plan.steps[0].status = StepStatus::Completed { evidence: vec![] };
    plan.steps[0].step_state = ExecutionStepState::Completed;

    // Step 1: blocked with Retry policy in contract
    plan.steps[1].status = StepStatus::Blocked {
        reason: "rate limited".to_string(),
        context_summary: vec![],
        recovery_suggestion: "retry".to_string(),
    };
    plan.steps[1].step_state = ExecutionStepState::Blocked;
    plan.steps[1].contract = Some(StepContract {
        step_id: "step-1".to_string(),
        input_summary: "draft".to_string(),
        required_context: vec![],
        allowed_tools: vec![],
        max_side_effect: ToolSideEffectLevel::ProviderCall,
        provider_allowed: true,
        success_evidence_required: vec![],
        success_signals: vec![],
        failure_policy: StepFailureAction::Retry { max_retries: 2 },
    });

    plan.status = PlanStatus::Failed {
        failed_step: "step-1".to_string(),
        reason: "rate limited".to_string(),
    };

    plan.resume();

    // Step 1 should be recovered to Ready (Retry policy allows recovery)
    assert_eq!(plan.steps[1].step_state, ExecutionStepState::Ready);
    assert_eq!(plan.steps[1].status, StepStatus::Ready);
}

#[test]
fn plan_resume_keeps_blocked_step_with_abort_policy() {
    let task = TaskPacket::new("t-resume-abort", "test", TaskScope::Chapter, 1);
    let mut plan = compile_plan(&task, "p-resume-abort", 1);

    // Step 0: completed
    plan.steps[0].status = StepStatus::Completed { evidence: vec![] };
    plan.steps[0].step_state = ExecutionStepState::Completed;

    // Step 1: blocked with Abort policy in contract
    plan.steps[1].status = StepStatus::Blocked {
        reason: "save conflict".to_string(),
        context_summary: vec![],
        recovery_suggestion: "manual resolve".to_string(),
    };
    plan.steps[1].step_state = ExecutionStepState::Blocked;
    plan.steps[1].contract = Some(StepContract {
        step_id: "step-1".to_string(),
        input_summary: "save".to_string(),
        required_context: vec![],
        allowed_tools: vec![],
        max_side_effect: ToolSideEffectLevel::Write,
        provider_allowed: false,
        success_evidence_required: vec![],
        success_signals: vec![],
        failure_policy: StepFailureAction::Abort,
    });

    plan.status = PlanStatus::Failed {
        failed_step: "step-1".to_string(),
        reason: "save conflict".to_string(),
    };

    plan.resume();

    // Step 1 should remain Blocked (Abort policy prevents recovery)
    assert_eq!(plan.steps[1].step_state, ExecutionStepState::Blocked);
    assert!(matches!(plan.steps[1].status, StepStatus::Blocked { .. }));
}

#[test]
fn plan_resume_uses_step_level_on_failure_when_no_contract() {
    let task = TaskPacket::new("t-resume-no-contract", "test", TaskScope::Chapter, 1);
    let mut plan = compile_plan(&task, "p-resume-no-contract", 1);

    // Step 0: completed
    plan.steps[0].status = StepStatus::Completed { evidence: vec![] };
    plan.steps[0].step_state = ExecutionStepState::Completed;

    // Step 1: blocked, no contract, but step-level on_failure is Skip (recoverable)
    plan.steps[1].status = StepStatus::Blocked {
        reason: "missing context".to_string(),
        context_summary: vec!["outline".to_string()],
        recovery_suggestion: "load outline".to_string(),
    };
    plan.steps[1].step_state = ExecutionStepState::Blocked;
    plan.steps[1].on_failure = StepFailureAction::Skip;
    // No contract

    plan.status = PlanStatus::Failed {
        failed_step: "step-1".to_string(),
        reason: "missing context".to_string(),
    };

    plan.resume();

    // Step 1 should be recovered because on_failure is Skip (not Abort/Stop)
    assert_eq!(plan.steps[1].step_state, ExecutionStepState::Ready);
    assert_eq!(plan.steps[1].status, StepStatus::Ready);
}

#[test]
fn step_contract_with_success_signals_serialization_roundtrip() {
    let contract = StepContract {
        step_id: "step-1".to_string(),
        input_summary: "draft chapter".to_string(),
        required_context: vec!["outline".to_string()],
        allowed_tools: vec!["generate_bounded_continuation".to_string()],
        max_side_effect: ToolSideEffectLevel::ProviderCall,
        provider_allowed: true,
        success_evidence_required: vec!["chapter_text".to_string()],
        success_signals: vec![
            StepSignal::ArtifactProduced {
                artifact_name: "chapter_draft".to_string(),
            },
            StepSignal::ReadOnlyCheckCalled {
                tool_name: "validate_quality".to_string(),
            },
        ],
        failure_policy: StepFailureAction::Retry { max_retries: 1 },
    };
    let json = serde_json::to_string(&contract).unwrap();
    let decoded: StepContract = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded.success_signals.len(), 2);
    assert_eq!(
        decoded.success_signals[0],
        StepSignal::ArtifactProduced {
            artifact_name: "chapter_draft".to_string(),
        }
    );
    assert_eq!(
        decoded.success_signals[1],
        StepSignal::ReadOnlyCheckCalled {
            tool_name: "validate_quality".to_string(),
        }
    );
}

#[test]
fn deserialize_old_format_with_string_success_signals_defaults_to_empty() {
    // Old format had success_signals as Vec<String>; new format is Vec<StepSignal>.
    // serde should handle the empty array case gracefully.
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
    assert!(decoded.steps[0].success_signals.is_empty());
}
