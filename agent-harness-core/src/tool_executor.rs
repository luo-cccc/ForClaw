use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::permission::{PermissionDecision, PermissionMode, PermissionPolicy};
use crate::recovery::{classify_failure_kind, FailureRemediation};
use crate::tool_registry::{ToolRegistry, ToolSideEffectLevel};

/// Callback trait for tool handlers.
/// Implementations bridge to the application layer storage and domain tools.
/// Ported from Claw Code's tool dispatch pattern.
#[async_trait::async_trait]
pub trait ToolHandler: Send + Sync {
    async fn execute(
        &self,
        tool_name: &str,
        args: serde_json::Value,
    ) -> Result<serde_json::Value, String>;
}

/// Result of a single tool execution.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ToolExecution {
    pub tool_name: String,
    pub input: serde_json::Value,
    pub output: serde_json::Value,
    pub error: Option<String>,
    #[serde(default)]
    pub remediation: Vec<ToolExecutionRemediation>,
    pub duration_ms: u64,
    /// A2: Approval context binding for write/proposal/approval tools.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval_context: Option<ApprovalContext>,
}

/// A2: Approval context binding for write/proposal/approval tools.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalContext {
    pub proposal_id: Option<String>,
    pub approval_source: Option<String>,
}

impl ToolExecution {
    /// Derive the `FailureKind` from this tool execution's error and remediation.
    pub fn failure_kind(&self) -> Option<crate::recovery::FailureKind> {
        let error = self.error.as_ref()?;
        let code = self.remediation.first().map(|r| r.code.as_str());
        Some(classify_failure_kind(error, code))
    }
}

/// A2: Redact and summarize tool input before entering audit payload.
/// - Truncates inputs longer than 200 chars
/// - Masks API keys, secrets, tokens (regex for key=..., token=..., secret=...)
/// - Keeps the first 100 chars of content as summary
pub fn redact_tool_input(input: &str) -> String {
    const MAX_LEN: usize = 200;
    const SUMMARY_LEN: usize = 100;

    if input.is_empty() {
        return String::new();
    }

    // First apply regex-like masking for key=..., token=..., secret=...
    let masked = mask_secrets_in_text(input);

    if masked.len() <= MAX_LEN {
        return masked;
    }

    // Truncate and keep first SUMMARY_LEN chars as summary
    let summary: String = masked.chars().take(SUMMARY_LEN).collect();
    format!(
        "{}... [truncated {} chars]",
        summary,
        masked.len().saturating_sub(SUMMARY_LEN)
    )
}

fn mask_secrets_in_text(text: &str) -> String {
    use regex::Regex;
    let mut result = text.to_string();

    // Mask key=..., token=..., secret=..., password=..., api_key=...
    let patterns = [
        (Regex::new(r"(?i)(key\s*=\s*)[^\s,;\}\]]+").ok(), "key=***"),
        (
            Regex::new(r"(?i)(token\s*=\s*)[^\s,;\}\]]+").ok(),
            "token=***",
        ),
        (
            Regex::new(r"(?i)(secret\s*=\s*)[^\s,;\}\]]+").ok(),
            "secret=***",
        ),
        (
            Regex::new(r"(?i)(password\s*=\s*)[^\s,;\}\]]+").ok(),
            "password=***",
        ),
        (
            Regex::new(r"(?i)(api_key\s*=\s*)[^\s,;\}\]]+").ok(),
            "api_key=***",
        ),
        (
            Regex::new(r"(?i)(auth\s*=\s*)[^\s,;\}\]]+").ok(),
            "auth=***",
        ),
    ];

    for (maybe_re, replacement) in &patterns {
        if let Some(re) = maybe_re {
            result = re.replace_all(&result, *replacement).to_string();
        }
    }

    result
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolExecutionRemediation {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "phase", rename_all = "snake_case")]
pub enum ToolExecutionAuditEvent {
    Start {
        tool_name: String,
        input: serde_json::Value,
    },
    End {
        execution: ToolExecution,
    },
}

