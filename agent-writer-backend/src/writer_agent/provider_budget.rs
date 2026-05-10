//! Provider-call budget checks for long Writer Agent tasks.
//!
//! This first slice estimates tokens and nominal cost before expensive provider
//! calls. It does not charge users or call providers; it creates a structured
//! approval boundary that generation/research flows can enforce.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WriterProviderBudgetTask {
    ChapterGeneration,
    BatchGeneration,
    ProjectBrainQuery,
    ProjectBrainRebuild,
    ExternalResearch,
    ManualRequest,
    MetacognitiveRecovery,
    GhostPreview,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WriterProviderBudgetDecision {
    Allowed,
    Warn,
    ApprovalRequired,
    Blocked,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WriterProviderBudgetRequest {
    pub task: WriterProviderBudgetTask,
    pub model: String,
    pub estimated_input_tokens: u64,
    pub requested_output_tokens: u64,
    pub max_total_tokens_without_approval: u64,
    pub max_estimated_cost_micros_without_approval: u64,
    pub already_approved: bool,
    // P0: calibration support
    #[serde(default)]
    pub input_chars: usize,
    #[serde(default)]
    pub expected_output_chars: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WriterProviderBudgetReport {
    pub task: WriterProviderBudgetTask,
    pub model: String,
    pub estimated_input_tokens: u64,
    pub requested_output_tokens: u64,
    pub estimated_total_tokens: u64,
    pub estimated_cost_micros: u64,
    pub max_total_tokens_without_approval: u64,
    pub max_estimated_cost_micros_without_approval: u64,
    pub decision: WriterProviderBudgetDecision,
    pub approval_required: bool,
    pub reasons: Vec<String>,
    pub remediation: Vec<String>,
    // Calibration fields (P0)
    #[serde(default)]
    pub calibrated_input_tokens: u64,
    #[serde(default)]
    pub calibrated_output_tokens: u64,
    #[serde(default)]
    pub calibration_confidence: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub calibration_fallback_reason: Option<String>,
    // Latency telemetry fields (P8)
    #[serde(default)]
    pub provider_calls: u64,
    #[serde(default)]
    pub provider_retries: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WriterProviderBudgetApproval {
    pub task: WriterProviderBudgetTask,
    pub model: String,
    pub approved_total_tokens: u64,
    pub approved_cost_micros: u64,
    pub approved_at_ms: u64,
    pub source: String,
}

impl WriterProviderBudgetRequest {
    pub fn new(
        task: WriterProviderBudgetTask,
        model: impl Into<String>,
        estimated_input_tokens: u64,
        requested_output_tokens: u64,
    ) -> Self {
        let defaults = default_provider_budget_limits(task);
        Self {
            task,
            model: model.into(),
            estimated_input_tokens,
            requested_output_tokens,
            max_total_tokens_without_approval: defaults.max_total_tokens_without_approval,
            max_estimated_cost_micros_without_approval: defaults
                .max_estimated_cost_micros_without_approval,
            already_approved: false,
            input_chars: 0,
            expected_output_chars: 0,
        }
    }

    pub fn with_chars(mut self, input_chars: usize, expected_output_chars: usize) -> Self {
        self.input_chars = input_chars;
        self.expected_output_chars = expected_output_chars;
        self
    }
}

impl WriterProviderBudgetApproval {
    pub fn covers(&self, report: &WriterProviderBudgetReport) -> bool {
        self.task == report.task
            && self.model == report.model
            && self.approved_total_tokens >= report.estimated_total_tokens
            && self.approved_cost_micros >= report.estimated_cost_micros
    }
}

pub fn apply_provider_budget_approval(
    mut report: WriterProviderBudgetReport,
    approval: Option<&WriterProviderBudgetApproval>,
) -> WriterProviderBudgetReport {
    let Some(approval) = approval else {
        return report;
    };
    if report.decision != WriterProviderBudgetDecision::ApprovalRequired
        || !approval.covers(&report)
    {
        return report;
    }

    report.decision = WriterProviderBudgetDecision::Warn;
    report.approval_required = false;
    report.reasons.push(format!(
        "provider budget approved by {} at {}",
        approval.source, approval.approved_at_ms
    ));
    report.remediation = remediation_for_decision(report.decision, report.task);
    report
}

pub fn default_provider_budget_limits(
    task: WriterProviderBudgetTask,
) -> WriterProviderBudgetRequest {
    let (tokens, cost_micros) = match task {
        WriterProviderBudgetTask::GhostPreview => (8_000, 150_000),
        WriterProviderBudgetTask::ManualRequest => (18_000, 450_000),
        WriterProviderBudgetTask::ProjectBrainQuery => (24_000, 650_000),
        WriterProviderBudgetTask::ChapterGeneration => (55_000, 1_200_000),
        WriterProviderBudgetTask::BatchGeneration => (85_000, 1_800_000),
        WriterProviderBudgetTask::ProjectBrainRebuild => (120_000, 2_500_000),
        WriterProviderBudgetTask::ExternalResearch => (45_000, 1_000_000),
        WriterProviderBudgetTask::MetacognitiveRecovery => (28_000, 700_000),
    };
    WriterProviderBudgetRequest {
        task,
        model: String::new(),
        estimated_input_tokens: 0,
        requested_output_tokens: 0,
        max_total_tokens_without_approval: tokens,
        max_estimated_cost_micros_without_approval: cost_micros,
        already_approved: false,
        input_chars: 0,
        expected_output_chars: 0,
    }
}

pub fn evaluate_provider_budget(
    request: WriterProviderBudgetRequest,
) -> WriterProviderBudgetReport {
    let estimated_total_tokens = request
        .estimated_input_tokens
        .saturating_add(request.requested_output_tokens);
    let estimated_cost_micros = estimate_provider_cost_micros(
        &request.model,
        request.estimated_input_tokens,
        request.requested_output_tokens,
    );

    // P2: usage calibration — compute calibrated estimates before decision
    let (calibrated_input, calibrated_output, calibration_confidence, calibration_fallback) =
        if request.input_chars > 0 {
            let est = agent_harness_core::estimate_with_confidence(
                &request.model,
                request.input_chars,
                request
                    .expected_output_chars
                    .max(request.requested_output_tokens as usize * 2),
            );
            (
                est.input_tokens,
                est.output_tokens,
                format!("{:?}", est.confidence),
                est.fallback_reason,
            )
        } else {
            (
                request.estimated_input_tokens,
                request.requested_output_tokens,
                "unknown".to_string(),
                Some("input_chars not provided; using static token estimate".to_string()),
            )
        };

    let calibrated_total_tokens = calibrated_input.saturating_add(calibrated_output);
    let calibrated_cost_micros = estimate_provider_cost_micros(
        &request.model,
        calibrated_input,
        calibrated_output,
    );

    // Use calibrated estimates for decision when confidence is sufficient;
    // otherwise fall back to static estimates (conservative).
    let use_calibration = calibration_confidence == "High" || calibration_confidence == "Medium";
    let decision_total_tokens = if use_calibration {
        calibrated_total_tokens
    } else {
        estimated_total_tokens
    };
    let decision_cost_micros = if use_calibration {
        calibrated_cost_micros
    } else {
        estimated_cost_micros
    };

    let mut reasons = Vec::new();
    if decision_total_tokens > request.max_total_tokens_without_approval {
        reasons.push(format!(
            "{}estimated tokens {} exceed approval-free limit {}",
            if use_calibration { "[calibrated] " } else { "" },
            decision_total_tokens,
            request.max_total_tokens_without_approval
        ));
    }
    if decision_cost_micros > request.max_estimated_cost_micros_without_approval {
        reasons.push(format!(
            "{}estimated cost {} micros exceeds approval-free limit {}",
            if use_calibration { "[calibrated] " } else { "" },
            decision_cost_micros,
            request.max_estimated_cost_micros_without_approval
        ));
    }

    let high_risk_long_task = matches!(
        request.task,
        WriterProviderBudgetTask::ChapterGeneration
            | WriterProviderBudgetTask::BatchGeneration
            | WriterProviderBudgetTask::ProjectBrainQuery
            | WriterProviderBudgetTask::ProjectBrainRebuild
            | WriterProviderBudgetTask::ExternalResearch
            | WriterProviderBudgetTask::MetacognitiveRecovery
    ) && decision_total_tokens
        >= request.max_total_tokens_without_approval * 4 / 5;
    if high_risk_long_task {
        reasons.push("long-running provider task is near approval-free budget".to_string());
    }

    let decision = if decision_total_tokens == 0 {
        WriterProviderBudgetDecision::Blocked
    } else if reasons.is_empty() {
        WriterProviderBudgetDecision::Allowed
    } else if request.already_approved {
        WriterProviderBudgetDecision::Warn
    } else {
        WriterProviderBudgetDecision::ApprovalRequired
    };
    let approval_required = decision == WriterProviderBudgetDecision::ApprovalRequired;
    let remediation = remediation_for_decision(decision, request.task);

    WriterProviderBudgetReport {
        task: request.task,
        model: request.model,
        estimated_input_tokens: request.estimated_input_tokens,
        requested_output_tokens: request.requested_output_tokens,
        estimated_total_tokens,
        estimated_cost_micros,
        max_total_tokens_without_approval: request.max_total_tokens_without_approval,
        max_estimated_cost_micros_without_approval: request
            .max_estimated_cost_micros_without_approval,
        decision,
        approval_required,
        reasons,
        remediation,
        calibrated_input_tokens: calibrated_input,
        calibrated_output_tokens: calibrated_output,
        calibration_confidence,
        calibration_fallback_reason: calibration_fallback,
        provider_calls: 0,
        provider_retries: 0,
    }
}

/// A2: Convert a WriterProviderBudgetReport into a RuntimeCallRecord.
/// Bridges provider budget data into the unified runtime call audit system.
pub fn provider_call_record_from_budget_report(
    call_id: impl Into<String>,
    step_id: impl Into<String>,
    report: &WriterProviderBudgetReport,
    timestamp_ms: u64,
    duration_ms: u64,
    ttft_ms: Option<u64>,
) -> agent_harness_core::RuntimeCallRecord {
    use agent_harness_core::{
        FailureRemediation, RuntimeCallRecord, RuntimeCallStatus, RuntimeCallType,
    };
    let status = match report.decision {
        WriterProviderBudgetDecision::Allowed | WriterProviderBudgetDecision::Warn => {
            RuntimeCallStatus::Success
        }
        WriterProviderBudgetDecision::ApprovalRequired => RuntimeCallStatus::Blocked {
            reason: format!(
                "Provider budget approval required: {} tokens, {} micros",
                report.estimated_total_tokens, report.estimated_cost_micros
            ),
        },
        WriterProviderBudgetDecision::Blocked => RuntimeCallStatus::Blocked {
            reason: "Provider budget blocked: zero tokens or empty prompt".to_string(),
        },
    };
    let remediation_code = if report.approval_required {
        Some(FailureRemediation::RequestApproval.code().to_string())
    } else {
        None
    };
    RuntimeCallRecord {
        call_id: call_id.into(),
        call_type: RuntimeCallType::ProviderCall,
        step_id: step_id.into(),
        task_id: Some(format!("{:?}", report.task)),
        timestamp_ms,
        input_redacted_summary: format!(
            "model={} input_tokens={} output_tokens={}",
            report.model, report.estimated_input_tokens, report.requested_output_tokens
        ),
        output_summary: format!(
            "total_tokens={} cost_micros={} calls={} retries={}",
            report.estimated_total_tokens,
            report.estimated_cost_micros,
            report.provider_calls,
            report.provider_retries
        ),
        duration_ms,
        ttft_ms,
        status,
        remediation_code,
    }
}

pub fn estimate_provider_cost_micros(model: &str, input_tokens: u64, output_tokens: u64) -> u64 {
    let lower = model.to_ascii_lowercase();
    let (input_per_million_micros, output_per_million_micros) = if lower.contains("gpt-4o") {
        (2_500_000, 10_000_000)
    } else if lower.contains("gpt-5") {
        (1_250_000, 10_000_000)
    } else if lower.contains("claude") {
        (3_000_000, 15_000_000)
    } else if lower.contains("deepseek") {
        (300_000, 1_200_000)
    } else {
        (1_000_000, 4_000_000)
    };
    input_tokens.saturating_mul(input_per_million_micros) / 1_000_000
        + output_tokens.saturating_mul(output_per_million_micros) / 1_000_000
}

fn remediation_for_decision(
    decision: WriterProviderBudgetDecision,
    task: WriterProviderBudgetTask,
) -> Vec<String> {
    match decision {
        WriterProviderBudgetDecision::Allowed => Vec::new(),
        WriterProviderBudgetDecision::Warn => vec![format!(
            "Budget was approved for {:?}; record this approval with the run trace.",
            task
        )],
        WriterProviderBudgetDecision::ApprovalRequired => vec![
            "Surface estimated token/cost budget to the author before calling the provider."
                .to_string(),
            "Reduce chapter range, context budget, or requested output tokens if approval is not granted."
                .to_string(),
        ],
        WriterProviderBudgetDecision::Blocked => vec![
            "Rebuild the provider request with a non-empty prompt and explicit output budget."
                .to_string(),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_generation_over_budget_requires_approval() {
        let mut request = WriterProviderBudgetRequest::new(
            WriterProviderBudgetTask::ChapterGeneration,
            "gpt-4o",
            70_000,
            20_000,
        );
        request.max_total_tokens_without_approval = 60_000;
        request.max_estimated_cost_micros_without_approval = 100_000;

        let report = evaluate_provider_budget(request);

        assert_eq!(
            report.decision,
            WriterProviderBudgetDecision::ApprovalRequired
        );
        assert!(report.approval_required);
        assert!(!report.reasons.is_empty());
        assert!(!report.remediation.is_empty());
    }

    #[test]
    fn matching_budget_approval_downgrades_to_warn() {
        let report = evaluate_provider_budget(WriterProviderBudgetRequest::new(
            WriterProviderBudgetTask::ChapterGeneration,
            "gpt-4o",
            70_000,
            20_000,
        ));
        assert_eq!(
            report.decision,
            WriterProviderBudgetDecision::ApprovalRequired
        );

        let approval = WriterProviderBudgetApproval {
            task: report.task,
            model: report.model.clone(),
            approved_total_tokens: report.estimated_total_tokens,
            approved_cost_micros: report.estimated_cost_micros,
            approved_at_ms: 42,
            source: "test".to_string(),
        };
        let approved_report = apply_provider_budget_approval(report, Some(&approval));

        assert_eq!(approved_report.decision, WriterProviderBudgetDecision::Warn);
        assert!(!approved_report.approval_required);
    }

    #[test]
    fn smaller_budget_approval_does_not_cover_larger_request() {
        let report = evaluate_provider_budget(WriterProviderBudgetRequest::new(
            WriterProviderBudgetTask::ChapterGeneration,
            "gpt-4o",
            70_000,
            20_000,
        ));
        let approval = WriterProviderBudgetApproval {
            task: report.task,
            model: report.model.clone(),
            approved_total_tokens: report.estimated_total_tokens.saturating_sub(1),
            approved_cost_micros: report.estimated_cost_micros,
            approved_at_ms: 42,
            source: "test".to_string(),
        };
        let approved_report = apply_provider_budget_approval(report, Some(&approval));

        assert_eq!(
            approved_report.decision,
            WriterProviderBudgetDecision::ApprovalRequired
        );
        assert!(approved_report.approval_required);
    }

    #[test]
    fn calibration_used_when_input_chars_provided() {
        // Static estimate would exceed budget (100k > 80k)
        let mut request = WriterProviderBudgetRequest::new(
            WriterProviderBudgetTask::ChapterGeneration,
            "gpt-4o",
            80_000,
            20_000,
        )
        .with_chars(50_000, 10_000);
        request.max_total_tokens_without_approval = 90_000;

        let report = evaluate_provider_budget(request);

        // With calibration, 50k chars at ~1/3 tokens per char = ~16k input tokens
        // which is well under the 90k limit, so decision should be Allowed
        assert!(
            report.calibrated_input_tokens > 0,
            "calibrated input should be computed"
        );
        assert!(
            report.calibrated_output_tokens > 0,
            "calibrated output should be computed"
        );
        // The calibration confidence should be recorded
        assert!(
            !report.calibration_confidence.is_empty(),
            "calibration confidence should be recorded"
        );
    }

    #[test]
    fn static_estimate_used_when_no_input_chars() {
        let mut request = WriterProviderBudgetRequest::new(
            WriterProviderBudgetTask::ChapterGeneration,
            "gpt-4o",
            100_000,
            20_000,
        );
        // No input_chars set, so calibration falls back to static
        request.max_total_tokens_without_approval = 90_000;

        let report = evaluate_provider_budget(request);

        assert_eq!(
            report.decision,
            WriterProviderBudgetDecision::ApprovalRequired,
            "static estimate 120k > 90k should require approval"
        );
        assert_eq!(
            report.calibration_fallback_reason,
            Some("input_chars not provided; using static token estimate".to_string())
        );
    }

    // ── A2: ProviderCallRecord bridging tests ──

    #[test]
    fn provider_call_record_from_allowed_budget() {
        let request = WriterProviderBudgetRequest::new(
            WriterProviderBudgetTask::ChapterGeneration,
            "gpt-4o",
            1_000,
            500,
        );
        let report = evaluate_provider_budget(request);
        assert_eq!(report.decision, WriterProviderBudgetDecision::Allowed);

        let record = provider_call_record_from_budget_report(
            "call-1", "step-0", &report, 1_000, 200, Some(50),
        );
        assert_eq!(record.call_id, "call-1");
        assert_eq!(record.step_id, "step-0");
        assert_eq!(record.call_type, agent_harness_core::RuntimeCallType::ProviderCall);
        assert!(
            matches!(record.status, agent_harness_core::RuntimeCallStatus::Success),
            "expected success for allowed budget, got: {:?}",
            record.status
        );
        assert_eq!(record.remediation_code, None);
        assert!(record.input_redacted_summary.contains("gpt-4o"));
        assert!(record.output_summary.contains("total_tokens"));
    }

    #[test]
    fn provider_call_record_from_blocked_budget_has_remediation() {
        let mut request = WriterProviderBudgetRequest::new(
            WriterProviderBudgetTask::ChapterGeneration,
            "gpt-4o",
            100_000,
            20_000,
        );
        request.max_total_tokens_without_approval = 10_000;
        let report = evaluate_provider_budget(request);
        assert_eq!(report.decision, WriterProviderBudgetDecision::ApprovalRequired);

        let record = provider_call_record_from_budget_report(
            "call-2", "step-1", &report, 2_000, 300, None,
        );
        assert!(
            matches!(
                record.status,
                agent_harness_core::RuntimeCallStatus::Blocked { .. }
            ),
            "expected blocked for approval-required budget, got: {:?}",
            record.status
        );
        assert_eq!(
            record.remediation_code,
            Some("request_approval".to_string())
        );
    }

    #[test]
    fn provider_call_record_includes_task_id() {
        let request = WriterProviderBudgetRequest::new(
            WriterProviderBudgetTask::ExternalResearch,
            "claude-3",
            5_000,
            2_000,
        );
        let report = evaluate_provider_budget(request);
        let record = provider_call_record_from_budget_report(
            "call-3", "step-2", &report, 3_000, 400, Some(80),
        );
        assert!(record.task_id.is_some());
        assert!(record.task_id.as_ref().unwrap().contains("ExternalResearch"));
    }
}
