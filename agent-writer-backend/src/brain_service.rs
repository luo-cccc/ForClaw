use agent_harness_core::vector_db::{Chunk, VectorDB};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use crate::writer_agent::context_relevance::{
    format_text_chunk_relevance, score_text_for_writing_focus,
};
use crate::writer_agent::kernel::WriterAgentKernel;
use crate::writer_agent::provider_budget::{
    evaluate_provider_budget, WriterProviderBudgetReport, WriterProviderBudgetRequest,
    WriterProviderBudgetTask,
};
use crate::{llm_runtime, storage};

include!("brain_service/types.in.rs");
include!("brain_service/functions_a_embedding.in.rs");
include!("brain_service/functions_b_index.in.rs");
include!("brain_service/functions_b_query.in.rs");
include!("brain_service/functions_b_search.in.rs");
include!("brain_service/functions_b_tests.in.rs");
