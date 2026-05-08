//! Tool lifecycle hooks — allows injecting logic before/after tool execution.
//! Ported from Claw Code `hooks.rs` (PreToolUse / PostToolUse / PostToolUseFailure).
//!
//! Hooks are shell commands or internal callbacks that receive JSON payloads
//! via stdin and return structured decisions.
//!
//! Use cases: permission override, input sanitization, audit logging,
//! content safety filtering, custom approval flows.

/// Hook event type — mirrors Claw Code HookEvent.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum HookEvent {
    PreToolUse,
    PostToolUse,
    PostToolUseFailure,
}

/// Decision a hook can return.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "decision")]
pub enum HookDecision {
    #[serde(rename = "allow")]
    Allow,
    #[serde(rename = "deny")]
    Deny { reason: String },
    #[serde(rename = "ask")]
    Ask { reason: String },
    #[serde(rename = "modify")]
    ModifyInput { updated_input: serde_json::Value },
}

/// Payload passed to a hook.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct HookPayload {
    pub event: HookEvent,
    pub tool_name: String,
    pub tool_input: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_output: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// A registered hook handler.
pub type HookHandler = Box<dyn Fn(&HookPayload) -> HookDecision + Send + Sync>;

/// HookRunner manages registered hooks and executes them in order.
/// Hooks can short-circuit: a Deny or ModifyInput stops the chain.
pub struct HookRunner {
    pre_tool_hooks: Vec<HookHandler>,
    post_tool_hooks: Vec<HookHandler>,
    failure_hooks: Vec<HookHandler>,
}

impl HookRunner {
    pub fn new() -> Self {
        Self {
            pre_tool_hooks: Vec::new(),
            post_tool_hooks: Vec::new(),
            failure_hooks: Vec::new(),
        }
    }

    pub fn on_pre_tool(&mut self, handler: HookHandler) {
        self.pre_tool_hooks.push(handler);
    }

    pub fn on_post_tool(&mut self, handler: HookHandler) {
        self.post_tool_hooks.push(handler);
    }

    pub fn on_failure(&mut self, handler: HookHandler) {
        self.failure_hooks.push(handler);
    }

    /// Run pre-tool hooks. Returns the effective decision after all hooks.
    /// The last non-Allow decision wins (allows progressive escalation).
    pub fn run_pre_tool(&self, tool_name: &str, input: &serde_json::Value) -> HookDecision {
        let payload = HookPayload {
            event: HookEvent::PreToolUse,
            tool_name: tool_name.to_string(),
            tool_input: input.clone(),
            tool_output: None,
            error: None,
        };
        let mut decision = HookDecision::Allow;
        for handler in &self.pre_tool_hooks {
            match handler(&payload) {
                HookDecision::Allow => {}
                d => decision = d,
            }
        }
        decision
    }

    /// Run post-tool hooks. Called after successful tool execution.
    pub fn run_post_tool(
        &self,
        tool_name: &str,
        input: &serde_json::Value,
        output: &serde_json::Value,
    ) {
        let payload = HookPayload {
            event: HookEvent::PostToolUse,
            tool_name: tool_name.to_string(),
            tool_input: input.clone(),
            tool_output: Some(output.clone()),
            error: None,
        };
        for handler in &self.post_tool_hooks {
            handler(&payload);
        }
    }

    /// Run failure hooks. Called when tool execution fails.
    pub fn run_failure(&self, tool_name: &str, input: &serde_json::Value, error: &str) {
        let payload = HookPayload {
            event: HookEvent::PostToolUseFailure,
            tool_name: tool_name.to_string(),
            tool_input: input.clone(),
            tool_output: None,
            error: Some(error.to_string()),
        };
        for handler in &self.failure_hooks {
            handler(&payload);
        }
    }
}

impl Default for HookRunner {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pre_tool_allow() {
        let mut runner = HookRunner::new();
        runner.on_pre_tool(Box::new(|_| HookDecision::Allow));
        let d = runner.run_pre_tool("test", &serde_json::json!({}));
        assert!(matches!(d, HookDecision::Allow));
    }

    #[test]
    fn test_pre_tool_deny_wins() {
        let mut runner = HookRunner::new();
        runner.on_pre_tool(Box::new(|_| HookDecision::Allow));
        runner.on_pre_tool(Box::new(|_| HookDecision::Deny {
            reason: "blocked".into(),
        }));
        let d = runner.run_pre_tool("test", &serde_json::json!({}));
        assert!(matches!(d, HookDecision::Deny { .. }));
    }

    #[test]
    fn test_post_tool_no_panic() {
        let mut runner = HookRunner::new();
        runner.on_post_tool(Box::new(|p| {
            assert_eq!(p.tool_name, "test");
            HookDecision::Allow
        }));
        runner.run_post_tool(
            "test",
            &serde_json::json!({}),
            &serde_json::json!({"ok": true}),
        );
    }
}
