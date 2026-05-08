//! Isolated read-only research / diagnostic subtasks for Writer Agent.
//!
//! Subtasks may create evidence artifacts in their own workspace, but they do
//! not emit direct WriterOperations. Their output is evidence that the main
//! Writer Agent loop can later surface in proposals.

use agent_harness_core::{Intent, ToolFilter, ToolPolicyContract, ToolSideEffectLevel};
use serde::{Deserialize, Serialize};
use std::path::{Component, Path, PathBuf};

use super::operation::WriterOperation;
use super::proposal::EvidenceRef;
use super::provider_budget::{
    evaluate_provider_budget, WriterProviderBudgetDecision, WriterProviderBudgetReport,
    WriterProviderBudgetRequest, WriterProviderBudgetTask,
};
use super::task_receipt::{
    failure_bundle_from_tool_execution, WriterFailureCategory, WriterFailureEvidenceBundle,
};

const SUBTASK_ROOT_DIR: &str = "agent_subtasks";
const SUBTASK_ARTIFACT_DIR: &str = "artifacts";
const EXTERNAL_RESEARCH_TOOL_OVERHEAD_TOKENS: u64 = 768;
const DEFAULT_EXTERNAL_RESEARCH_OUTPUT_TOKENS: u64 = 4_096;

include!("research_subtask/types.in.rs");

include!("research_subtask/workspace.in.rs");
include!("research_subtask/provider_budget.in.rs");
include!("research_subtask/helpers.in.rs");

#[cfg(test)]
mod tests {
    use super::*;
    use crate::writer_agent::proposal::{EvidenceRef, EvidenceSource};

    #[test]
    fn subtask_artifact_path_rejects_escape() {
        let root = std::env::temp_dir().join("forge-subtask-path-test");
        assert!(safe_subtask_artifact_path(&root, "research_1", "notes/evidence.json").is_ok());
        assert!(safe_subtask_artifact_path(&root, "../bad", "notes/evidence.json").is_err());
        assert!(safe_subtask_artifact_path(&root, "research_1", "../secret.md").is_err());
        assert!(safe_subtask_artifact_path(&root, "research_1", "notes/../../secret.md").is_err());
    }

    #[test]
    fn evidence_result_blocks_operations() {
        let result = build_evidence_only_subtask_result(
            WriterSubtaskKind::Diagnostic,
            "diag-1",
            "Check mission drift.",
            "No drift found.",
            vec![EvidenceRef {
                source: EvidenceSource::ChapterText,
                reference: "Chapter-1".to_string(),
                snippet: "林墨停在旧门前。".to_string(),
            }],
            vec!["subtask:diag-1:artifact:evidence.json".to_string()],
            &[WriterOperation::TextReplace {
                chapter: "Chapter-1".to_string(),
                from: 0,
                to: 2,
                text: "改写".to_string(),
                revision: "rev-1".to_string(),
            }],
            1,
        )
        .unwrap();

        assert!(validate_evidence_only_subtask_result(&result).is_empty());
        assert_eq!(result.blocked_operation_kinds, vec!["text.replace"]);
    }

    #[test]
    fn subtask_run_event_payloads_redact_evidence_snippets() {
        let workspace = WriterSubtaskWorkspace {
            subtask_id: "research-4".to_string(),
            kind: WriterSubtaskKind::Research,
            workspace_dir: "C:/project/agent_subtasks/research-4".to_string(),
            artifact_dir: "C:/project/agent_subtasks/research-4/artifacts".to_string(),
        };
        let started = subtask_started_payload(
            WriterSubtaskKind::Research,
            &workspace,
            "Find the ring clue.",
        );
        let result = build_evidence_only_subtask_result(
            WriterSubtaskKind::Research,
            "research-4",
            "Find the ring clue.",
            "Project Brain confirms the clue.",
            vec![EvidenceRef {
                source: EvidenceSource::ChapterText,
                reference: "project_brain:chunk-ring".to_string(),
                snippet: "sensitive manuscript evidence".to_string(),
            }],
            vec!["subtask:research-4:artifact:evidence/ring.json".to_string()],
            &[],
            4,
        )
        .unwrap();
        let completed = subtask_completed_payload(&result);

        assert_eq!(started.status, "started");
        assert_eq!(completed.status, "completed");
        assert_eq!(completed.evidence_refs, vec!["project_brain:chunk-ring"]);
        assert!(!serde_json::to_string(&completed)
            .unwrap()
            .contains("sensitive manuscript evidence"));
        assert!(!completed
            .artifact_refs
            .iter()
            .any(|artifact| artifact.contains("C:/")));
    }

    #[test]
    fn external_research_budget_failure_preserves_budget_without_query_text() {
        let input = WriterSubtaskProviderBudgetInput {
            subtask_id: "research-budget-1".to_string(),
            kind: WriterSubtaskKind::Research,
            model: "gpt-4o".to_string(),
            objective: "Verify public evidence without writing memory.".to_string(),
            query: "sensitive ring clue query".repeat(400),
            context_chars: 180_000,
            requested_output_tokens: 12_000,
        };
        let report = external_research_provider_budget_report(&input).unwrap();
        let bundle = failure_bundle_from_subtask_provider_budget(
            WriterSubtaskKind::Research,
            &input.subtask_id,
            &input.objective,
            report,
            vec!["subtask:research-budget-1:artifact:evidence/search.json".to_string()],
            99,
        )
        .unwrap()
        .unwrap();

        assert_eq!(
            bundle.code,
            "RESEARCH_SUBTASK_PROVIDER_BUDGET_APPROVAL_REQUIRED"
        );
        assert_eq!(bundle.task_id.as_deref(), Some("research-budget-1"));
        let serialized = serde_json::to_string(&bundle).unwrap();
        assert!(serialized.contains("providerBudget"));
        assert!(!serialized.contains("sensitive ring clue query"));
    }

    #[test]
    fn subtask_tool_failure_preserves_subtask_evidence() {
        let execution = agent_harness_core::ToolExecution {
            tool_name: "query_project_brain".to_string(),
            input: serde_json::json!({ "query": "ring crack" }),
            output: serde_json::Value::Null,
            error: Some("missing binary for external research adapter".to_string()),
            remediation: vec![agent_harness_core::ToolExecutionRemediation {
                code: "missing_binary_or_resource".to_string(),
                message: "Install the research adapter before retrying.".to_string(),
            }],
            duration_ms: 18,
        };
        let bundle = failure_bundle_from_subtask_tool_execution(
            WriterSubtaskKind::Research,
            "research-3",
            "Find public evidence for the ring crack.",
            &execution,
            vec!["subtask:research-3:artifact:evidence/search.json".to_string()],
            30,
        )
        .unwrap()
        .unwrap();

        assert_eq!(bundle.task_id.as_deref(), Some("research-3"));
        assert!(bundle.message.contains("research subtask"));
        assert!(bundle
            .evidence_refs
            .iter()
            .any(|reference| reference == "subtask:research-3"));
        assert_eq!(bundle.details["kind"], "research");
        assert!(bundle.details["toolExecution"]["remediation"]
            .as_array()
            .is_some_and(|items| !items.is_empty()));
        assert!(bundle
            .remediation
            .iter()
            .any(|item| item.contains("subtask_research_failure")));
    }
}
