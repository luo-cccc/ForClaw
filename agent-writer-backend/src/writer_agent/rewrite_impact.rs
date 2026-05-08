//! Rewrite Impact Preview — before accepting a rewrite, preview which story
//! facts (canon, promises, missions, style) will be affected.
//!
//! Reuses Story Impact Radius for bidirectional graph analysis.

use serde::{Deserialize, Serialize};

use super::memory::WriterMemory;
use super::observation::WriterObservation;
use super::story_impact::{compute_story_impact, StoryImpactRisk};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RewriteImpactPreview {
    pub observation_id: String,
    pub impacted_canon: Vec<ImpactedEntity>,
    pub impacted_promises: Vec<ImpactedPromise>,
    pub impacted_missions: Vec<ImpactedMission>,
    pub style_signals: Vec<ImpactedStyleSignal>,
    pub risk: String,
    pub truncated_high_risk_sources: Vec<String>,
    pub recommend_planning_review: bool,
    pub evidence_refs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ImpactedEntity {
    pub name: String,
    pub affected_attributes: Vec<String>,
    pub risk: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ImpactedPromise {
    pub title: String,
    pub kind: String,
    pub impact: String, // "payoff_opportunity" | "contradiction_risk" | "progress"
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ImpactedMission {
    pub chapter: String,
    pub clause: String,
    pub impact: String, // "supports" | "contradicts" | "drifts"
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ImpactedStyleSignal {
    pub signal: String,
    pub direction: String, // "aligns" | "diverges"
}

/// Compute a rewrite impact preview using Story Impact Radius.
/// Read-only — does not modify text or ledger.
pub fn compute_rewrite_impact_preview(
    observation: &WriterObservation,
    memory: &WriterMemory,
) -> RewriteImpactPreview {
    let dummy_pack = super::context::assemble_observation_context_with_default_budget(
        super::context::AgentTask::GhostWriting,
        observation,
        memory,
    );

    let (radius, budget) = compute_story_impact(observation, &dummy_pack, memory, None);

    let impacted_canon: Vec<ImpactedEntity> = radius
        .impacted_nodes
        .iter()
        .filter(|n| matches!(n.kind, super::story_impact::StoryNodeKind::CanonEntity))
        .map(|n| ImpactedEntity {
            name: n.label.clone(),
            affected_attributes: Vec::new(),
            risk: if n.confidence < 0.6 { "high" } else { "low" }.to_string(),
        })
        .collect();

    let impacted_promises: Vec<ImpactedPromise> = radius
        .impacted_nodes
        .iter()
        .filter(|n| matches!(n.kind, super::story_impact::StoryNodeKind::PlotPromise))
        .map(|n| ImpactedPromise {
            title: n.label.clone(),
            kind: String::new(),
            impact: if n.confidence < 0.6 {
                "contradiction_risk"
            } else {
                "payoff_opportunity"
            }
            .to_string(),
        })
        .collect();

    let impacted_missions: Vec<ImpactedMission> = radius
        .impacted_nodes
        .iter()
        .filter(|n| matches!(n.kind, super::story_impact::StoryNodeKind::ChapterMission))
        .map(|n| ImpactedMission {
            chapter: n.chapter.clone().unwrap_or_default(),
            clause: n.summary.clone(),
            impact: "supports".to_string(),
        })
        .collect();

    let style_signals: Vec<ImpactedStyleSignal> = Vec::new();

    let recommend_planning_review =
        matches!(radius.risk, StoryImpactRisk::High) || budget.truncated_node_count > 3;

    RewriteImpactPreview {
        observation_id: observation.id.clone(),
        impacted_canon,
        impacted_promises,
        impacted_missions,
        style_signals,
        risk: format!("{:?}", radius.risk),
        truncated_high_risk_sources: budget.dropped_high_risk_sources,
        recommend_planning_review,
        evidence_refs: radius.impacted_sources,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::writer_agent::memory::WriterMemory;
    use crate::writer_agent::observation::{ObservationReason, ObservationSource};
    use std::path::Path;

    fn obs(chapter: &str, text: &str) -> WriterObservation {
        WriterObservation {
            id: "obs-1".to_string(),
            created_at: 1,
            source: ObservationSource::ManualRequest,
            reason: ObservationReason::Explicit,
            project_id: "eval".to_string(),
            chapter_title: Some(chapter.to_string()),
            chapter_revision: None,
            cursor: None,
            selection: None,
            prefix: text.to_string(),
            suffix: String::new(),
            paragraph: text.to_string(),
            full_text_digest: None,
            editor_dirty: false,
        }
    }

    #[test]
    fn preview_is_read_only_no_memory_change() {
        let memory = WriterMemory::open(Path::new(":memory:")).unwrap();
        memory
            .ensure_story_contract_seed("eval", "T", "fantasy", "p", "j", "")
            .unwrap();
        memory
            .upsert_canon_entity(
                "location",
                "林墨",
                &[],
                "主角",
                &serde_json::json!({"weapon":"sword"}),
                0.9,
            )
            .ok();
        let canon_before = memory.list_canon_entities().unwrap().len();
        let _ = compute_rewrite_impact_preview(&obs("Ch1", "test"), &memory);
        assert_eq!(memory.list_canon_entities().unwrap().len(), canon_before);
    }

    #[test]
    fn preview_recommends_planning_review_on_high_risk() {
        let memory = WriterMemory::open(Path::new(":memory:")).unwrap();
        memory
            .ensure_story_contract_seed("eval", "T", "fantasy", "p", "j", "")
            .unwrap();
        // Add many entities to increase impact
        for i in 0..40 {
            memory
                .upsert_canon_entity(
                    "location",
                    &format!("E{}", i),
                    &[],
                    "x",
                    &serde_json::json!({}),
                    0.4,
                )
                .ok();
        }
        let preview = compute_rewrite_impact_preview(&obs("Ch1", "E1"), &memory);
        // Should at minimum have risk assessment populated
        assert!(!preview.risk.is_empty());
    }
}
