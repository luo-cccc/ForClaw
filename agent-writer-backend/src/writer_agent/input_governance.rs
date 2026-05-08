pub mod compiler;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CompiledInput {
    pub intent_text: String,
    pub selected_evidence: Vec<String>,
    pub rule_stack: Vec<String>,
    pub trace_hint: String,
    pub compiled_at_ms: u64,
}
