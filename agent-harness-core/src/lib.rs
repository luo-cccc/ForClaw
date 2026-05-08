pub mod agent_loop;
pub mod ambient;
pub mod compaction;
pub mod config;
pub mod context_pack;
pub mod context_window_guard;
pub mod credential_pool;
pub mod domain;
pub mod hermes_memory;
pub mod hooks;
pub mod llm;
pub mod permission;
pub mod planner;
pub mod prompt_cache;
pub mod provider;
pub mod ptc;
pub mod retry;
pub mod router;
pub mod run_trace;
pub mod skill_lifecycle;
pub mod task_packet;
pub mod tool_executor;
pub mod tool_registry;
pub mod vector_db;

pub use agent_loop::{AgentLoop, AgentLoopConfig, AgentLoopEvent};
pub use ambient::{AgentOutput, AmbientAgent, AmbientEventBus, EditorEvent};
pub use compaction::{
    anchor_latest_user_message, compact_messages, compact_messages_with_trigger,
    estimate_message_tokens, find_safe_boundary, should_compact, CompactionConfig,
    CompactionResult, CompactionTrigger, ContextSpineCompactionReport,
};
pub use config::HarnessConfig;
pub use context_pack::{
    char_count, truncate_text_report, ContextBudgetReport, ContextPacker, ContextSourceReport,
    PackedContext,
};
pub use context_window_guard::{
    evaluate_context_window, guard_request, resolve_context_window_info, ContextWindowGuard,
    ContextWindowInfo, ContextWindowSource,
};
pub use credential_pool::{CredentialPool, CredentialRegistry, PoolStrategy, PooledCredential};
pub use domain::{writing_domain_profile, AgentDomainProfile, ContextPriority, DomainCapability};
pub use hermes_memory::{HermesDB, SessionSearchResult};
pub use hooks::{HookDecision, HookEvent, HookPayload, HookRunner};
pub use llm::LLMClient;
pub use permission::{PermissionDecision, PermissionMode, PermissionPolicy, PermissionRule};
pub use planner::{ExecutionPlan, PlanState, PlanStep};
pub use prompt_cache::{PromptCache, PromptCacheConfig, PromptCacheStats};
pub use ptc::{build_ptc_prompt, parse_ptc_output, PtcConfig, PtcResult, PtcScript};
pub use router::{classify_intent, Intent};
pub use run_trace::{AgentRunEvent, AgentRunEventKind, AgentRunStatus, AgentRunTrace};
pub use skill_lifecycle::{CurationReport, CuratorConfig, Skill, SkillCategory, SkillCurator};
pub use task_packet::{
    FeedbackContract, FoundationCoverage, RequiredContext, TaskBelief, TaskPacket,
    TaskPacketValidationError, TaskScope, ToolPolicyContract,
};
pub use tool_executor::{
    DoomLoopDetector, ToolExecution, ToolExecutionAuditEvent, ToolExecutionAuditSink,
    ToolExecutionRemediation, ToolExecutor, ToolHandler,
};
pub use tool_registry::{
    default_writing_tool_registry, EffectiveToolEntry, EffectiveToolInventory, EffectiveToolStatus,
    ToolDescriptor, ToolFilter, ToolRegistry, ToolRegistryError, ToolSideEffectLevel, ToolStage,
};
pub use vector_db::{chunk_text, cosine_similarity, extract_keywords, Chunk, VectorDB};

/// 通用文本截断 — 取最后 max_chars 字符，从词边界断开
pub fn truncate_context(text: &str, max_chars: usize) -> &str {
    if text.len() <= max_chars {
        return text;
    }
    let start = text.len().saturating_sub(max_chars);
    let slice = &text[start..];
    if let Some(idx) = slice.find(' ') {
        &slice[idx + 1..]
    } else {
        slice
    }
}
