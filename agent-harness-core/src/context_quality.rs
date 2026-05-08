use crate::context_pack::PackedContext;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextQualityReport {
    pub request_id: String,
    pub overall_score: f32,
    pub source_coverage: f32,
    pub truncation_risk: f32,
    pub grounding_quality: f32,
    pub missing_evidence: Vec<String>,
    pub warnings: Vec<String>,
    pub recommendation: ContextQualityRecommendation,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextQualityRecommendation {
    Sufficient,
    Supplement { sources: Vec<String> },
    Critical { reason: String },
}

pub fn evaluate_context_quality(
    request_id: &str,
    packed: &PackedContext,
    required_sources: &[String],
) -> ContextQualityReport {
    let present_types: std::collections::HashSet<&str> = packed
        .sources
        .iter()
        .map(|s| s.source_type.as_str())
        .collect();

    let source_coverage = if required_sources.is_empty() {
        1.0
    } else {
        let covered = required_sources
            .iter()
            .filter(|req| present_types.contains(req.as_str()))
            .count();
        covered as f32 / required_sources.len().max(1) as f32
    };

    let missing_evidence: Vec<String> = required_sources
        .iter()
        .filter(|req| !present_types.contains(req.as_str()))
        .cloned()
        .collect();

    let truncation_risk = if packed.sources.is_empty() {
        0.0
    } else {
        packed.sources.iter().filter(|s| s.truncated).count() as f32 / packed.sources.len() as f32
    };

    let core_types = ["outline", "lorebook", "chapter", "project_brain"];
    let diverse_count = core_types
        .iter()
        .filter(|t| present_types.contains(*t))
        .count();
    let grounding_quality = diverse_count as f32 / core_types.len() as f32;

    let overall_score =
        source_coverage * 0.4 + (1.0 - truncation_risk) * 0.35 + grounding_quality * 0.25;

    let mut warnings = Vec::new();
    if truncation_risk > 0.3 {
        warnings.push(format!(
            "{} of {} sources truncated",
            packed.sources.iter().filter(|s| s.truncated).count(),
            packed.sources.len()
        ));
    }

    let recommendation = if overall_score < 0.4 {
        ContextQualityRecommendation::Critical {
            reason: format!(
                "Context quality critically low ({:.0}%). Missing: {}",
                overall_score * 100.0,
                missing_evidence.join(", ")
            ),
        }
    } else if !missing_evidence.is_empty() || truncation_risk > 0.3 {
        ContextQualityRecommendation::Supplement {
            sources: missing_evidence.clone(),
        }
    } else {
        ContextQualityRecommendation::Sufficient
    };

    ContextQualityReport {
        request_id: request_id.to_string(),
        overall_score,
        source_coverage,
        truncation_risk,
        grounding_quality,
        missing_evidence,
        warnings,
        recommendation,
    }
}

#[cfg(test)]
mod context_quality_tests {
    use super::*;
    use crate::context_pack::{ContextBudgetReport, ContextSourceReport, PackedContext};

    fn make_packed(types: &[&str], truncated_mask: &[bool]) -> PackedContext {
        PackedContext {
            text: "test".into(),
            sources: types
                .iter()
                .enumerate()
                .map(|(i, t)| ContextSourceReport {
                    source_type: t.to_string(),
                    id: format!("s{}", i),
                    label: t.to_string(),
                    original_chars: 100,
                    included_chars: 100,
                    truncated: truncated_mask.get(i).copied().unwrap_or(false),
                    score: None,
                })
                .collect(),
            budget: ContextBudgetReport {
                max_chars: 1000,
                included_chars: 100,
                source_count: types.len(),
                truncated_source_count: truncated_mask.iter().filter(|&&t| t).count(),
                warnings: vec![],
            },
        }
    }

    #[test]
    fn empty_input_returns_sufficient() {
        let packed = make_packed(&[], &[]);
        let report = evaluate_context_quality("r1", &packed, &[]);
        assert_eq!(
            report.recommendation,
            ContextQualityRecommendation::Sufficient
        );
        assert!((report.overall_score - 0.75).abs() < 0.01);
    }

    #[test]
    fn missing_required_source_detected() {
        let packed = make_packed(&["outline"], &[false]);
        let report =
            evaluate_context_quality("r2", &packed, &["outline".into(), "lorebook".into()]);
        assert_eq!(report.source_coverage, 0.5);
        assert!(report.missing_evidence.contains(&"lorebook".to_string()));
    }

    #[test]
    fn truncation_risk_detected() {
        let packed = make_packed(&["outline", "lorebook"], &[true, false]);
        let report = evaluate_context_quality("r3", &packed, &[]);
        assert!(report.truncation_risk > 0.0);
        assert!(!report.warnings.is_empty());
    }

    #[test]
    fn all_sources_present_all_clean() {
        let packed = make_packed(
            &["outline", "lorebook", "chapter", "project_brain"],
            &[false; 4],
        );
        let required: Vec<String> = ["outline", "lorebook", "chapter"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let report = evaluate_context_quality("r4", &packed, &required);
        assert_eq!(report.source_coverage, 1.0);
        assert_eq!(report.grounding_quality, 1.0);
        assert!(report.missing_evidence.is_empty());
    }

    #[test]
    fn critical_when_score_below_threshold() {
        let packed = make_packed(&[], &[]);
        let report = evaluate_context_quality(
            "r5",
            &packed,
            &["outline".into(), "lorebook".into(), "chapter".into()],
        );
        assert!(matches!(
            report.recommendation,
            ContextQualityRecommendation::Critical { .. }
        ));
    }
}
