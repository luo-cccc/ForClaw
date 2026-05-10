use serde::{Deserialize, Serialize};

use crate::execution_plan::StepEvidence;

// ── RuntimeCallRecord: unified audit for tool/provider/context calls ──

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeCallRecord {
    pub call_id: String,
    pub call_type: RuntimeCallType,
    pub step_id: String,
    pub task_id: Option<String>,
    pub timestamp_ms: u64,
    pub input_redacted_summary: String,
    pub output_summary: String,
    pub duration_ms: u64,
    pub ttft_ms: Option<u64>,
    pub status: RuntimeCallStatus,
    pub remediation_code: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeCallType {
    ToolCall,
    ProviderCall,
    ContextRetrieval,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeCallStatus {
    Success,
    Failed { reason: String },
    Blocked { reason: String },
}

/// Structured remediation codes for tool/provider call failures.
/// A2: Unified failure remediation taxonomy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureRemediation {
    /// Tool not found or permission changed — refresh the tool inventory.
    RefreshInventory,
    /// Approval required but not provided — surface an approval request.
    RequestApproval,
    /// Context too large — shrink or truncate context before retry.
    ShrinkContext,
    /// Transient error (rate limit, timeout) — retry with backoff.
    RetryTransient,
    /// Unsafe write detected — abort and require explicit approval.
    AbortUnsafeWrite,
}

impl FailureRemediation {
    pub fn code(&self) -> &'static str {
        match self {
            FailureRemediation::RefreshInventory => "refresh_inventory",
            FailureRemediation::RequestApproval => "request_approval",
            FailureRemediation::ShrinkContext => "shrink_context",
            FailureRemediation::RetryTransient => "retry_transient",
            FailureRemediation::AbortUnsafeWrite => "abort_unsafe_write",
        }
    }

    pub fn message(&self, tool_name: &str) -> String {
        match self {
            FailureRemediation::RefreshInventory => format!(
                "Check the external agent/tool name for '{}', refresh the registry, and retry only if it appears in the allowed inventory.",
                tool_name
            ),
            FailureRemediation::RequestApproval => format!(
                "Surface an explicit approval request before retrying '{}', or choose a read-only/preview tool.",
                tool_name
            ),
            FailureRemediation::ShrinkContext => format!(
                "Reduce context size before retrying '{}'. Remove non-essential sources or truncate long artifacts.",
                tool_name
            ),
            FailureRemediation::RetryTransient => format!(
                "Transient failure for '{}'. Back off and retry with the same or narrower arguments.",
                tool_name
            ),
            FailureRemediation::AbortUnsafeWrite => format!(
                "'{}' attempted an unsafe write. Abort and surface an explicit approval request before retrying.",
                tool_name
            ),
        }
    }
}

/// Convert a remediation code string to a structured FailureRemediation if recognized.
pub fn parse_failure_remediation(code: &str) -> Option<FailureRemediation> {
    match code {
        "refresh_inventory" => Some(FailureRemediation::RefreshInventory),
        "request_approval" => Some(FailureRemediation::RequestApproval),
        "shrink_context" => Some(FailureRemediation::ShrinkContext),
        "retry_transient" => Some(FailureRemediation::RetryTransient),
        "abort_unsafe_write" => Some(FailureRemediation::AbortUnsafeWrite),
        _ => None,
    }
}