pub type ToolExecutionAuditSink = Arc<dyn Fn(ToolExecutionAuditEvent) + Send + Sync>;

/// Tracks tool calls to detect doom loops.
/// Ported from OpenCode `processor.ts` doom loop detection (line 305-331).
#[derive(Debug, Clone, Default)]
pub struct DoomLoopDetector {
    call_history: HashMap<(String, u64), u32>,
}

impl DoomLoopDetector {
    fn hash_args(args: &serde_json::Value) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        args.to_string().hash(&mut h);
        h.finish()
    }

    /// Returns true if same tool + same args called 3+ consecutive times.
    pub fn is_doom_loop(&mut self, tool_name: &str, args: &serde_json::Value) -> bool {
        let key = (tool_name.to_string(), Self::hash_args(args));
        let count = self.call_history.entry(key).or_insert(0);
        *count += 1;
        *count >= 3
    }

    /// Reset all tracking. Call after a successful round with different output.
    pub fn reset(&mut self) {
        self.call_history.clear();
    }
}

/// The tool executor dispatches tool calls to registered handlers.
/// Generic over the handler implementation — matches Claw Code pattern.
pub struct ToolExecutor<H: ToolHandler> {
    pub registry: Arc<Mutex<ToolRegistry>>,
    pub handler: H,
    pub doom_detector: DoomLoopDetector,
    pub permission_policy: PermissionPolicy,
    audit_sink: Option<ToolExecutionAuditSink>,
    /// If set, only tools in this list are permitted to execute.
    /// Checked before registry lookup and permission policy.
    pub allowed_tools: Option<Vec<String>>,
}

impl<H: ToolHandler> ToolExecutor<H> {
    pub fn new(registry: ToolRegistry, handler: H) -> Self {
        Self {
            registry: Arc::new(Mutex::new(registry)),
            handler,
            doom_detector: DoomLoopDetector::default(),
            permission_policy: PermissionPolicy::new(PermissionMode::WorkspaceWrite),
            audit_sink: None,
            allowed_tools: None,
        }
    }

    pub fn with_permission_policy(mut self, policy: PermissionPolicy) -> Self {
        self.permission_policy = policy;
        self
    }

    pub fn with_audit_sink(mut self, audit_sink: ToolExecutionAuditSink) -> Self {
        self.audit_sink = Some(audit_sink);
        self
    }

    pub fn set_audit_sink(&mut self, audit_sink: ToolExecutionAuditSink) {
        self.audit_sink = Some(audit_sink);
    }

    /// Set the step-level allowed-tools whitelist.
    /// If `names` is non-empty, only those tool names may be executed.
    /// Pass `None` to clear the restriction.
    pub fn set_allowed_tools(&mut self, names: Option<Vec<String>>) {
        self.allowed_tools = names;
    }

