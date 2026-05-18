pub mod agent_loop;
pub mod budget_calibration;
pub mod compaction;
pub mod context_pack;
pub mod context_quality;
pub mod context_window_guard;
pub mod credential_pool;
pub mod domain;
pub mod execution_plan;
pub mod permission;
pub mod prompt_cache;
pub mod provider;
pub mod recovery;
pub mod retry;
pub mod router;
pub mod run_trace;
pub mod task_packet;
pub mod tool_executor;
pub mod tool_registry;
pub mod vector_db;

pub use agent_loop::{AgentLoop, AgentLoopConfig, AgentLoopEvent};
pub use budget_calibration::{
    estimate_tokens, estimate_with_confidence, record_full_usage, record_usage, BudgetCalibration,
    BudgetCalibrationConfidence, CalibratedEstimate, CalibrationStore,
};
pub use compaction::{
    anchor_latest_user_message, compact_messages, compact_messages_with_trigger,
    estimate_message_tokens, find_safe_boundary, should_compact, CompactionConfig,
    CompactionResult, CompactionTrigger, ContextSpineCompactionReport,
};
pub use context_pack::{
    char_count, compute_context_hash, source_priority, truncate_text_report, ContextBudgetReport,
    ContextPacker, ContextSourceReport, PackedContext, TAXONOMY_AUTHOR_VOICE, TAXONOMY_CANON,
    TAXONOMY_INSTRUCTION, TAXONOMY_LORE, TAXONOMY_MEMORY, TAXONOMY_OUTLINE, TAXONOMY_PRIOR_CHAPTER,
    TAXONOMY_PROJECT_BRAIN, TAXONOMY_PROMISE, TAXONOMY_SCENE_PLAN, TAXONOMY_UNKNOWN,
    TAXONOMY_WORLD_APPROVED_RULE, TAXONOMY_WORLD_PROPOSED_RULE, TAXONOMY_WORLD_RAW_EVIDENCE,
};
pub use context_quality::{
    evaluate_context_quality, ContextQualityRecommendation, ContextQualityReport,
};
pub use context_window_guard::{
    evaluate_context_window, guard_request, resolve_context_window_info, ContextWindowGuard,
    ContextWindowInfo, ContextWindowSource,
};
pub use credential_pool::{CredentialPool, CredentialRegistry, PoolStrategy, PooledCredential};
pub use domain::{writing_domain_profile, AgentDomainProfile, ContextPriority, DomainCapability};
pub use execution_plan::{
    compile_plan, ExecutionPlan, ExecutionStep, PlanStatus, StepFailureAction, StepStatus,
};
pub use permission::{PermissionDecision, PermissionMode, PermissionPolicy, PermissionRule};
pub use prompt_cache::{PromptCache, PromptCacheConfig, PromptCacheStats};
pub use recovery::{
    classify_failure, classify_failure_kind, map_failure_to_recovery, parse_failure_remediation,
    redact_sensitive, FailureBundle, FailureKind, FailureRemediation, RecoveryAction,
    RecoveryBundle, RecoveryContext, RecoveryDecision, RetryParams, RuntimeCallRecord,
    RuntimeCallStatus, RuntimeCallType,
};
pub use router::{
    classify_intent, classify_intent_simple, ClassificationSource, Intent, IntentClassification,
};
pub use run_trace::{
    build_agent_run_report, AgentRunEvent, AgentRunEventKind, AgentRunReport, AgentRunStatus,
    AgentRunTrace, BudgetSummary, ContextQualitySummary, FailureRecoverySummary, PlanSummary,
    ProviderTimeline, StepSummary,
};
pub use task_packet::{
    FeedbackContract, FoundationCoverage, RequiredContext, TaskBelief, TaskPacket,
    TaskPacketValidationError, TaskScope, ToolPolicyContract,
};
pub use tool_executor::{
    redact_tool_input, ApprovalContext, DoomLoopDetector, ToolExecution, ToolExecutionAuditEvent,
    ToolExecutionAuditSink, ToolExecutionRemediation, ToolExecutor, ToolHandler,
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
