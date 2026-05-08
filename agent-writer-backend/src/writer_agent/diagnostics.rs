//! DiagnosticsEngine — ambient canon + promise checking for story continuity.
//! Runs on paragraph completion (3s idle) or chapter save to detect:
//! - Entity/attribute conflicts (weapon, location, relationship)
//! - Unresolved plot promises in current chapter scope
//! - Timeline inconsistencies

use super::memory::{ChapterMissionSummary, StoryContractSummary, WriterMemory};
use super::operation::{AnnotationSeverity, WriterOperation};
use serde::{Deserialize, Serialize};

include!("diagnostics/core.in.rs");
include!("diagnostics/helpers_extract.in.rs");
include!("diagnostics/helpers_violations.in.rs");
include!("diagnostics/helpers_promise.in.rs");
include!("diagnostics/helpers_tests.in.rs");
include!("diagnostics/payoff.in.rs");
