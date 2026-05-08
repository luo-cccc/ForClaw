//! Evidence-first conflict explanations for writer beliefs.
//!
//! This module does not decide which source wins. It collects source-labelled
//! evidence and explains why two or more beliefs cannot all be true at once.

use agent_harness_core::Chunk;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

use super::memory::{
    CanonEntitySummary, ChapterMissionSummary, PlotPromiseSummary, StoryContractQuality,
    StoryContractSummary, WriterMemory,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BeliefSource {
    StoryContract,
    ChapterMission,
    Canon,
    PromiseLedger,
    ProjectBrain,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BeliefEvidence {
    pub source: BeliefSource,
    pub reference: String,
    pub snippet: String,
    pub confidence: f64,
    #[serde(default)]
    pub signals: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BeliefConflictKind {
    ForbiddenReveal,
    FactContradiction,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BeliefConflictExplanation {
    pub id: String,
    pub kind: BeliefConflictKind,
    pub summary: String,
    pub rationale: String,
    pub confidence: f64,
    pub evidence: Vec<BeliefEvidence>,
    pub resolution_hint: String,
}

#[derive(Debug, Clone)]
struct GuardBelief<'a> {
    evidence: &'a BeliefEvidence,
    terms: Vec<String>,
    signal: GuardSignal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GuardSignal {
    Forbidden,
    DeferredPayoff,
}

#[derive(Debug, Clone)]
struct FactBelief<'a> {
    subject: String,
    predicate: String,
    object: String,
    evidence: &'a BeliefEvidence,
}

pub fn project_brain_chunk_belief_evidence(chunk: &Chunk, confidence: f64) -> BeliefEvidence {
    let mut signals = Vec::new();
    if let Some(source_ref) = chunk.source_ref.as_deref() {
        signals.push(format!("source_ref={source_ref}"));
    }
    if let Some(source_revision) = chunk.source_revision.as_deref() {
        signals.push(format!("source_revision={source_revision}"));
    }
    BeliefEvidence {
        source: BeliefSource::ProjectBrain,
        reference: format!("project_brain:{}", chunk.id),
        snippet: snippet(&chunk.text, 260),
        confidence: clamp_confidence(confidence),
        signals,
    }
}

pub fn explain_memory_belief_conflicts(
    memory: &WriterMemory,
    project_id: &str,
    chapter_title: Option<&str>,
    project_brain_evidence: &[BeliefEvidence],
) -> rusqlite::Result<Vec<BeliefConflictExplanation>> {
    let mut evidence = Vec::new();

    if let Some(contract) = memory.get_story_contract(project_id)? {
        evidence.extend(story_contract_evidence(&contract));
    }
    if let Some(chapter_title) = chapter_title {
        if let Some(mission) = memory.get_chapter_mission(project_id, chapter_title)? {
            evidence.extend(chapter_mission_evidence(&mission));
        }
    }
    evidence.extend(canon_evidence(&memory.list_canon_entities()?));
    evidence.extend(promise_evidence(&memory.get_open_promise_summaries()?));
    evidence.extend(
        project_brain_evidence
            .iter()
            .filter(|item| item.source == BeliefSource::ProjectBrain)
            .cloned(),
    );

    Ok(explain_belief_conflicts(&evidence))
}

pub fn explain_belief_conflicts(evidence: &[BeliefEvidence]) -> Vec<BeliefConflictExplanation> {
    let mut conflicts = Vec::new();
    conflicts.extend(explain_forbidden_reveals(evidence));
    conflicts.extend(explain_fact_contradictions(evidence));
    dedupe_conflicts(conflicts)
}

include!("belief_conflict/evidence.in.rs");
include!("belief_conflict/explain.in.rs");
include!("belief_conflict/text_utils.in.rs");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explains_forbidden_reveal_without_choosing_winner() {
        let evidence = vec![
            BeliefEvidence {
                source: BeliefSource::StoryContract,
                reference: "story_contract:structural_boundary".to_string(),
                snippet: "不得提前泄露寒玉戒指来源".to_string(),
                confidence: 0.9,
                signals: Vec::new(),
            },
            BeliefEvidence {
                source: BeliefSource::ProjectBrain,
                reference: "project_brain:ring".to_string(),
                snippet: "寒玉戒指来源已经揭示，来自皇宫禁库。".to_string(),
                confidence: 0.85,
                signals: Vec::new(),
            },
        ];

        let conflicts = explain_belief_conflicts(&evidence);
        assert!(conflicts.iter().any(|conflict| {
            conflict.kind == BeliefConflictKind::ForbiddenReveal
                && conflict.evidence.len() == 2
                && conflict
                    .evidence
                    .iter()
                    .any(|item| item.source == BeliefSource::StoryContract)
                && conflict
                    .evidence
                    .iter()
                    .any(|item| item.source == BeliefSource::ProjectBrain)
        }));
    }
}
