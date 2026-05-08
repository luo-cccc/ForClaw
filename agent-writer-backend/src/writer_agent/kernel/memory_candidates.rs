//! Memory candidate extraction and proposal construction helpers.

use std::collections::HashSet;

use super::chapters::proposal_id;
use super::memory_feedback::{
    memory_candidate_slot_for_canon, memory_candidate_slot_for_promise, MemoryCandidate,
    MemoryExtractionFeedback,
};
use crate::writer_agent::memory::{
    CanonEntitySummary, PromiseKind, StylePreferenceSummary, WriterMemory,
};
use crate::writer_agent::observation::WriterObservation;
use crate::writer_agent::operation::{CanonEntityOp, PlotPromiseOp, WriterOperation};
use crate::writer_agent::proposal::{
    AgentProposal, EvidenceRef, EvidenceSource, ProposalKind, ProposalPriority,
};

include!("memory_candidates/extraction.in.rs");
include!("memory_candidates/types.in.rs");
include!("memory_candidates/validation.in.rs");