/// Redact sensitive values (API keys, secrets, tokens) from a JSON value before
/// including it in an audit payload. Returns a shallow-redacted clone.
pub fn redact_sensitive(value: &serde_json::Value) -> serde_json::Value {
    use serde_json::Map;

    fn redact_str(s: &str) -> String {
        let lower = s.to_ascii_lowercase();
        if lower.contains("api_key")
            || lower.contains("apikey")
            || lower.contains("secret")
            || lower.contains("token")
            || lower.contains("password")
            || lower.contains("auth")
            || lower.contains("credential")
            || lower.contains("key")
                && (lower.contains("sk-") || lower.contains("ak-") || lower.starts_with("ey"))
            || s.starts_with("sk-")
            || s.starts_with("eyJ")
        {
            "[REDACTED]".to_string()
        } else {
            s.to_string()
        }
    }

    match value {
        serde_json::Value::Object(map) => {
            let mut out = Map::new();
            for (k, v) in map {
                let key_lower = k.to_ascii_lowercase();
                let is_sensitive_key = key_lower.contains("api_key")
                    || key_lower.contains("apikey")
                    || key_lower.contains("secret")
                    || key_lower.contains("token")
                    || key_lower.contains("password")
                    || key_lower.contains("auth")
                    || key_lower.contains("credential")
                    || key_lower.contains("key")
                    || key_lower.contains("authorization");

                if is_sensitive_key {
                    out.insert(
                        k.clone(),
                        serde_json::Value::String("[REDACTED]".to_string()),
                    );
                } else if let serde_json::Value::String(s) = v {
                    out.insert(k.clone(), serde_json::Value::String(redact_str(s)));
                } else {
                    out.insert(k.clone(), redact_sensitive(v));
                }
            }
            serde_json::Value::Object(out)
        }
        serde_json::Value::Array(arr) => {
            serde_json::Value::Array(arr.iter().map(redact_sensitive).collect())
        }
        serde_json::Value::String(s) => serde_json::Value::String(redact_str(s)),
        other => other.clone(),
    }
}

