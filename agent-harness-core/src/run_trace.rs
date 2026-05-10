use serde::{Deserialize, Serialize};

use crate::execution_plan::{ExecutionPlan, PlanStatus, StepEvidence, StepStatus};
use crate::recovery::{RuntimeCallRecord, RuntimeCallStatus, RuntimeCallType};

// ── AgentRunReport: observability summary emitted at run end (A6) ──

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRunReport {
    pub run_id: String,
    pub plan_summary: PlanSummary,
    pub step_summaries: Vec<StepSummary>,
    pub provider_timeline: ProviderTimeline,
    pub context_quality: ContextQualitySummary,
    pub budget_summary: BudgetSummary,
    pub failure_recovery: Option<FailureRecoverySummary>,
    pub generated_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanSummary {
    pub plan_id: String,
    pub total_steps: usize,
    pub completed_steps: usize,
    pub failed_steps: usize,
    pub skipped_steps: usize,
    pub total_duration_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StepSummary {
    pub step_id: String,
    pub status: String,
    pub provider_calls: u32,
    pub tool_calls: u32,
    pub retry_count: u32,
    pub duration_ms: u64,
    pub allowed_tools: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderTimeline {
    pub total_calls: u32,
    pub total_duration_ms: u64,
    pub latency_p50_ms: u64,
    pub latency_p90_ms: u64,
    pub latency_p95_ms: u64,
    pub avg_ttft_ms: u64,
    pub avg_call_duration_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextQualitySummary {
    pub sources_count: u32,
    pub sources_missing: u32,
    pub sources_truncated: u32,
    pub retrieval_duration_ms: u64,
    pub quality_score: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BudgetSummary {
    pub total_prompt_tokens: u32,
    pub total_completion_tokens: u32,
    pub total_tokens: u32,
    pub estimated_cost_usd: f32,
    pub budget_limit_usd: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FailureRecoverySummary {
    pub failed_step: String,
    pub failure_kind: String,
    pub recovery_decision: String,
    pub user_choice_required: bool,
}

/// Build an `AgentRunReport` from an `ExecutionPlan`, runtime call records, and step evidence.
/// Works for both completed and failed runs (partial report).
pub fn build_agent_run_report(
    run_id: impl Into<String>,
    plan: &ExecutionPlan,
    runtime_calls: &[RuntimeCallRecord],
    step_evidences: &[StepEvidence],
    generated_at_ms: u64,
) -> AgentRunReport {
    let run_id = run_id.into();

    // ── Plan summary ──
    let (completed_steps, failed_steps, skipped_steps) = plan.summary();
    let total_duration_ms = step_evidences.iter().map(|e| e.completion_time_ms).sum();

    let plan_summary = PlanSummary {
        plan_id: plan.plan_id.clone(),
        total_steps: plan.steps.len(),
        completed_steps,
        failed_steps,
        skipped_steps,
        total_duration_ms,
    };

    // ── Step summaries ──
    let step_summaries: Vec<StepSummary> = plan
        .steps
        .iter()
        .map(|step| {
            let step_calls: Vec<&RuntimeCallRecord> = runtime_calls
                .iter()
                .filter(|rc| rc.step_id == step.step_id)
                .collect();
            let provider_calls = step_calls
                .iter()
                .filter(|rc| rc.call_type == RuntimeCallType::ProviderCall)
                .count() as u32;
            let tool_calls = step_calls
                .iter()
                .filter(|rc| rc.call_type == RuntimeCallType::ToolCall)
                .count() as u32;
            let retry_count = step_calls
                .iter()
                .filter(|rc| {
                    rc.call_type == RuntimeCallType::ProviderCall
                        && matches!(rc.status, RuntimeCallStatus::Failed { .. })
                })
                .count() as u32;
            let duration_ms = step_evidences
                .iter()
                .find(|e| e.step_id == step.step_id)
                .map(|e| e.completion_time_ms)
                .unwrap_or(0);
            let status = step_status_label(&step.status);
            let allowed_tools = step
                .contract
                .as_ref()
                .map(|c| c.allowed_tools.clone())
                .unwrap_or_else(|| step.allowed_tools.clone());

            StepSummary {
                step_id: step.step_id.clone(),
                status,
                provider_calls,
                tool_calls,
                retry_count,
                duration_ms,
                allowed_tools,
            }
        })
        .collect();

    // ── Provider timeline ──
    let provider_calls: Vec<&RuntimeCallRecord> = runtime_calls
        .iter()
        .filter(|rc| rc.call_type == RuntimeCallType::ProviderCall)
        .collect();

    let provider_timeline = if provider_calls.is_empty() {
        ProviderTimeline {
            total_calls: 0,
            total_duration_ms: 0,
            latency_p50_ms: 0,
            latency_p90_ms: 0,
            latency_p95_ms: 0,
            avg_ttft_ms: 0,
            avg_call_duration_ms: 0,
        }
    } else {
        let total_calls = provider_calls.len() as u32;
        let provider_durations: Vec<u64> = provider_calls.iter().map(|rc| rc.duration_ms).collect();
        let total_duration_ms: u64 = provider_durations.iter().sum();
        let mut sorted = provider_durations.clone();
        sorted.sort_unstable();
        let latency_p50_ms = percentile(&sorted, 0.50);
        let latency_p90_ms = percentile(&sorted, 0.90);
        let latency_p95_ms = percentile(&sorted, 0.95);
        let avg_call_duration_ms = total_duration_ms / total_calls as u64;

        // avg_ttft_ms: average of real TTFT values where available
        let ttft_values: Vec<u64> = provider_calls.iter().filter_map(|rc| rc.ttft_ms).collect();
        let avg_ttft_ms = if !ttft_values.is_empty() {
            ttft_values.iter().sum::<u64>() / ttft_values.len() as u64
        } else {
            avg_call_duration_ms
        };

        ProviderTimeline {
            total_calls,
            total_duration_ms,
            latency_p50_ms,
            latency_p90_ms,
            latency_p95_ms,
            avg_ttft_ms,
            avg_call_duration_ms,
        }
    };

    // ── Context quality ──
    let sources_count = step_evidences
        .iter()
        .map(|e| e.context_refs.len() as u32)
        .sum();
    let sources_missing = 0u32; // computed from context quality if available
    let sources_truncated = 0u32; // computed from context quality if available
    let retrieval_duration_ms = total_duration_ms;
    let quality_score = if sources_count > 0 { 1.0 } else { 0.0 };

    let context_quality = ContextQualitySummary {
        sources_count,
        sources_missing,
        sources_truncated,
        retrieval_duration_ms,
        quality_score,
    };

    // ── Budget summary ──
    let total_prompt_tokens: u32 = step_evidences
        .iter()
        .filter_map(|e| e.provider_usage.as_ref())
        .map(|u| u.prompt_tokens)
        .sum();
    let total_completion_tokens: u32 = step_evidences
        .iter()
        .filter_map(|e| e.provider_usage.as_ref())
        .map(|u| u.completion_tokens)
        .sum();
    let total_tokens = total_prompt_tokens + total_completion_tokens;
    // Rough cost estimate: $0.0015 per 1K prompt tokens, $0.006 per 1K completion tokens
    let estimated_cost_usd = (total_prompt_tokens as f32 * 0.0015 / 1000.0)
        + (total_completion_tokens as f32 * 0.006 / 1000.0);

    let budget_summary = BudgetSummary {
        total_prompt_tokens,
        total_completion_tokens,
        total_tokens,
        estimated_cost_usd,
        budget_limit_usd: None,
    };

    // ── Failure recovery ──
    let failure_recovery = match &plan.status {
        PlanStatus::Failed {
            failed_step,
            reason,
        } => {
            let failure_kind = crate::recovery::classify_failure_kind(reason, None);
            let recovery_decision = crate::recovery::map_failure_to_recovery(
                &failure_kind,
                &crate::recovery::RecoveryContext::default(),
            );
            Some(FailureRecoverySummary {
                failed_step: failed_step.clone(),
                failure_kind: format!("{:?}", failure_kind).to_snake_case(),
                recovery_decision: format!("{:?}", recovery_decision).to_snake_case(),
                user_choice_required: matches!(
                    recovery_decision,
                    crate::recovery::RecoveryDecision::SurfaceUserChoice
                        | crate::recovery::RecoveryDecision::RequestApproval
                ),
            })
        }
        _ => None,
    };

    AgentRunReport {
        run_id,
        plan_summary,
        step_summaries,
        provider_timeline,
        context_quality,
        budget_summary,
        failure_recovery,
        generated_at_ms,
    }
}

fn step_status_label(status: &StepStatus) -> String {
    match status {
        StepStatus::Planned => "planned".to_string(),
        StepStatus::Ready => "ready".to_string(),
        StepStatus::Running => "running".to_string(),
        StepStatus::Blocked { .. } => "blocked".to_string(),
        StepStatus::Completed { .. } => "completed".to_string(),
        StepStatus::Failed { .. } => "failed".to_string(),
        StepStatus::Skipped { .. } => "skipped".to_string(),
        StepStatus::Active => "active".to_string(),
    }
}

fn percentile(sorted: &[u64], p: f64) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let idx = ((sorted.len() as f64 - 1.0) * p).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

/// Snake-case helper for FailureKind / RecoveryDecision formatting.
trait ToSnakeCase {
    fn to_snake_case(&self) -> String;
}

impl ToSnakeCase for str {
    fn to_snake_case(&self) -> String {
        let mut result = String::new();
        for (i, ch) in self.chars().enumerate() {
            if ch.is_uppercase() {
                if i > 0 {
                    result.push('_');
                }
                result.push(ch.to_ascii_lowercase());
            } else {
                result.push(ch);
            }
        }
        result
    }
}

// ── Existing trace types ──

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentRunStatus {
    Running,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentRunEventKind {
    Started,
    Observation,
    ContextBuilt,
    ToolInventoryBuilt,
    ProviderGuardCheck,
    ContextWindowCheck,
    PlanStarted,
    StepStarted,
    StepCompleted,
    StepFailed,
    PlanCompleted,
    FailureBundle,
    ToolSelected,
    ToolFinished,
    LlmDelta,
    RetryAttempt,
    Compaction,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRunEvent {
    pub sequence: u32,
    pub elapsed_ms: u64,
    pub kind: AgentRunEventKind,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(default)]
    pub metadata: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRunTrace {
    pub run_id: String,
    pub goal: String,
    pub started_at_ms: u64,
    pub status: AgentRunStatus,
    pub events: Vec<AgentRunEvent>,
    pub tool_call_count: u32,
    pub context_chars: usize,
}

impl AgentRunTrace {
    pub fn new(run_id: impl Into<String>, goal: impl Into<String>, started_at_ms: u64) -> Self {
        let mut trace = Self {
            run_id: run_id.into(),
            goal: goal.into(),
            started_at_ms,
            status: AgentRunStatus::Running,
            events: Vec::new(),
            tool_call_count: 0,
            context_chars: 0,
        };
        trace.push(
            AgentRunEventKind::Started,
            "run_started",
            None,
            serde_json::Value::Null,
            started_at_ms,
        );
        trace
    }

    pub fn push(
        &mut self,
        kind: AgentRunEventKind,
        label: impl Into<String>,
        detail: Option<String>,
        metadata: serde_json::Value,
        now_ms: u64,
    ) {
        if matches!(kind, AgentRunEventKind::ToolSelected) {
            self.tool_call_count += 1;
        }

        self.events.push(AgentRunEvent {
            sequence: self.events.len() as u32 + 1,
            elapsed_ms: now_ms.saturating_sub(self.started_at_ms),
            kind,
            label: label.into(),
            detail,
            metadata,
        });
    }

    pub fn record_context_built(&mut self, context_chars: usize, source_count: usize, now_ms: u64) {
        self.context_chars = context_chars;
        self.push(
            AgentRunEventKind::ContextBuilt,
            "context_built",
            Some(format!(
                "{} chars from {} sources",
                context_chars, source_count
            )),
            serde_json::json!({
                "contextChars": context_chars,
                "sourceCount": source_count
            }),
            now_ms,
        );
    }

    pub fn complete(&mut self, now_ms: u64) {
        self.status = AgentRunStatus::Completed;
        self.push(
            AgentRunEventKind::Completed,
            "run_completed",
            None,
            serde_json::Value::Null,
            now_ms,
        );
    }

    pub fn fail(&mut self, error: impl Into<String>, now_ms: u64) {
        let error = error.into();
        self.status = AgentRunStatus::Failed;
        self.push(
            AgentRunEventKind::Failed,
            "run_failed",
            Some(error.clone()),
            serde_json::json!({ "error": error }),
            now_ms,
        );
    }

    pub fn cancel(&mut self, reason: impl Into<String>, now_ms: u64) {
        let reason = reason.into();
        self.status = AgentRunStatus::Cancelled;
        self.push(
            AgentRunEventKind::Cancelled,
            "run_cancelled",
            Some(reason.clone()),
            serde_json::json!({ "reason": reason }),
            now_ms,
        );
    }

    pub fn record_retry_attempt(
        &mut self,
        step_id: impl Into<String>,
        attempt: u32,
        reason: impl Into<String>,
        now_ms: u64,
    ) {
        let step_id = step_id.into();
        let reason = reason.into();
        self.push(
            AgentRunEventKind::RetryAttempt,
            "retry_attempt",
            Some(format!(
                "step={} attempt={} reason={}",
                step_id, attempt, reason
            )),
            serde_json::json!({
                "stepId": step_id,
                "attempt": attempt,
                "reason": reason
            }),
            now_ms,
        );
    }

    pub fn record_compaction(
        &mut self,
        before_tokens: u64,
        after_tokens: u64,
        tokens_saved: u64,
        now_ms: u64,
    ) {
        self.push(
            AgentRunEventKind::Compaction,
            "compaction",
            Some(format!(
                "{} -> {} tokens (saved {})",
                before_tokens, after_tokens, tokens_saved
            )),
            serde_json::json!({
                "beforeTokens": before_tokens,
                "afterTokens": after_tokens,
                "tokensSaved": tokens_saved
            }),
            now_ms,
        );
    }

    pub fn record_provider_guard(
        &mut self,
        allowed: bool,
        model: impl Into<String>,
        estimated_input_tokens: u64,
        requested_output_tokens: u64,
        now_ms: u64,
    ) {
        let model = model.into();
        self.push(
            AgentRunEventKind::ProviderGuardCheck,
            if allowed {
                "provider_guard_allowed"
            } else {
                "provider_guard_blocked"
            },
            Some(format!(
                "model={} estimated_input={} requested_output={}",
                model, estimated_input_tokens, requested_output_tokens
            )),
            serde_json::json!({
                "allowed": allowed,
                "model": model,
                "estimatedInputTokens": estimated_input_tokens,
                "requestedOutputTokens": requested_output_tokens
            }),
            now_ms,
        );
    }

    pub fn record_context_window(
        &mut self,
        estimated_input: u64,
        requested_output: u64,
        should_warn: bool,
        should_block: bool,
        now_ms: u64,
    ) {
        self.push(
            AgentRunEventKind::ContextWindowCheck,
            "context_window_check",
            Some(format!(
                "estimated_input={} requested_output={} warn={} block={}",
                estimated_input, requested_output, should_warn, should_block
            )),
            serde_json::json!({
                "estimatedInput": estimated_input,
                "requestedOutput": requested_output,
                "shouldWarn": should_warn,
                "shouldBlock": should_block
            }),
            now_ms,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution_plan::{ExecutionStep, ProviderUsageSummary};
    use crate::tool_registry::ToolSideEffectLevel;

    #[test]
    fn trace_records_context_and_completion() {
        let mut trace = AgentRunTrace::new("run-1", "draft a scene", 1_000);
        trace.record_context_built(1200, 3, 1_050);
        trace.complete(1_100);

        assert_eq!(trace.status, AgentRunStatus::Completed);
        assert_eq!(trace.context_chars, 1200);
        assert_eq!(trace.events.len(), 3);
        assert_eq!(trace.events[1].elapsed_ms, 50);
    }

    #[test]
    fn trace_records_retry_and_compaction() {
        let mut trace = AgentRunTrace::new("run-1", "draft a scene", 1_000);
        trace.record_retry_attempt("step-1", 1, "rate limit", 1_200);
        trace.record_compaction(8000, 4000, 4000, 1_500);
        trace.fail("max retries exceeded", 2_000);

        assert_eq!(trace.status, AgentRunStatus::Failed);
        assert_eq!(trace.events.len(), 4);
        assert_eq!(trace.events[1].kind, AgentRunEventKind::RetryAttempt);
        assert_eq!(trace.events[2].kind, AgentRunEventKind::Compaction);
        assert_eq!(trace.events[1].metadata["reason"], "rate limit");
    }

    #[test]
    fn trace_records_provider_guard_and_context_window() {
        let mut trace = AgentRunTrace::new("run-1", "draft a scene", 1_000);
        trace.record_provider_guard(true, "gpt-4", 1000, 500, 1_100);
        trace.record_context_window(1000, 500, false, false, 1_200);
        trace.complete(1_500);

        assert_eq!(trace.events.len(), 4);
        assert_eq!(trace.events[1].kind, AgentRunEventKind::ProviderGuardCheck);
        assert_eq!(trace.events[2].kind, AgentRunEventKind::ContextWindowCheck);
        assert_eq!(trace.events[1].metadata["allowed"], true);
        assert_eq!(trace.events[2].metadata["shouldBlock"], false);
    }

    // ── A6: AgentRunReport tests ──

    fn make_test_plan() -> ExecutionPlan {
        ExecutionPlan {
            plan_id: "plan-1".to_string(),
            task_id: "task-1".to_string(),
            steps: vec![
                ExecutionStep {
                    step_id: "step-0".to_string(),
                    index: 0,
                    goal: "preflight".to_string(),
                    allowed_tools: vec!["load_current_chapter".to_string()],
                    max_side_effect: ToolSideEffectLevel::Read,
                    status: StepStatus::Completed {
                        evidence: vec!["ok".to_string()],
                    },
                    evidence: Some(StepEvidence {
                        step_id: "step-0".to_string(),
                        artifact_refs: vec!["draft.txt".to_string()],
                        tool_executions: vec![],
                        provider_usage: Some(ProviderUsageSummary {
                            model: "gpt-4".to_string(),
                            prompt_tokens: 1000,
                            completion_tokens: 500,
                            total_tokens: 1500,
                            duration_ms: 1200,
                        }),
                        context_refs: vec!["outline".to_string(), "lorebook".to_string()],
                        completion_time_ms: 1200,
                        context_hash: String::new(),
                    }),
                    ..Default::default()
                },
                ExecutionStep {
                    step_id: "step-1".to_string(),
                    index: 1,
                    goal: "draft".to_string(),
                    allowed_tools: vec!["generate_bounded_continuation".to_string()],
                    max_side_effect: ToolSideEffectLevel::ProviderCall,
                    status: StepStatus::Completed {
                        evidence: vec!["ok".to_string()],
                    },
                    evidence: Some(StepEvidence {
                        step_id: "step-1".to_string(),
                        artifact_refs: vec!["chapter.txt".to_string()],
                        tool_executions: vec![],
                        provider_usage: Some(ProviderUsageSummary {
                            model: "gpt-4".to_string(),
                            prompt_tokens: 2000,
                            completion_tokens: 1000,
                            total_tokens: 3000,
                            duration_ms: 3500,
                        }),
                        context_refs: vec!["outline".to_string()],
                        completion_time_ms: 3500,
                        context_hash: String::new(),
                    }),
                    ..Default::default()
                },
                ExecutionStep {
                    step_id: "step-2".to_string(),
                    index: 2,
                    goal: "validate".to_string(),
                    allowed_tools: vec!["run_quality_diagnostics".to_string()],
                    max_side_effect: ToolSideEffectLevel::Read,
                    status: StepStatus::Skipped {
                        reason: "skipped".to_string(),
                    },
                    evidence: None,
                    ..Default::default()
                },
            ],
            status: PlanStatus::Completed,
            created_at_ms: 0,
        }
    }

    fn make_test_runtime_calls() -> Vec<RuntimeCallRecord> {
        vec![
            RuntimeCallRecord {
                call_id: "c1".to_string(),
                call_type: RuntimeCallType::ProviderCall,
                step_id: "step-0".to_string(),
                timestamp_ms: 1000,
                input_redacted_summary: "{\"model\":\"gpt-4\"}".to_string(),
                output_summary: "ok".to_string(),
                duration_ms: 1200,
                ttft_ms: Some(300),
                status: RuntimeCallStatus::Success,
                remediation_code: None,
            },
            RuntimeCallRecord {
                call_id: "c2".to_string(),
                call_type: RuntimeCallType::ProviderCall,
                step_id: "step-1".to_string(),
                timestamp_ms: 2500,
                input_redacted_summary: "{\"model\":\"gpt-4\"}".to_string(),
                output_summary: "ok".to_string(),
                duration_ms: 3500,
                ttft_ms: Some(800),
                status: RuntimeCallStatus::Success,
                remediation_code: None,
            },
            RuntimeCallRecord {
                call_id: "c3".to_string(),
                call_type: RuntimeCallType::ToolCall,
                step_id: "step-1".to_string(),
                timestamp_ms: 2600,
                input_redacted_summary: "{\"query\":\"test\"}".to_string(),
                output_summary: "result".to_string(),
                duration_ms: 100,
                ttft_ms: None,
                status: RuntimeCallStatus::Success,
                remediation_code: None,
            },
        ]
    }

    #[test]
    fn build_report_computes_plan_summary() {
        let plan = make_test_plan();
        let calls = make_test_runtime_calls();
        let evidences: Vec<StepEvidence> = plan
            .steps
            .iter()
            .filter_map(|s| s.evidence.clone())
            .collect();
        let report = build_agent_run_report("run-1", &plan, &calls, &evidences, 10_000);

        assert_eq!(report.plan_summary.plan_id, "plan-1");
        assert_eq!(report.plan_summary.total_steps, 3);
        assert_eq!(report.plan_summary.completed_steps, 2);
        assert_eq!(report.plan_summary.failed_steps, 0);
        assert_eq!(report.plan_summary.skipped_steps, 1);
        assert_eq!(report.plan_summary.total_duration_ms, 4700);
    }

    #[test]
    fn build_report_computes_step_summaries() {
        let plan = make_test_plan();
        let calls = make_test_runtime_calls();
        let evidences: Vec<StepEvidence> = plan
            .steps
            .iter()
            .filter_map(|s| s.evidence.clone())
            .collect();
        let report = build_agent_run_report("run-1", &plan, &calls, &evidences, 10_000);

        assert_eq!(report.step_summaries.len(), 3);
        assert_eq!(report.step_summaries[0].step_id, "step-0");
        assert_eq!(report.step_summaries[0].status, "completed");
        assert_eq!(report.step_summaries[0].provider_calls, 1);
        assert_eq!(report.step_summaries[0].tool_calls, 0);
        assert_eq!(report.step_summaries[0].duration_ms, 1200);

        assert_eq!(report.step_summaries[1].step_id, "step-1");
        assert_eq!(report.step_summaries[1].status, "completed");
        assert_eq!(report.step_summaries[1].provider_calls, 1);
        assert_eq!(report.step_summaries[1].tool_calls, 1);
        assert_eq!(report.step_summaries[1].duration_ms, 3500);

        assert_eq!(report.step_summaries[2].step_id, "step-2");
        assert_eq!(report.step_summaries[2].status, "skipped");
        assert_eq!(report.step_summaries[2].provider_calls, 0);
        assert_eq!(report.step_summaries[2].tool_calls, 0);
        assert_eq!(report.step_summaries[2].duration_ms, 0);
    }

    #[test]
    fn build_report_computes_provider_timeline() {
        let plan = make_test_plan();
        let calls = make_test_runtime_calls();
        let evidences: Vec<StepEvidence> = plan
            .steps
            .iter()
            .filter_map(|s| s.evidence.clone())
            .collect();
        let report = build_agent_run_report("run-1", &plan, &calls, &evidences, 10_000);

        assert_eq!(report.provider_timeline.total_calls, 2);
        assert_eq!(report.provider_timeline.total_duration_ms, 4700);
        assert_eq!(report.provider_timeline.latency_p50_ms, 3500);
        assert_eq!(report.provider_timeline.latency_p90_ms, 3500);
        // avg_ttft_ms is now real TTFT average: (300 + 800) / 2 = 550
        assert_eq!(report.provider_timeline.avg_ttft_ms, 550);
        // avg_call_duration_ms is the average provider call duration: (1200 + 3500) / 2 = 2350
        assert_eq!(report.provider_timeline.avg_call_duration_ms, 2350);
    }

    #[test]
    fn build_report_computes_budget_summary() {
        let plan = make_test_plan();
        let calls = make_test_runtime_calls();
        let evidences: Vec<StepEvidence> = plan
            .steps
            .iter()
            .filter_map(|s| s.evidence.clone())
            .collect();
        let report = build_agent_run_report("run-1", &plan, &calls, &evidences, 10_000);

        assert_eq!(report.budget_summary.total_prompt_tokens, 3000);
        assert_eq!(report.budget_summary.total_completion_tokens, 1500);
        assert_eq!(report.budget_summary.total_tokens, 4500);
        assert!(report.budget_summary.estimated_cost_usd > 0.0);
    }

    #[test]
    fn build_report_has_no_failure_recovery_for_success() {
        let plan = make_test_plan();
        let calls = make_test_runtime_calls();
        let evidences: Vec<StepEvidence> = plan
            .steps
            .iter()
            .filter_map(|s| s.evidence.clone())
            .collect();
        let report = build_agent_run_report("run-1", &plan, &calls, &evidences, 10_000);

        assert!(report.failure_recovery.is_none());
    }

    #[test]
    fn build_report_has_failure_recovery_for_failed_run() {
        let mut plan = make_test_plan();
        plan.status = PlanStatus::Failed {
            failed_step: "step-1".to_string(),
            reason: "rate limit exceeded".to_string(),
        };
        let calls = make_test_runtime_calls();
        let evidences: Vec<StepEvidence> = plan
            .steps
            .iter()
            .filter_map(|s| s.evidence.clone())
            .collect();
        let report = build_agent_run_report("run-1", &plan, &calls, &evidences, 10_000);

        assert!(report.failure_recovery.is_some());
        let fr = report.failure_recovery.unwrap();
        assert_eq!(fr.failed_step, "step-1");
        assert_eq!(fr.failure_kind, "provider_transient");
        assert_eq!(fr.recovery_decision, "retry_with_backoff");
        assert!(!fr.user_choice_required);
    }

    #[test]
    fn build_report_does_not_contain_api_key() {
        let plan = make_test_plan();
        let mut calls = make_test_runtime_calls();
        calls.push(RuntimeCallRecord {
            call_id: "c4".to_string(),
            call_type: RuntimeCallType::ProviderCall,
            step_id: "step-0".to_string(),
            timestamp_ms: 5000,
            input_redacted_summary: "{\"api_key\":\"sk-live-1234567890abcdef\"}".to_string(),
            output_summary: "ok".to_string(),
            duration_ms: 500,
            ttft_ms: Some(100),
            status: RuntimeCallStatus::Success,
            remediation_code: None,
        });
        let evidences: Vec<StepEvidence> = plan
            .steps
            .iter()
            .filter_map(|s| s.evidence.clone())
            .collect();
        let report = build_agent_run_report("run-1", &plan, &calls, &evidences, 10_000);

        let json = serde_json::to_string(&report).unwrap();
        assert!(
            !json.contains("sk-live"),
            "report must not contain raw API key"
        );
    }

    #[test]
    fn build_report_outputs_partial_for_failed_run() {
        let mut plan = make_test_plan();
        plan.status = PlanStatus::Failed {
            failed_step: "step-1".to_string(),
            reason: "timeout".to_string(),
        };
        // Only step-0 completed with evidence; step-1 has no evidence
        plan.steps[1].evidence = None;
        plan.steps[1].status = StepStatus::Failed {
            reason: "timeout".to_string(),
            input_context_summary: vec![],
            recovery_suggestion: "retry".to_string(),
        };
        let calls = make_test_runtime_calls();
        let evidences: Vec<StepEvidence> = plan
            .steps
            .iter()
            .filter_map(|s| s.evidence.clone())
            .collect();
        let report = build_agent_run_report("run-1", &plan, &calls, &evidences, 10_000);

        // Partial report should still have plan summary
        assert_eq!(report.plan_summary.total_steps, 3);
        assert_eq!(report.plan_summary.completed_steps, 1);
        assert_eq!(report.plan_summary.failed_steps, 1);
        // Step summaries should still include all steps
        assert_eq!(report.step_summaries.len(), 3);
        // Provider timeline should still include calls from all steps
        assert_eq!(report.provider_timeline.total_calls, 2);
        // Budget should only count completed steps with evidence
        assert_eq!(report.budget_summary.total_prompt_tokens, 1000);
    }

    #[test]
    fn build_report_step_level_timing_is_correct() {
        let mut plan = make_test_plan();
        // Set explicit completion times
        plan.steps[0].evidence.as_mut().unwrap().completion_time_ms = 1500;
        plan.steps[1].evidence.as_mut().unwrap().completion_time_ms = 2500;
        let calls = make_test_runtime_calls();
        let evidences: Vec<StepEvidence> = plan
            .steps
            .iter()
            .filter_map(|s| s.evidence.clone())
            .collect();
        let report = build_agent_run_report("run-1", &plan, &calls, &evidences, 10_000);

        assert_eq!(report.step_summaries[0].duration_ms, 1500);
        assert_eq!(report.step_summaries[1].duration_ms, 2500);
        assert_eq!(report.step_summaries[2].duration_ms, 0);
        assert_eq!(report.plan_summary.total_duration_ms, 4000);
    }

    #[test]
    fn to_snake_case_converts_camel_case() {
        assert_eq!("ProviderTransient".to_snake_case(), "provider_transient");
        assert_eq!("RetryWithBackoff".to_snake_case(), "retry_with_backoff");
        assert_eq!("SurfaceUserChoice".to_snake_case(), "surface_user_choice");
    }
}
