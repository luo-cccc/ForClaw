//! Post-write diagnostic reports for saved chapter text.
//!
//! The report is generated from the same diagnostics that produce story review
//! proposals. It gives the run timeline a compact, replayable record that a
//! durable write was followed by continuity/contract/mission checks.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::diagnostics::{DiagnosticCategory, DiagnosticResult, DiagnosticSeverity};
use super::observation::{TextRange, WriterObservation};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WriterPostWriteDiagnosticReport {
    pub observation_id: String,
    pub chapter_title: Option<String>,
    pub chapter_revision: Option<String>,
    pub total_count: usize,
    pub error_count: usize,
    pub warning_count: usize,
    pub info_count: usize,
    pub categories: Vec<WriterPostWriteDiagnosticCategoryCount>,
    pub diagnostics: Vec<WriterPostWriteDiagnosticItem>,
    pub source_refs: Vec<String>,
    pub remediation: Vec<String>,
    pub created_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WriterPostWriteDiagnosticCategoryCount {
    pub category: String,
    pub count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WriterPostWriteDiagnosticItem {
    pub diagnostic_id: String,
    pub severity: DiagnosticSeverity,
    pub category: DiagnosticCategory,
    pub message: String,
    pub target: TextRange,
    pub evidence_refs: Vec<String>,
    pub fix_suggestion: Option<String>,
    pub operation_count: usize,
}

pub fn build_post_write_diagnostic_report(
    observation: &WriterObservation,
    diagnostics: &[DiagnosticResult],
    created_at_ms: u64,
) -> WriterPostWriteDiagnosticReport {
    let mut category_counts = BTreeMap::<String, usize>::new();
    let mut source_refs = vec![observation.id.clone()];
    if let Some(chapter) = observation.chapter_title.as_ref() {
        source_refs.push(format!("chapter:{}", chapter));
    }
    if let Some(revision) = observation.chapter_revision.as_ref() {
        source_refs.push(format!("revision:{}", revision));
    }

    let mut error_count = 0usize;
    let mut warning_count = 0usize;
    let mut info_count = 0usize;
    let mut items = Vec::new();

    for diagnostic in diagnostics {
        match diagnostic.severity {
            DiagnosticSeverity::Error => error_count += 1,
            DiagnosticSeverity::Warning => warning_count += 1,
            DiagnosticSeverity::Info => info_count += 1,
        }
        *category_counts
            .entry(format!("{:?}", diagnostic.category))
            .or_default() += 1;

        let evidence_refs = diagnostic
            .evidence
            .iter()
            .map(|evidence| format!("{}:{}", evidence.source, evidence.reference))
            .collect::<Vec<_>>();
        source_refs.extend(evidence_refs.iter().cloned());

        items.push(WriterPostWriteDiagnosticItem {
            diagnostic_id: diagnostic.id.clone(),
            severity: diagnostic.severity.clone(),
            category: diagnostic.category.clone(),
            message: diagnostic.message.clone(),
            target: TextRange {
                from: diagnostic.from,
                to: diagnostic.to,
            },
            evidence_refs,
            fix_suggestion: diagnostic.fix_suggestion.clone(),
            operation_count: diagnostic.operations.len(),
        });
    }

    WriterPostWriteDiagnosticReport {
        observation_id: observation.id.clone(),
        chapter_title: observation.chapter_title.clone(),
        chapter_revision: observation.chapter_revision.clone(),
        total_count: diagnostics.len(),
        error_count,
        warning_count,
        info_count,
        categories: category_counts
            .into_iter()
            .map(|(category, count)| WriterPostWriteDiagnosticCategoryCount { category, count })
            .collect(),
        diagnostics: items,
        source_refs: normalize_source_refs(source_refs),
        remediation: remediation_for_counts(error_count, warning_count, info_count),
        created_at_ms,
    }
}

fn remediation_for_counts(
    error_count: usize,
    warning_count: usize,
    info_count: usize,
) -> Vec<String> {
    let mut remediation = Vec::new();
    if error_count > 0 {
        remediation.push(
            "Block automatic follow-up writes until urgent post-write diagnostics are reviewed."
                .to_string(),
        );
    }
    if warning_count > 0 {
        remediation.push(
            "Queue continuity, mission, or promise warnings for the story review surface."
                .to_string(),
        );
    }
    if error_count == 0 && warning_count == 0 && info_count > 0 {
        remediation.push(
            "Keep informational promise/style opportunities visible without blocking the saved draft."
                .to_string(),
        );
    }
    if remediation.is_empty() {
        remediation.push("No post-write remediation required.".to_string());
    }
    remediation
}

fn normalize_source_refs(source_refs: Vec<String>) -> Vec<String> {
    let mut normalized = Vec::new();
    for source_ref in source_refs {
        let source_ref = source_ref.trim();
        if source_ref.is_empty() || normalized.iter().any(|existing| existing == source_ref) {
            continue;
        }
        normalized.push(source_ref.to_string());
    }
    normalized
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::writer_agent::diagnostics::DiagnosticEvidence;
    use crate::writer_agent::observation::{ObservationReason, ObservationSource};

    #[test]
    fn report_counts_severities_and_sources() {
        let observation = WriterObservation {
            id: "save-1".to_string(),
            created_at: 10,
            source: ObservationSource::ChapterSave,
            reason: ObservationReason::Save,
            project_id: "project".to_string(),
            chapter_title: Some("Chapter-3".to_string()),
            chapter_revision: Some("rev-3".to_string()),
            cursor: None,
            selection: None,
            prefix: String::new(),
            suffix: String::new(),
            paragraph: "林墨拔出长剑。".to_string(),
            full_text_digest: None,
            editor_dirty: false,
        };
        let diagnostics = vec![DiagnosticResult {
            id: "diag-1".to_string(),
            severity: DiagnosticSeverity::Error,
            category: DiagnosticCategory::CanonConflict,
            message: "weapon conflict".to_string(),
            entity_name: Some("林墨".to_string()),
            from: 4,
            to: 6,
            evidence: vec![DiagnosticEvidence {
                source: "canon".to_string(),
                reference: "林墨".to_string(),
                snippet: "weapon=寒影刀".to_string(),
            }],
            fix_suggestion: Some("改回寒影刀".to_string()),
            operations: Vec::new(),
        }];

        let report = build_post_write_diagnostic_report(&observation, &diagnostics, 12);

        assert_eq!(report.total_count, 1);
        assert_eq!(report.error_count, 1);
        assert!(report
            .source_refs
            .contains(&"chapter:Chapter-3".to_string()));
        assert!(report.source_refs.contains(&"canon:林墨".to_string()));
        assert!(!report.remediation.is_empty());
    }
}
