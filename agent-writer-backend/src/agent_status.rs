use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentKernelStatus {
    pub tool_generation: u64,
    pub tool_count: usize,
    pub effective_tool_count: usize,
    pub blocked_tool_count: usize,
    pub model_callable_tool_count: usize,
    pub approval_required_tool_count: usize,
    pub write_tool_count: usize,
    pub domain_id: String,
    pub capability_count: usize,
    pub quality_gate_count: usize,
    pub trace_enabled: bool,
}