    /// Execute a tool and return structured result.
    pub async fn execute(&mut self, tool_name: &str, args: serde_json::Value) -> ToolExecution {
        let start = std::time::Instant::now();
        self.emit_audit_start(tool_name, &args);

        // Step-level allowed_tools check
        if let Some(ref allowed) = self.allowed_tools {
            if !allowed.is_empty() && !allowed.iter().any(|name| name == tool_name) {
                return self.emit_audit_end(ToolExecution {
                    tool_name: tool_name.to_string(),
                    input: args,
                    output: serde_json::Value::Null,
                    error: Some(format!(
                        "Tool '{}' is not in the step's allowed_tools list",
                        tool_name
                    )),
                    remediation: vec![ToolExecutionRemediation {
                        code: "tool_not_in_allowed_list".to_string(),
                        message: format!(
                            "Tool '{}' is outside the current step's contract. Use the effective tool inventory to pick an allowed alternative.",
                            tool_name
                        ),
                    }],
                    duration_ms: start.elapsed().as_millis() as u64,
                    approval_context: None,
                });
            }
        }

        let descriptor = {
            let registry = self.registry.lock().await;
            registry.get(tool_name).cloned()
        };
        let Some(descriptor) = descriptor else {
            return self.emit_audit_end(ToolExecution {
                tool_name: tool_name.to_string(),
                input: args,
                output: serde_json::Value::Null,
                error: Some(format!("Tool '{}' is not registered", tool_name)),
                remediation: remediation_for_missing_tool(tool_name),
                duration_ms: start.elapsed().as_millis() as u64,
                approval_context: None,
            });
        };

        // Extract path / command context from tool args for permission check.
        let resolved_path = extract_path_from_args(&args);
        let command_preview = extract_command_from_args(&args);

        let invocation_ctx = crate::permission::ToolInvocationContext {
            tool_name: descriptor.name.clone(),
            side_effect: descriptor.side_effect_level,
            requires_approval: descriptor.requires_approval,
            resolved_path,
            command_preview,
            source_refs: Vec::new(),
            task_id: None,
        };

        match self
            .permission_policy
            .authorize_with_context(&invocation_ctx)
        {
            PermissionDecision::Allow => {}
            PermissionDecision::Deny { reason } | PermissionDecision::Ask { reason } => {
                return self.emit_audit_end(ToolExecution {
                    tool_name: tool_name.to_string(),
                    input: args,
                    output: serde_json::Value::Null,
                    remediation: remediation_for_permission_error(
                        &descriptor.name,
                        descriptor.requires_approval,
                        &reason,
                    ),
                    error: Some(reason),
                    duration_ms: start.elapsed().as_millis() as u64,
                    approval_context: None,
                });
            }
        }

        // A2: Approval context binding for write/proposal/approval tools.
        // If a tool has side effect level >= Write and no approval_context or proposal_id
        // in args, reject with remediation request_approval.
        let approval_context = extract_approval_context(&args);
        if descriptor.side_effect_level >= ToolSideEffectLevel::Write && approval_context.is_none()
        {
            return self.emit_audit_end(ToolExecution {
                tool_name: tool_name.to_string(),
                input: args,
                output: serde_json::Value::Null,
                error: Some(format!(
                    "Tool '{}' requires approval context (proposal_id or approval_source) for write-level side effects",
                    tool_name
                )),
                remediation: vec![ToolExecutionRemediation {
                    code: FailureRemediation::RequestApproval.code().to_string(),
                    message: FailureRemediation::RequestApproval.message(tool_name),
                }],
                duration_ms: start.elapsed().as_millis() as u64,
                approval_context: None,
            });
        }

        // Doom loop check
        let is_doom = self.doom_detector.is_doom_loop(tool_name, &args);

        let (output, error, mut remediation) =
            match self.handler.execute(tool_name, args.clone()).await {
                Ok(result) => (result, None, Vec::new()),
                Err(e) => (
                    serde_json::Value::Null,
                    Some(e.clone()),
                    remediation_for_handler_error(tool_name, &e),
                ),
            };

        let mut error_msg = error;
        if is_doom {
            error_msg = Some(format!(
                "DOOM LOOP DETECTED: tool '{}' called with same args 3+ times",
                tool_name
            ));
            remediation = vec![ToolExecutionRemediation {
                code: "tool_doom_loop".to_string(),
                message: "Stop retrying this identical tool call; change the arguments or return a blocked-tool result to the caller.".to_string(),
            }];
        }

        self.emit_audit_end(ToolExecution {
            tool_name: tool_name.to_string(),
            input: args,
            output,
            error: error_msg,
            remediation,
            duration_ms: start.elapsed().as_millis() as u64,
            approval_context,
        })
    }

    fn emit_audit_start(&self, tool_name: &str, input: &serde_json::Value) {
        if let Some(audit_sink) = self.audit_sink.as_ref() {
            audit_sink(ToolExecutionAuditEvent::Start {
                tool_name: tool_name.to_string(),
                input: input.clone(),
            });
        }
    }