// ── FailureBundle: existing recovery types ──

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FailureBundle {
    pub run_id: String,
    pub failed_step: String,
    pub error_kind: String,
    pub completed_steps: Vec<String>,
    pub stuck_at: String,
    pub retry_parameters: Option<RetryParams>,
    pub suggested_action: RecoveryAction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RetryParams {
    pub delay_ms: u64,
    pub max_context_chars: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryAction {
    Retry { delay_ms: u64 },
    ShrinkContext { max_chars: usize },
    ApprovalRequired { reason: String },
    Stop,
}

// ── Failure Taxonomy (A5) ──

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureKind {
    ProviderTransient,
    ProviderBudget,
    ProviderAuth,
    ContextMissing,
    ContextOverflow,
    ToolPermission,
    ToolSchema,
    SaveConflict,
    QualityGate,
    DoomLoop,
    UnsafeWrite,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryDecision {
    RetryWithBackoff,
    RequestApproval,
    ShrinkContext,
    CompactContext,
    RefreshInventory,
    RecheckRevision,
    TargetedRevision,
    StrictBlock,
    AbortWithBundle,
    SurfaceUserChoice,
}

/// Context passed to the failure recovery mapper.
#[derive(Debug, Clone, Default)]
pub struct RecoveryContext {
    pub retry_count: u32,
    pub has_user_intervention_channel: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryBundle {
    pub completed_steps: Vec<StepEvidence>,
    pub failed_step: String,
    pub failure_kind: FailureKind,
    pub input_context_summary: String,
    pub runtime_calls: Vec<RuntimeCallRecord>,
    pub suggested_action: RecoveryDecision,
    pub user_choice_required: bool,
}

/// Maps a `FailureKind` to the appropriate `RecoveryDecision`.
pub fn map_failure_to_recovery(kind: &FailureKind, _context: &RecoveryContext) -> RecoveryDecision {
    match kind {
        FailureKind::ProviderTransient => RecoveryDecision::RetryWithBackoff,
        FailureKind::ProviderBudget => RecoveryDecision::ShrinkContext,
        FailureKind::ProviderAuth => RecoveryDecision::AbortWithBundle,
        FailureKind::ContextMissing => RecoveryDecision::CompactContext,
        FailureKind::ContextOverflow => RecoveryDecision::ShrinkContext,
        FailureKind::ToolPermission => RecoveryDecision::RequestApproval,
        FailureKind::ToolSchema => RecoveryDecision::RefreshInventory,
        FailureKind::SaveConflict => RecoveryDecision::SurfaceUserChoice,
        FailureKind::QualityGate => RecoveryDecision::TargetedRevision,
        FailureKind::DoomLoop => RecoveryDecision::AbortWithBundle,
        FailureKind::UnsafeWrite => RecoveryDecision::AbortWithBundle,
        FailureKind::Unknown => RecoveryDecision::SurfaceUserChoice,
    }
}

/// Derive a `FailureKind` from an error string and optional remediation code.
pub fn classify_failure_kind(error: &str, remediation_code: Option<&str>) -> FailureKind {
    let lower = error.to_ascii_lowercase();
    let code = remediation_code.unwrap_or("");

    if lower.contains("doom loop") || code == "tool_doom_loop" {
        return FailureKind::DoomLoop;
    }
    if lower.contains("unsafe")
        || lower.contains("destructive")
        || lower.contains("overwrite")
        || code == "abort_unsafe_write"
    {
        return FailureKind::UnsafeWrite;
    }
    if lower.contains("rate limit")
        || lower.contains("429")
        || lower.contains("timeout")
        || lower.contains("transient")
        || code == "retry_transient"
    {
        return FailureKind::ProviderTransient;
    }
    if lower.contains("401")
        || lower.contains("403")
        || lower.contains("unauthorized")
        || lower.contains("auth")
    {
        return FailureKind::ProviderAuth;
    }
    if lower.contains("budget") || lower.contains("ceiling") {
        return FailureKind::ProviderBudget;
    }
    if lower.contains("context") && (lower.contains("overflow") || lower.contains("too long")) {
        return FailureKind::ContextOverflow;
    }
    if lower.contains("context") && (lower.contains("missing") || lower.contains("unavailable")) {
        return FailureKind::ContextMissing;
    }
    if code == "tool_not_in_allowed_list"
        || code == "tool_denied"
        || code == "request_approval"
        || code == "external_access_denied"
    {
        return FailureKind::ToolPermission;
    }
    if code == "tool_not_registered"
        || code == "refresh_inventory"
        || code == "missing_binary_or_resource"
    {
        return FailureKind::ToolSchema;
    }
    if lower.contains("conflict") || lower.contains("save conflict") || code == "save_conflict" {
        return FailureKind::SaveConflict;
    }
    if lower.contains("quality gate") || lower.contains("quality_gate") {
        return FailureKind::QualityGate;
    }
    FailureKind::Unknown
}

pub fn classify_failure(
    run_id: &str,
    error: &str,
    failed_step: &str,
    completed_steps: &[String],
) -> FailureBundle {
    let lower = error.to_ascii_lowercase();

    let (suggested_action, retry_parameters) =
        if lower.contains("rate limit") || lower.contains("429") {
            (
                RecoveryAction::Retry { delay_ms: 30000 },
                Some(RetryParams {
                    delay_ms: 30000,
                    max_context_chars: None,
                }),
            )
        } else if lower.contains("context")
            && (lower.contains("overflow") || lower.contains("too long"))
        {
            (
                RecoveryAction::ShrinkContext { max_chars: 16000 },
                Some(RetryParams {
                    delay_ms: 1000,
                    max_context_chars: Some(16000),
                }),
            )
        } else if lower.contains("timeout") || lower.contains("timed out") {
            (
                RecoveryAction::Retry { delay_ms: 15000 },
                Some(RetryParams {
                    delay_ms: 15000,
                    max_context_chars: None,
                }),
            )
        } else if lower.contains("approval") || lower.contains("budget") {
            (
                RecoveryAction::ApprovalRequired {
                    reason: error.to_string(),
                },
                None,
            )
        } else {
            (RecoveryAction::Stop, None)
        };

    let error_kind = if lower.contains("rate limit") || lower.contains("429") {
        "provider"
    } else if lower.contains("context") {
        "context_overflow"
    } else if lower.contains("approval") || lower.contains("budget") {
        "budget"
    } else {
        "backend"
    };

    FailureBundle {
        run_id: run_id.to_string(),
        failed_step: failed_step.to_string(),
        error_kind: error_kind.to_string(),
        completed_steps: completed_steps.to_vec(),
        stuck_at: failed_step.to_string(),
        retry_parameters,
        suggested_action,
    }
}

#[cfg(test)]
mod recovery_tests {
    use super::*;

    #[test]
    fn rate_limit_suggests_retry() {
        let bundle = classify_failure(
            "r1",
            "LLM call failed (429): rate limited",
            "step-1",
            &["step-0".into()],
        );
        assert_eq!(
            bundle.suggested_action,
            RecoveryAction::Retry { delay_ms: 30000 }
        );
        assert_eq!(bundle.error_kind, "provider");
    }

    #[test]
    fn context_overflow_suggests_shrink() {
        let bundle = classify_failure(
            "r2",
            "context_length_exceeded: context overflow",
            "step-2",
            &[],
        );
        assert_eq!(
            bundle.suggested_action,
            RecoveryAction::ShrinkContext { max_chars: 16000 }
        );
        assert_eq!(bundle.error_kind, "context_overflow");
    }

    #[test]
    fn timeout_suggests_retry() {
        let bundle = classify_failure("r3", "request timed out", "step-1", &[]);
        assert_eq!(
            bundle.suggested_action,
            RecoveryAction::Retry { delay_ms: 15000 }
        );
    }

    #[test]
    fn approval_error_suggests_approval() {
        let bundle = classify_failure("r4", "provider budget approval required", "step-3", &[]);
        assert!(matches!(
            bundle.suggested_action,
            RecoveryAction::ApprovalRequired { .. }
        ));
        assert_eq!(bundle.error_kind, "budget");
    }

    #[test]
    fn unknown_error_suggests_stop() {
        let bundle = classify_failure("r5", "something unexpected happened", "step-0", &[]);
        assert_eq!(bundle.suggested_action, RecoveryAction::Stop);
        assert_eq!(bundle.error_kind, "backend");
    }

    #[test]
    fn runtime_call_record_serializes() {
        let record = RuntimeCallRecord {
            call_id: "c1".into(),
            call_type: RuntimeCallType::ProviderCall,
            step_id: "step-0".into(),
            task_id: Some("task-1".into()),
            timestamp_ms: 1_000,
            input_redacted_summary: "redacted".into(),
            output_summary: "ok".into(),
            duration_ms: 120,
            ttft_ms: Some(80),
            status: RuntimeCallStatus::Success,
            remediation_code: None,
        };
        let json = serde_json::to_string(&record).unwrap();
        assert!(json.contains("provider_call"), "json: {}", json);
        assert!(json.contains("success"), "json: {}", json);
        assert!(json.contains("taskId"), "json: {}", json);
    }

    #[test]
    fn redact_sensitive_strips_api_keys() {
        let input = serde_json::json!({
            "api_key": "sk-live-1234567890abcdef",
            "secret": "my-secret-value",
            "normal": "hello world",
            "nested": {
                "token": "bearer-abc",
                "data": "visible"
            }
        });
        let redacted = redact_sensitive(&input);
        assert_eq!(redacted["api_key"], "[REDACTED]");
        assert_eq!(redacted["secret"], "[REDACTED]");
        assert_eq!(redacted["normal"], "hello world");
        assert_eq!(redacted["nested"]["token"], "[REDACTED]");
        assert_eq!(redacted["nested"]["data"], "visible");
    }

    #[test]
    fn redact_sensitive_strips_sk_prefix() {
        let input = serde_json::json!({"key": "sk-test-abc123"});
        let redacted = redact_sensitive(&input);
        assert_eq!(redacted["key"], "[REDACTED]");
    }

    #[test]
    fn redact_sensitive_preserves_safe_strings() {
        let input = serde_json::json!({"message": "The quick brown fox"});
        let redacted = redact_sensitive(&input);
        assert_eq!(redacted["message"], "The quick brown fox");
    }

    #[test]
    fn serialization_roundtrip() {
        let bundle = FailureBundle {
            run_id: "r1".into(),
            failed_step: "step-1".into(),
            error_kind: "provider".into(),
            completed_steps: vec!["step-0".into()],
            stuck_at: "step-1".into(),
            retry_parameters: Some(RetryParams {
                delay_ms: 30000,
                max_context_chars: None,
            }),
            suggested_action: RecoveryAction::Retry { delay_ms: 30000 },
        };
        let json = serde_json::to_string(&bundle).unwrap();
        let decoded: FailureBundle = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.run_id, "r1");
        assert_eq!(
            decoded.suggested_action,
            RecoveryAction::Retry { delay_ms: 30000 }
        );
    }

    // ── A5: Failure Taxonomy tests ──

    #[test]
    fn provider_429_maps_to_retry_with_backoff() {
        let kind = classify_failure_kind("LLM call failed (429): rate limited", None);
        assert_eq!(kind, FailureKind::ProviderTransient);
        let decision = map_failure_to_recovery(&kind, &RecoveryContext::default());
        assert_eq!(decision, RecoveryDecision::RetryWithBackoff);
    }

    #[test]
    fn provider_5xx_maps_to_retry_with_backoff() {
        let kind = classify_failure_kind("Internal Server Error (503): transient failure", None);
        assert_eq!(kind, FailureKind::ProviderTransient);
        let decision = map_failure_to_recovery(&kind, &RecoveryContext::default());
        assert_eq!(decision, RecoveryDecision::RetryWithBackoff);
    }

    #[test]
    fn provider_401_maps_to_abort_with_bundle() {
        let kind = classify_failure_kind("Unauthorized (401): invalid API key", None);
        assert_eq!(kind, FailureKind::ProviderAuth);
        let decision = map_failure_to_recovery(&kind, &RecoveryContext::default());
        assert_eq!(decision, RecoveryDecision::AbortWithBundle);
    }

    #[test]
    fn provider_403_maps_to_abort_with_bundle() {
        let kind = classify_failure_kind("Forbidden (403): insufficient permissions", None);
        assert_eq!(kind, FailureKind::ProviderAuth);
        let decision = map_failure_to_recovery(&kind, &RecoveryContext::default());
        assert_eq!(decision, RecoveryDecision::AbortWithBundle);
    }

    #[test]
    fn context_missing_maps_to_compact_context() {
        let kind = classify_failure_kind("Required context missing: outline not loaded", None);
        assert_eq!(kind, FailureKind::ContextMissing);
        let decision = map_failure_to_recovery(&kind, &RecoveryContext::default());
        assert_eq!(decision, RecoveryDecision::CompactContext);
    }

    #[test]
    fn context_overflow_maps_to_shrink_context() {
        let kind = classify_failure_kind("context_length_exceeded: context overflow", None);
        assert_eq!(kind, FailureKind::ContextOverflow);
        let decision = map_failure_to_recovery(&kind, &RecoveryContext::default());
        assert_eq!(decision, RecoveryDecision::ShrinkContext);
    }

    #[test]
    fn save_conflict_maps_to_surface_user_choice() {
        let kind = classify_failure_kind("Save conflict: revision mismatch", None);
        assert_eq!(kind, FailureKind::SaveConflict);
        let decision = map_failure_to_recovery(&kind, &RecoveryContext::default());
        assert_eq!(decision, RecoveryDecision::SurfaceUserChoice);
    }

    #[test]
    fn tool_permission_maps_to_request_approval() {
        let kind = classify_failure_kind(
            "Tool 'write_file' requires explicit approval",
            Some("request_approval"),
        );
        assert_eq!(kind, FailureKind::ToolPermission);
        let decision = map_failure_to_recovery(&kind, &RecoveryContext::default());
        assert_eq!(decision, RecoveryDecision::RequestApproval);
    }

    #[test]
    fn tool_schema_maps_to_refresh_inventory() {
        let kind = classify_failure_kind(
            "Tool 'search' is not registered",
            Some("tool_not_registered"),
        );
        assert_eq!(kind, FailureKind::ToolSchema);
        let decision = map_failure_to_recovery(&kind, &RecoveryContext::default());
        assert_eq!(decision, RecoveryDecision::RefreshInventory);
    }

    #[test]
    fn doom_loop_maps_to_abort_with_bundle() {
        let kind = classify_failure_kind(
            "DOOM LOOP DETECTED: tool 'search' called with same args 3+ times",
            Some("tool_doom_loop"),
        );
        assert_eq!(kind, FailureKind::DoomLoop);
        let decision = map_failure_to_recovery(&kind, &RecoveryContext::default());
        assert_eq!(decision, RecoveryDecision::AbortWithBundle);
    }

    #[test]
    fn unsafe_write_maps_to_abort_with_bundle() {
        let kind = classify_failure_kind(
            "Tool 'write_file' attempted an unsafe write",
            Some("abort_unsafe_write"),
        );
        assert_eq!(kind, FailureKind::UnsafeWrite);
        let decision = map_failure_to_recovery(&kind, &RecoveryContext::default());
        assert_eq!(decision, RecoveryDecision::AbortWithBundle);
    }

    #[test]
    fn quality_gate_maps_to_targeted_revision() {
        let kind = classify_failure_kind("Quality gate failed: prose below threshold", None);
        assert_eq!(kind, FailureKind::QualityGate);
        let decision = map_failure_to_recovery(&kind, &RecoveryContext::default());
        assert_eq!(decision, RecoveryDecision::TargetedRevision);
    }

    #[test]
    fn unknown_error_maps_to_surface_user_choice() {
        let kind = classify_failure_kind("Something completely unexpected happened", None);
        assert_eq!(kind, FailureKind::Unknown);
        let decision = map_failure_to_recovery(&kind, &RecoveryContext::default());
        assert_eq!(decision, RecoveryDecision::SurfaceUserChoice);
    }

    #[test]
    fn recovery_bundle_serializes() {
        let bundle = RecoveryBundle {
            completed_steps: vec![],
            failed_step: "step-1".into(),
            failure_kind: FailureKind::ProviderTransient,
            input_context_summary: "rate limited".into(),
            runtime_calls: vec![],
            suggested_action: RecoveryDecision::RetryWithBackoff,
            user_choice_required: false,
        };
        let json = serde_json::to_string(&bundle).unwrap();
        assert!(json.contains("provider_transient"), "json: {}", json);
        assert!(json.contains("retry_with_backoff"), "json: {}", json);
    }

    #[test]
    fn classify_failure_kind_from_remediation_code() {
        assert_eq!(
            classify_failure_kind("any error", Some("tool_not_in_allowed_list")),
            FailureKind::ToolPermission
        );
        assert_eq!(
            classify_failure_kind("any error", Some("refresh_inventory")),
            FailureKind::ToolSchema
        );
        assert_eq!(
            classify_failure_kind("any error", Some("retry_transient")),
            FailureKind::ProviderTransient
        );
    }

    // ── A2: FailureRemediation tests ──

    #[test]
    fn failure_remediation_codes_match_spec() {
        assert_eq!(FailureRemediation::RefreshInventory.code(), "refresh_inventory");
        assert_eq!(FailureRemediation::RequestApproval.code(), "request_approval");
        assert_eq!(FailureRemediation::ShrinkContext.code(), "shrink_context");
        assert_eq!(FailureRemediation::RetryTransient.code(), "retry_transient");
        assert_eq!(FailureRemediation::AbortUnsafeWrite.code(), "abort_unsafe_write");
    }

    #[test]
    fn failure_remediation_messages_include_tool_name() {
        let msg = FailureRemediation::RequestApproval.message("write_file");
        assert!(msg.contains("write_file"));
        assert!(msg.contains("approval"));
    }

    #[test]
    fn parse_failure_remediation_roundtrip() {
        assert_eq!(
            parse_failure_remediation("refresh_inventory"),
            Some(FailureRemediation::RefreshInventory)
        );
        assert_eq!(
            parse_failure_remediation("request_approval"),
            Some(FailureRemediation::RequestApproval)
        );
        assert_eq!(
            parse_failure_remediation("shrink_context"),
            Some(FailureRemediation::ShrinkContext)
        );
        assert_eq!(
            parse_failure_remediation("retry_transient"),
            Some(FailureRemediation::RetryTransient)
        );
        assert_eq!(
            parse_failure_remediation("abort_unsafe_write"),
            Some(FailureRemediation::AbortUnsafeWrite)
        );
        assert_eq!(parse_failure_remediation("unknown_code"), None);
    }

    #[test]
    fn failure_remediation_classifies_to_failure_kind() {
        assert_eq!(
            classify_failure_kind("any", Some("refresh_inventory")),
            FailureKind::ToolSchema
        );
        assert_eq!(
            classify_failure_kind("any", Some("request_approval")),
            FailureKind::ToolPermission
        );
        assert_eq!(
            classify_failure_kind("any", Some("retry_transient")),
            FailureKind::ProviderTransient
        );
        assert_eq!(
            classify_failure_kind("any", Some("abort_unsafe_write")),
            FailureKind::UnsafeWrite
        );
    }
}
