//! Verifiable task receipts and failure evidence bundles for long Writer Agent work.

use serde::{Deserialize, Serialize};

use super::context::{ContextSource, WritingContextPack};
use super::observation::WriterObservation;

const MAX_TASK_ARTIFACT_CONTENT_CHARS: usize = 12_000;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WriterTaskReceipt {
    pub task_id: String,
    pub task_kind: String,
    pub chapter: Option<String>,
    pub objective: String,
    pub required_evidence: Vec<String>,
    pub expected_artifacts: Vec<String>,
    pub must_not: Vec<String>,
    pub source_refs: Vec<String>,
    pub base_revision: Option<String>,
    pub created_at_ms: u64,
}

include!("task_receipt/receipt_impl.in.rs");

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WriterTaskArtifact {
    pub artifact_id: String,
    pub task_id: String,
    pub task_kind: String,
    pub artifact_kind: String,
    pub chapter: Option<String>,
    pub objective: String,
    pub content: String,
    pub content_char_count: usize,
    pub content_truncated: bool,
    pub required_evidence: Vec<String>,
    pub source_refs: Vec<String>,
    pub base_revision: Option<String>,
    pub created_at_ms: u64,
}

impl WriterTaskArtifact {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        artifact_id: impl Into<String>,
        task_id: impl Into<String>,
        task_kind: impl Into<String>,
        artifact_kind: impl Into<String>,
        chapter: Option<String>,
        objective: impl Into<String>,
        content: impl Into<String>,
        content_char_count: usize,
        content_truncated: bool,
        required_evidence: Vec<String>,
        source_refs: Vec<String>,
        base_revision: Option<String>,
        created_at_ms: u64,
    ) -> Self {
        Self {
            artifact_id: artifact_id.into(),
            task_id: task_id.into(),
            task_kind: task_kind.into(),
            artifact_kind: artifact_kind.into(),
            chapter,
            objective: objective.into(),
            content: content.into(),
            content_char_count,
            content_truncated,
            required_evidence: normalize_strings(required_evidence),
            source_refs: normalize_strings(source_refs),
            base_revision,
            created_at_ms,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WriterTaskReceiptMismatch {
    pub field: String,
    pub expected: String,
    pub actual: String,
    pub evidence_ref: String,
}

impl WriterTaskReceiptMismatch {
    pub fn new(
        field: impl Into<String>,
        expected: impl Into<String>,
        actual: impl Into<String>,
        evidence_ref: impl Into<String>,
    ) -> Self {
        Self {
            field: field.into(),
            expected: expected.into(),
            actual: actual.into(),
            evidence_ref: evidence_ref.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WriterFailureCategory {
    ContextMissing,
    ToolDenied,
    ToolFailed,
    ProviderFailed,
    ReceiptMismatch,
    SaveFailed,
    FeedbackBlocked,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WriterFailureEvidenceBundle {
    pub category: WriterFailureCategory,
    pub code: String,
    pub message: String,
    pub recoverable: bool,
    pub task_id: Option<String>,
    pub evidence_refs: Vec<String>,
    pub details: serde_json::Value,
    pub remediation: Vec<String>,
    pub created_at_ms: u64,
}

impl WriterFailureEvidenceBundle {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        category: WriterFailureCategory,
        code: impl Into<String>,
        message: impl Into<String>,
        recoverable: bool,
        task_id: Option<String>,
        evidence_refs: Vec<String>,
        details: serde_json::Value,
        remediation: Vec<String>,
        created_at_ms: u64,
    ) -> Self {
        Self {
            category,
            code: code.into(),
            message: message.into(),
            recoverable,
            task_id,
            evidence_refs: normalize_strings(evidence_refs),
            details,
            remediation: normalize_strings(remediation),
            created_at_ms,
        }
    }
}

include!("task_receipt/builders.in.rs");
include!("task_receipt/helpers.in.rs");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn receipt_validates_chapter_write_attempt() {
        let receipt = WriterTaskReceipt::new(
            "req-1",
            "ChapterGeneration",
            Some("Chapter-1".to_string()),
            "Draft Chapter-1.",
            vec!["instruction".to_string(), "target_beat".to_string()],
            vec!["saved_chapter".to_string()],
            vec!["overwrite_without_revision_match".to_string()],
            vec![
                "instruction:user".to_string(),
                "target_beat:Chapter-1".to_string(),
            ],
            Some("rev-1".to_string()),
            10,
        );

        assert!(receipt
            .validate_write_attempt("req-1", "Chapter-1", "rev-1", "saved_chapter")
            .is_empty());
        assert_eq!(
            receipt
                .validate_write_attempt("req-2", "Chapter-1", "rev-1", "saved_chapter")
                .first()
                .map(|mismatch| mismatch.field.as_str()),
            Some("task_id")
        );
    }

    #[test]
    fn tool_execution_failure_maps_to_failure_bundle() {
        let execution = agent_harness_core::ToolExecution {
            tool_name: "query_project_brain".to_string(),
            input: serde_json::json!({ "query": "jade" }),
            output: serde_json::Value::Null,
            error: Some("missing binary for external search".to_string()),
            remediation: vec![agent_harness_core::ToolExecutionRemediation {
                code: "missing_binary_or_resource".to_string(),
                message: "Install the configured search binary before retrying.".to_string(),
            }],
            duration_ms: 12,
        };

        let bundle = failure_bundle_from_tool_execution(Some("task-1"), &execution, 20).unwrap();

        assert_eq!(bundle.category, WriterFailureCategory::ToolFailed);
        assert_eq!(bundle.code, "TOOL_MISSING_BINARY_OR_RESOURCE");
        assert!(bundle
            .evidence_refs
            .iter()
            .any(|source| source == "tool:query_project_brain"));
        assert!(bundle
            .remediation
            .iter()
            .any(|item| item.contains("missing_binary_or_resource")));
        assert_eq!(bundle.details["toolName"], "query_project_brain");
    }
}