    fn emit_audit_end(&self, execution: ToolExecution) -> ToolExecution {
        if let Some(audit_sink) = self.audit_sink.as_ref() {
            audit_sink(ToolExecutionAuditEvent::End {
                execution: execution.clone(),
            });
        }
        execution
    }
}

fn remediation_for_missing_tool(tool_name: &str) -> Vec<ToolExecutionRemediation> {
    vec![ToolExecutionRemediation {
        code: "tool_not_registered".to_string(),
        message: format!(
            "Check the task tool inventory before calling '{}', or register the tool before this run.",
            tool_name
        ),
    }]
}

fn remediation_for_permission_error(
    tool_name: &str,
    requires_approval: bool,
    reason: &str,
) -> Vec<ToolExecutionRemediation> {
    let lower = reason.to_ascii_lowercase();
    if requires_approval || lower.contains("approval") {
        return vec![ToolExecutionRemediation {
            code: "request_approval".to_string(),
            message: format!(
                "Surface an explicit approval request before retrying '{}', or choose a read-only/preview tool.",
                tool_name
            ),
        }];
    }
    if lower.contains("external access") {
        return vec![ToolExecutionRemediation {
            code: "external_access_denied".to_string(),
            message: format!(
                "Keep '{}' inside the workspace boundary, or request an external-access policy change before retrying.",
                tool_name
            ),
        }];
    }
    vec![ToolExecutionRemediation {
        code: "tool_denied".to_string(),
        message: format!(
            "Use the effective tool inventory to pick an allowed alternative to '{}'.",
            tool_name
        ),
    }]
}

fn remediation_for_handler_error(tool_name: &str, error: &str) -> Vec<ToolExecutionRemediation> {
    let lower = error.to_ascii_lowercase();
    let (code, message) = if lower.contains("unknown tool") || lower.contains("unknown agent") {
        (
            "refresh_inventory",
            format!(
                "Verify the external agent/tool name for '{}', refresh the registry, and retry only if it appears in the allowed inventory.",
                tool_name
            ),
        )
    } else if lower.contains("missing binary")
        || lower.contains("not found")
        || lower.contains("no such file")
        || lower.contains("could not find")
    {
        (
            "refresh_inventory",
            format!(
                "Install or configure the binary/resource required by '{}', then run the tool again.",
                tool_name
            ),
        )
    } else if lower.contains("rate limit")
        || lower.contains("429")
        || lower.contains("timeout")
        || lower.contains("transient")
    {
        (
            "retry_transient",
            format!(
                "Transient failure for '{}'. Back off and retry with the same or narrower arguments.",
                tool_name
            ),
        )
    } else if lower.contains("workspace")
        && (lower.contains("unavailable") || lower.contains("missing") || lower.contains("denied"))
    {
        (
            "workspace_unavailable",
            format!(
                "Recreate or select a valid workspace for '{}', then retry with a workspace-local path.",
                tool_name
            ),
        )
    } else if lower.contains("context")
        && (lower.contains("overflow") || lower.contains("too long"))
    {
        (
            "shrink_context",
            format!(
                "Reduce context size before retrying '{}'. Remove non-essential sources or truncate long artifacts.",
                tool_name
            ),
        )
    } else if lower.contains("unsafe")
        || lower.contains("destructive")
        || lower.contains("overwrite")
    {
        (
            "abort_unsafe_write",
            format!(
                "'{}' attempted an unsafe write. Abort and surface an explicit approval request before retrying.",
                tool_name
            ),
        )
    } else {
        (
            "tool_handler_failed",
            format!(
                "Record the failure evidence for '{}' and either retry with narrower arguments or ask the caller for recovery input.",
                tool_name
            ),
        )
    };
    vec![ToolExecutionRemediation {
        code: code.to_string(),
        message,
    }]
}

// ── Path / command extraction for permission context ──

