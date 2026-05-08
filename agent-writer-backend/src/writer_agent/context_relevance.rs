//! Writing relevance ranking for context ledger slices.

use std::collections::HashSet;

use agent_harness_core::extract_keywords;

use super::memory::{CanonEntitySummary, CreativeDecisionSummary, PlotPromiseSummary};
use super::observation::WriterObservation;

pub(crate) struct WritingRelevance {
    cursor_text: String,
    foundation_text: String,
    cursor_terms: Vec<String>,
    foundation_terms: Vec<String>,
    cursor_scene_types: Vec<WritingSceneType>,
    foundation_scene_types: Vec<WritingSceneType>,
}

#[derive(Default)]
pub(crate) struct RelevanceScore {
    pub score: i32,
    pub reasons: Vec<String>,
}

impl RelevanceScore {
    fn add(&mut self, score: i32, reason: impl Into<String>) {
        self.score += score;
        let reason = reason.into();
        if !reason.trim().is_empty() && !self.reasons.contains(&reason) {
            self.reasons.push(reason);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WritingSceneType {
    Dialogue,
    Action,
    Description,
    EmotionalBeat,
    ConflictEscalation,
    Reveal,
    SetupPayoff,
    Exposition,
    Transition,
}

impl WritingSceneType {
    fn label(self) -> &'static str {
        match self {
            WritingSceneType::Dialogue => "dialogue",
            WritingSceneType::Action => "action",
            WritingSceneType::Description => "description",
            WritingSceneType::EmotionalBeat => "emotional_beat",
            WritingSceneType::ConflictEscalation => "conflict_escalation",
            WritingSceneType::Reveal => "reveal",
            WritingSceneType::SetupPayoff => "setup_payoff",
            WritingSceneType::Exposition => "exposition",
            WritingSceneType::Transition => "transition",
        }
    }
}

impl WritingRelevance {
    pub(crate) fn new(
        observation: &WriterObservation,
        chapter_mission: &str,
        next_beat: &str,
        result_feedback: &str,
        decision_slice: &str,
    ) -> Self {
        let cursor_text = [
            observation.prefix.as_str(),
            observation.paragraph.as_str(),
            observation.suffix.as_str(),
            observation.selected_text(),
        ]
        .join("\n");
        let foundation_text = [chapter_mission, next_beat, result_feedback, decision_slice]
            .into_iter()
            .filter(|text| !text.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        Self {
            cursor_terms: relevance_terms(&cursor_text),
            foundation_terms: relevance_terms(&foundation_text),
            cursor_scene_types: infer_scene_types(&cursor_text),
            foundation_scene_types: infer_scene_types(&foundation_text),
            cursor_text,
            foundation_text,
        }
    }

    fn cursor_contains(&self, needle: &str) -> bool {
        !needle.trim().is_empty() && self.cursor_text.contains(needle)
    }

    fn foundation_contains(&self, needle: &str) -> bool {
        !needle.trim().is_empty() && self.foundation_text.contains(needle)
    }
}

pub fn rerank_text_chunks<T, F>(
    chunks: Vec<(f32, T)>,
    writing_focus: &str,
    text_for_chunk: F,
) -> Vec<(f32, Vec<String>, T)>
where
    F: Fn(&T) -> String,
{
    let focus = WritingRelevanceFocus::new(writing_focus);
    let mut scored = chunks
        .into_iter()
        .map(|(base_score, chunk)| {
            let text = text_for_chunk(&chunk);
            let relevance = score_text_chunk(&focus, &text);
            let combined = base_score + relevance.score as f32;
            (combined, relevance.reasons, chunk)
        })
        .collect::<Vec<_>>();
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    scored
}

pub fn score_text_for_writing_focus(writing_focus: &str, text: &str) -> (f32, Vec<String>) {
    let focus = WritingRelevanceFocus::new(writing_focus);
    let relevance = score_text_chunk(&focus, text);
    (relevance.score as f32, relevance.reasons)
}

pub fn format_text_chunk_relevance(reasons: &[String]) -> String {
    let reason = if reasons.is_empty() {
        "semantic similarity".to_string()
    } else {
        relevance_reason_text(reasons)
    };
    format!("WHY writing_relevance: {}", reason)
}

pub fn writing_scene_types(text: &str) -> Vec<String> {
    infer_scene_types(text)
        .into_iter()
        .map(|scene_type| scene_type.label().to_string())
        .collect()
}

include!("context_relevance/typed_filter.in.rs");
include!("context_relevance/scoring.in.rs");
include!("context_relevance/focus.in.rs");
include!("context_relevance/scene_helpers.in.rs");
include!("context_relevance/terms.in.rs");
