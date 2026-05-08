//! ContextEngine — deterministic context packs for every agent action.
//! Replaces ad-hoc prompt assembly with budgeted, priority-ordered context sources.

use serde::{Deserialize, Serialize};

use super::context_relevance::{
    format_canon_line, format_promise_line, score_canon_entity, score_promise, WritingRelevance,
};
use super::kernel::derive_next_beat;
use super::memory::{
    push_contract_line, CanonEntitySummary, CreativeDecisionSummary, PlotPromiseSummary,
    StoryContractQuality, WriterMemory,
};
use super::observation::WriterObservation;

include!("context/types.in.rs");
include!("context/assembly.in.rs");
include!("context/seed.in.rs");
include!("context/spine.in.rs");