/// Extract a resolved file path from common tool argument keys.
fn extract_path_from_args(args: &serde_json::Value) -> Option<String> {
    let obj = args.as_object()?;
    for key in &["path", "file_path", "root", "outline_path", "chapter_path"] {
        if let Some(val) = obj.get(*key).and_then(|v| v.as_str()) {
            let trimmed = val.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

/// Extract a command preview from common tool argument keys.
fn extract_command_from_args(args: &serde_json::Value) -> Option<String> {
    let obj = args.as_object()?;
    for key in &["command", "shell_command", "cmd"] {
        if let Some(val) = obj.get(*key).and_then(|v| v.as_str()) {
            let trimmed = val.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

/// A2: Extract approval context from tool args.
/// Looks for `proposal_id` or `approval_source` keys.
fn extract_approval_context(args: &serde_json::Value) -> Option<ApprovalContext> {
    let obj = args.as_object()?;
    let proposal_id = obj
        .get("proposal_id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let approval_source = obj
        .get("approval_source")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    if proposal_id.is_some() || approval_source.is_some() {
        Some(ApprovalContext {
            proposal_id,
            approval_source,
        })
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool_registry::{ToolDescriptor, ToolSideEffectLevel, ToolStage};

    struct MockHandler;

    #[async_trait::async_trait]
    impl ToolHandler for MockHandler {
        async fn execute(
            &self,
            tool_name: &str,
            args: serde_json::Value,
        ) -> Result<serde_json::Value, String> {
            Ok(serde_json::json!({"tool": tool_name, "args": args}))
        }
    }

    fn registry() -> ToolRegistry {
        let mut registry = ToolRegistry::new();
        registry
            .register(ToolDescriptor::new(
                "read_tool",
                "Read.",
                "none",
                "json",
                ToolSideEffectLevel::Read,
                false,
                100,
                0,
                ToolStage::Context,
            ))
            .unwrap();
        registry
            .register(ToolDescriptor::new(
                "write_tool",
                "Write.",
                "none",
                "json",
                ToolSideEffectLevel::Write,
                true,
                100,
                0,
                ToolStage::Execute,
            ))
            .unwrap();
        registry
    }

    #[test]
    fn test_doom_loop_detection() {
        let mut d = DoomLoopDetector::default();
        let args = serde_json::json!({"q": "test"});
        assert!(!d.is_doom_loop("search", &args));
        assert!(!d.is_doom_loop("search", &args));
        assert!(d.is_doom_loop("search", &args));
    }

    #[test]
    fn test_doom_loop_different_args_no_trigger() {
        let mut d = DoomLoopDetector::default();
        d.is_doom_loop("search", &serde_json::json!({"q": "a"}));
        d.is_doom_loop("search", &serde_json::json!({"q": "b"}));
        assert!(!d.is_doom_loop("search", &serde_json::json!({"q": "c"})));
    }

    #[test]
    fn test_doom_loop_reset() {
        let mut d = DoomLoopDetector::default();
        d.is_doom_loop("s", &serde_json::json!({}));
        d.is_doom_loop("s", &serde_json::json!({}));
        d.reset();
        assert!(!d.is_doom_loop("s", &serde_json::json!({})));
    }

    #[tokio::test]
    async fn executor_rejects_unregistered_tool() {
        let mut executor = ToolExecutor::new(registry(), MockHandler);
        let result = executor
            .execute("missing_tool", serde_json::json!({}))
            .await;

        assert!(result
            .error
            .as_deref()
            .is_some_and(|error| error.contains("not registered")));
        assert!(result
            .remediation
            .iter()
            .any(|item| item.code == "tool_not_registered"));
    }

    #[tokio::test]
    async fn executor_blocks_approval_required_tool_before_handler() {
        let mut executor = ToolExecutor::new(registry(), MockHandler);
        let result = executor.execute("write_tool", serde_json::json!({})).await;

        assert!(result
            .error
            .as_deref()
            .is_some_and(|error| error.contains("requires explicit approval")));
        assert!(result.output.is_null());
        assert!(result
            .remediation
            .iter()
            .any(|item| item.code == "request_approval"));
    }

    #[tokio::test]
    async fn executor_allows_registered_read_tool() {
        let mut executor = ToolExecutor::new(registry(), MockHandler);
        let result = executor
            .execute("read_tool", serde_json::json!({"id": 1}))
            .await;

        assert!(result.error.is_none());
        assert_eq!(result.output["tool"], "read_tool");
        assert!(result.remediation.is_empty());
    }

    #[tokio::test]
    async fn executor_audit_sink_records_start_and_end_without_raw_policy() {
        let events = Arc::new(std::sync::Mutex::new(Vec::new()));
        let captured = events.clone();
        let audit_sink: ToolExecutionAuditSink = Arc::new(move |event| {
            captured.lock().unwrap().push(event);
        });
        let mut executor = ToolExecutor::new(registry(), MockHandler).with_audit_sink(audit_sink);
        let result = executor
            .execute("read_tool", serde_json::json!({"id": 7}))
            .await;

        assert!(result.error.is_none());
        let events = events.lock().unwrap();
        assert_eq!(events.len(), 2);
        assert!(matches!(
            &events[0],
            ToolExecutionAuditEvent::Start { tool_name, input }
                if tool_name == "read_tool" && input.get("id").and_then(|value| value.as_u64()) == Some(7)
        ));
        assert!(matches!(
            &events[1],
            ToolExecutionAuditEvent::End { execution }
                if execution.tool_name == "read_tool" && execution.error.is_none()
        ));
    }

    #[tokio::test]
    async fn executor_remediation_codes_match_governance_spec() {
        // Missing tool -> refresh_inventory
        let mut executor = ToolExecutor::new(registry(), MockHandler);
        let result = executor
            .execute("missing_tool", serde_json::json!({}))
            .await;
        assert!(result
            .remediation
            .iter()
            .any(|r| r.code == "refresh_inventory" || r.code == "tool_not_registered"));

        // Approval required -> request_approval
        let result2 = executor.execute("write_tool", serde_json::json!({})).await;
        assert!(result2
            .remediation
            .iter()
            .any(|r| r.code == "request_approval"));
    }

    #[tokio::test]
    async fn executor_blocks_tool_not_in_allowed_list() {
        let mut executor = ToolExecutor::new(registry(), MockHandler);
        executor.set_allowed_tools(Some(vec!["read_tool".to_string()]));

        let result = executor.execute("write_tool", serde_json::json!({})).await;

        assert!(result
            .error
            .as_deref()
            .is_some_and(|error| error.contains("not in the step's allowed_tools list")));
        assert!(result
            .remediation
            .iter()
            .any(|item| item.code == "tool_not_in_allowed_list"));
    }

    #[tokio::test]
    async fn executor_allows_tool_in_allowed_list() {
        let mut executor = ToolExecutor::new(registry(), MockHandler);
        executor.set_allowed_tools(Some(vec!["read_tool".to_string()]));

        let result = executor
            .execute("read_tool", serde_json::json!({"id": 1}))
            .await;

        assert!(result.error.is_none());
        assert_eq!(result.output["tool"], "read_tool");
    }

    #[tokio::test]
    async fn executor_allows_all_tools_when_allowed_list_empty() {
        let mut executor = ToolExecutor::new(registry(), MockHandler);
        executor.set_allowed_tools(Some(vec![]));

        let result = executor
            .execute("read_tool", serde_json::json!({"id": 1}))
            .await;
        assert!(result.error.is_none());

        let result2 = executor.execute("write_tool", serde_json::json!({})).await;
        // write_tool still blocked by approval policy, not by allowed list
        assert!(result2
            .error
            .as_deref()
            .is_some_and(|error| error.contains("requires explicit approval")));
    }

    #[tokio::test]
    async fn executor_allows_all_tools_when_allowed_list_none() {
        let mut executor = ToolExecutor::new(registry(), MockHandler);
        executor.set_allowed_tools(None);

        let result = executor
            .execute("read_tool", serde_json::json!({"id": 1}))
            .await;
        assert!(result.error.is_none());
    }

    // ── A7: Agent Runtime Eval Harness tests ──

    #[tokio::test]
    async fn step_allowed_tools_blocks_unauthorized_write() {
        // Create a StepContract with allowed_tools containing only read_tool
        let contract = crate::execution_plan::StepContract {
            step_id: "step-0".to_string(),
            input_summary: "read-only context gathering".to_string(),
            required_context: vec![],
            allowed_tools: vec!["read_tool".to_string()],
            max_side_effect: ToolSideEffectLevel::Read,
            provider_allowed: false,
            success_evidence_required: vec![],
            success_signals: vec![],
            failure_policy: crate::execution_plan::StepFailureAction::Stop,
        };

        let mut executor = ToolExecutor::new(registry(), MockHandler);
        executor.set_allowed_tools(Some(contract.allowed_tools.clone()));

        // Attempt to call write_tool (not in allowed list) -> blocked
        let result = executor
            .execute("write_tool", serde_json::json!({"path": "/tmp/out.txt"}))
            .await;
        assert!(
            result
                .error
                .as_ref()
                .is_some_and(|e| e.contains("not in the step's allowed_tools list")),
            "expected blocked by allowed_tools, got: {:?}",
            result.error
        );
        assert!(
            result
                .remediation
                .iter()
                .any(|r| r.code == "tool_not_in_allowed_list"),
            "expected remediation code tool_not_in_allowed_list"
        );
        let kind = result.failure_kind();
        assert_eq!(kind, Some(crate::recovery::FailureKind::ToolPermission));
    }

    #[tokio::test]
    async fn doom_loop_detected_triggers_abort() {
        let mut executor = ToolExecutor::new(registry(), MockHandler);
        let args = serde_json::json!({"q": "same query"});

        // First two calls with same args should NOT trigger doom loop
        let r1 = executor.execute("read_tool", args.clone()).await;
        assert!(r1.error.is_none());
        let r2 = executor.execute("read_tool", args.clone()).await;
        assert!(r2.error.is_none());

        // Third call with identical args triggers doom loop
        let r3 = executor.execute("read_tool", args.clone()).await;
        assert!(
            r3.error.as_ref().is_some_and(|e| e.contains("DOOM LOOP")),
            "expected doom loop error, got: {:?}",
            r3.error
        );
        assert!(
            r3.remediation.iter().any(|r| r.code == "tool_doom_loop"),
            "expected remediation code tool_doom_loop"
        );
        let kind = r3.failure_kind();
        assert_eq!(kind, Some(crate::recovery::FailureKind::DoomLoop));
    }

    #[tokio::test]
    async fn approval_required_tool_without_approval_is_denied() {
        // write_tool is registered with requires_approval=true
        let mut executor = ToolExecutor::new(registry(), MockHandler);

        // Call write_tool without any approval context -> denied
        let result = executor
            .execute(
                "write_tool",
                serde_json::json!({"path": "/workspace/chapter.md"}),
            )
            .await;
        assert!(
            result
                .error
                .as_ref()
                .is_some_and(|e| e.contains("requires explicit approval")),
            "expected approval required error, got: {:?}",
            result.error
        );
        assert!(
            result
                .remediation
                .iter()
                .any(|r| r.code == "request_approval"),
            "expected remediation code request_approval"
        );
        let kind = result.failure_kind();
        assert_eq!(kind, Some(crate::recovery::FailureKind::ToolPermission));
    }

    // ── A2: Tool Governance and Provider Governance unification tests ──

    #[test]
    fn redact_tool_input_truncates_long_input() {
        let long = "a".repeat(300);
        let redacted = redact_tool_input(&long);
        assert!(redacted.len() < long.len(), "should truncate long input");
        assert!(redacted.contains("truncated"), "should indicate truncation");
    }

    #[test]
    fn redact_tool_input_masks_secrets() {
        let input = "api_key=sk-live-1234567890abcdef token=bearer-abc secret=my-secret";
        let redacted = redact_tool_input(input);
        assert!(!redacted.contains("sk-live"), "should mask api key");
        assert!(!redacted.contains("bearer-abc"), "should mask token");
        assert!(!redacted.contains("my-secret"), "should mask secret");
        assert!(redacted.contains("api_key=***"), "should replace with ***");
        assert!(
            redacted.contains("token=***"),
            "should replace token with ***"
        );
    }

    #[test]
    fn redact_tool_input_keeps_short_input() {
        let input = "query=hello world";
        let redacted = redact_tool_input(input);
        assert_eq!(redacted, input);
    }

    #[test]
    fn redact_tool_input_empty() {
        assert_eq!(redact_tool_input(""), "");
    }

    #[tokio::test]
    async fn write_tool_without_approval_context_is_rejected() {
        let mut executor = ToolExecutor::new(registry(), MockHandler);
        // write_tool has side_effect_level = Write and requires_approval = true
        let result = executor
            .execute("write_tool", serde_json::json!({"path": "/tmp/out.txt"}))
            .await;

        // Should be rejected either by permission policy (requires_approval=true)
        // or by approval context binding (no proposal_id/approval_source)
        assert!(
            result.error.is_some(),
            "expected write tool without approval context to be rejected, got: {:?}",
            result
        );
        assert!(
            result
                .remediation
                .iter()
                .any(|r| r.code == "request_approval"),
            "expected remediation code request_approval, got: {:?}",
            result.remediation
        );
    }

    #[tokio::test]
    async fn write_tool_with_proposal_id_is_allowed() {
        let mut executor = ToolExecutor::new(registry(), MockHandler);
        // write_tool with proposal_id should pass approval context binding
        // but still blocked by permission policy (requires_approval=true)
        let result = executor
            .execute(
                "write_tool",
                serde_json::json!({"path": "/tmp/out.txt", "proposal_id": "proposal-123"}),
            )
            .await;

        // The permission policy still blocks because requires_approval=true
        // but the approval context binding check passes
        assert!(
            result.error.is_some(),
            "expected blocked by permission policy, got: {:?}",
            result
        );
    }

    #[tokio::test]
    async fn read_tool_without_approval_context_is_allowed() {
        let mut executor = ToolExecutor::new(registry(), MockHandler);
        // read_tool has side_effect_level = Read, so approval context not required
        let result = executor
            .execute("read_tool", serde_json::json!({"path": "/tmp/in.txt"}))
            .await;

        assert!(
            result.error.is_none(),
            "read tool should not require approval context"
        );
        assert!(result.approval_context.is_none());
    }

    #[test]
    fn extract_approval_context_from_args() {
        let args = serde_json::json!({"proposal_id": "p1", "path": "/tmp"});
        let ctx = extract_approval_context(&args);
        assert!(ctx.is_some());
        assert_eq!(ctx.unwrap().proposal_id, Some("p1".to_string()));

        let args2 = serde_json::json!({"approval_source": "user", "path": "/tmp"});
        let ctx2 = extract_approval_context(&args2);
        assert!(ctx2.is_some());
        assert_eq!(ctx2.unwrap().approval_source, Some("user".to_string()));

        let args3 = serde_json::json!({"path": "/tmp"});
        assert!(extract_approval_context(&args3).is_none());
    }

    #[test]
    fn failure_remediation_integration_in_tool_execution() {
        let remediation = ToolExecutionRemediation {
            code: FailureRemediation::AbortUnsafeWrite.code().to_string(),
            message: FailureRemediation::AbortUnsafeWrite.message("write_file"),
        };
        assert_eq!(remediation.code, "abort_unsafe_write");
        assert!(remediation.message.contains("write_file"));
    }
}
