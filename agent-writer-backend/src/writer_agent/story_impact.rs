//! Story Impact Radius — graph-shaped context assembly for Writer Agent.
//!
//! Instead of stuffing context by source priority alone, this module computes
//! which story facts (canon, promises, missions, Project Brain chunks) are in
//! the "blast radius" of the current writing task and assembles a budgeted,
//! distance-weighted context report.

use serde::{Deserialize, Serialize};

use super::context::{ContextSource, WritingContextPack};
use super::memory::WriterMemory;
use super::observation::WriterObservation;

include!("story_impact/types.in.rs");
include!("story_impact/graph.in.rs");
include!("story_impact/impact.in.rs");
