use crate::context_pack::PackedContext;
use crate::execution_plan::StepFailureAction;
use serde::{Deserialize, Serialize};

/// Evidence of a low-value source being truncated, preserving the chain of custody.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TruncationEvidence {
    pub source_type: String,
    pub id: String,
    pub reason: String,
    pub original_chars: usize,
    pub included_chars: usize,
}

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
    /// Evidence chain for truncated low-value sources.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub truncation_evidence: Vec<TruncationEvidence>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextQualityRecommendation {
    Sufficient,
    Supplement {
        sources: Vec<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        actions: Vec<String>,
    },
    Critical {
        reason: String,
    },
}

impl ContextQualityRecommendation {
    /// Map quality recommendation directly to a step failure action.
    /// - Critical -> blocked (Stop)
    /// - Supplement -> request context supplement
    /// - Sufficient -> no action (None)
    pub fn to_step_failure_action(&self) -> Option<StepFailureAction> {
        match self {
            ContextQualityRecommendation::Critical { .. } => Some(StepFailureAction::Stop),
            ContextQualityRecommendation::Supplement { sources, .. } => {
                Some(StepFailureAction::RequestContextSupplement {
                    sources: sources.clone(),
                })
            }
            ContextQualityRecommendation::Sufficient => None,
        }
    }
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

    let actions = action_codes_for_missing_sources(&missing_evidence, truncation_risk);
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
            actions,
        }
    } else {
        ContextQualityRecommendation::Sufficient
    };

    // Build truncation evidence chain for low-value truncated sources.
    let truncation_evidence: Vec<TruncationEvidence> = packed
        .sources
        .iter()
        .filter(|s| s.truncated)
        .map(|s| TruncationEvidence {
            source_type: s.source_type.clone(),
            id: s.id.clone(),
            reason: format!(
                "truncated from {} to {} chars (budget pressure)",
                s.original_chars, s.included_chars
            ),
            original_chars: s.original_chars,
            included_chars: s.included_chars,
        })
        .collect();

    ContextQualityReport {
        request_id: request_id.to_string(),
        overall_score,
        source_coverage,
        truncation_risk,
        grounding_quality,
        missing_evidence,
        warnings,
        recommendation,
        truncation_evidence,
    }
}

fn action_codes_for_missing_sources(
    missing_sources: &[String],
    truncation_risk: f32,
) -> Vec<String> {
    let mut actions = Vec::new();
    for source in missing_sources {
        let action = match source.as_str() {
            "project_brief" => "fetch_project_brain_anchor",
            "previous_chapter" => "refresh_prior_chapter_summary",
            "outline" => "refresh_outline_anchor",
            "canon" => "fetch_canon_anchor",
            "promise" => "fetch_promise_anchor",
            "chapter_mission" => "refresh_chapter_mission",
            "next_beat" => "refresh_next_beat",
            "lorebook" => "reduce_low_value_lore",
            _ => "supplement_context",
        };
        if !actions.contains(&action.to_string()) {
            actions.push(action.to_string());
        }
    }
    if truncation_risk > 0.3 && !actions.contains(&"reduce_low_value_lore".to_string()) {
        actions.push("reduce_low_value_lore".to_string());
    }
    actions
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
                    included_chars: if truncated_mask.get(i).copied().unwrap_or(false) {
                        30
                    } else {
                        100
                    },
                    truncated: truncated_mask.get(i).copied().unwrap_or(false),
                    score: None,
                    taxonomy: String::new(),
                    role: String::new(),
                    elapsed_ms: 0,
                    retrieval_status: String::new(),
                })
                .collect(),
            budget: ContextBudgetReport {
                max_chars: 1000,
                included_chars: 100,
                source_count: types.len(),
                truncated_source_count: truncated_mask.iter().filter(|&&t| t).count(),
                warnings: vec![],
            },
            context_hash: String::new(),
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

    #[test]
    fn missing_sources_generate_action_codes() {
        let packed = make_packed(&["outline"], &[false]);
        let report = evaluate_context_quality(
            "r6",
            &packed,
            &[
                "outline".into(),
                "project_brief".into(),
                "previous_chapter".into(),
            ],
        );
        match report.recommendation {
            ContextQualityRecommendation::Supplement { actions, .. } => {
                assert!(
                    actions.contains(&"fetch_project_brain_anchor".to_string()),
                    "missing project_brief should trigger fetch_project_brain_anchor"
                );
                assert!(
                    actions.contains(&"refresh_prior_chapter_summary".to_string()),
                    "missing previous_chapter should trigger refresh_prior_chapter_summary"
                );
            }
            other => panic!("expected Supplement recommendation, got {:?}", other),
        }
    }

    #[test]
    fn truncation_risk_generates_reduce_lore_action() {
        let packed = make_packed(&["outline", "lorebook", "chapter"], &[true, true, true]);
        let report = evaluate_context_quality("r7", &packed, &[]);
        match report.recommendation {
            ContextQualityRecommendation::Supplement { actions, .. } => {
                assert!(
                    actions.contains(&"reduce_low_value_lore".to_string()),
                    "high truncation risk should trigger reduce_low_value_lore"
                );
            }
            other => panic!("expected Supplement recommendation, got {:?}", other),
        }
    }

    #[test]
    fn truncation_evidence_chain_is_preserved() {
        let packed = make_packed(&["outline", "lorebook", "chapter"], &[true, false, true]);
        let report = evaluate_context_quality("r8", &packed, &[]);
        assert_eq!(report.truncation_evidence.len(), 2);
        assert!(report
            .truncation_evidence
            .iter()
            .any(|e| e.source_type == "outline"));
        assert!(report
            .truncation_evidence
            .iter()
            .any(|e| e.source_type == "chapter"));
        assert!(!report
            .truncation_evidence
            .iter()
            .any(|e| e.source_type == "lorebook"));
        let outline_ev = report
            .truncation_evidence
            .iter()
            .find(|e| e.source_type == "outline")
            .unwrap();
        assert_eq!(outline_ev.original_chars, 100);
        assert_eq!(outline_ev.included_chars, 30);
        assert!(outline_ev.reason.contains("truncated"));
    }

    #[test]
    fn critical_recommendation_maps_to_stop_action() {
        let packed = make_packed(&[], &[]);
        let report = evaluate_context_quality(
            "r9",
            &packed,
            &["outline".into(), "lorebook".into(), "chapter".into()],
        );
        let action = report.recommendation.to_step_failure_action();
        assert_eq!(action, Some(StepFailureAction::Stop));
    }

    #[test]
    fn supplement_recommendation_maps_to_request_context_supplement() {
        let packed = make_packed(&["outline"], &[false]);
        let report =
            evaluate_context_quality("r10", &packed, &["outline".into(), "lorebook".into()]);
        let action = report.recommendation.to_step_failure_action();
        assert!(
            matches!(
                &action,
                Some(StepFailureAction::RequestContextSupplement { sources })
                    if sources.contains(&"lorebook".to_string())
            ),
            "expected RequestContextSupplement with lorebook, got {:?}",
            action
        );
    }

    #[test]
    fn sufficient_recommendation_maps_to_none() {
        let packed = make_packed(
            &["outline", "lorebook", "chapter", "project_brain"],
            &[false; 4],
        );
        let report = evaluate_context_quality("r11", &packed, &[]);
        assert_eq!(
            report.recommendation,
            ContextQualityRecommendation::Sufficient
        );
        assert_eq!(report.recommendation.to_step_failure_action(), None);
    }
}
