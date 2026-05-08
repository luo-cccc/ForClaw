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
        }
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

    let mut reasons = Vec::new();
    if estimated_total_tokens > request.max_total_tokens_without_approval {
        reasons.push(format!(
            "estimated tokens {} exceed approval-free limit {}",
            estimated_total_tokens, request.max_total_tokens_without_approval
        ));
    }
    if estimated_cost_micros > request.max_estimated_cost_micros_without_approval {
        reasons.push(format!(
            "estimated cost {} micros exceeds approval-free limit {}",
            estimated_cost_micros, request.max_estimated_cost_micros_without_approval
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
    ) && estimated_total_tokens
        >= request.max_total_tokens_without_approval * 4 / 5;
    if high_risk_long_task {
        reasons.push("long-running provider task is near approval-free budget".to_string());
    }

    let decision = if estimated_total_tokens == 0 {
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
}
