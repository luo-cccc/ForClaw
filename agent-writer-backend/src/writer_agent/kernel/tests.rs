use super::*;
use crate::writer_agent::feedback::{FeedbackAction, ProposalFeedback};
use crate::writer_agent::memory::WriterMemory;
use crate::writer_agent::observation::{
    ObservationReason, ObservationSource, TextRange, WriterObservation,
};
use agent_harness_core::TaskScope;

include!("tests/helpers_and_ops.in.rs");
include!("tests/proposals.in.rs");
include!("tests/observations.in.rs");
include!("tests/ledger.in.rs");
include!("tests/memory.in.rs");
