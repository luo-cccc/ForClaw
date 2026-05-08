use crate::tool_registry::ToolSideEffectLevel;

/// Permission mode for the agent.
/// Ported from Claw Code `permissions.rs`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum PermissionMode {
    ReadOnly,
    WorkspaceWrite,
    DangerFullAccess,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum PermissionDecision {
    Allow,
    Deny { reason: String },
    Ask { reason: String },
}

#[derive(Debug, Clone)]
pub struct PermissionRule {
    pub pattern: String,
    pub action: PermissionDecision,
}

/// Context for a single tool invocation — used for path/command-level checks.
#[derive(Debug, Clone)]
pub struct ToolInvocationContext {
    pub tool_name: String,
    pub side_effect: crate::tool_registry::ToolSideEffectLevel,
    pub requires_approval: bool,
    pub resolved_path: Option<String>,
    pub command_preview: Option<String>,
    pub source_refs: Vec<String>,
    pub task_id: Option<String>,
}

/// Sensitive path patterns that are always denied regardless of mode.
/// Ported from OpenHarness permission checker SENSITIVE_PATH_PATTERNS.
pub const SENSITIVE_PATH_PATTERNS: &[&str] = &[
    "*/.ssh/*",
    "*/.aws/credentials",
    "*/.aws/config",
    "*/.config/gcloud/*",
    "*/.azure/*",
    "*/.gnupg/*",
    "*/.docker/config.json",
    "*/.kube/config",
    "*/.openharness/credentials.json",
    "*.pem",
    "*.key",
    "*.pfx",
];

/// Dangerous command patterns that are denied even in FullAccess.
pub const DANGEROUS_COMMAND_PATTERNS: &[&str] = &[
    "rm -rf /*",
    "rm -rf /",
    "dd if=*",
    "mkfs.*",
    ">: *",
    "chmod 777 /*",
];

/// Permission policy — evaluates rules against tool invocations.
/// Pipeline: sensitive path deny → deny rules → mode escalation → approval → allow rules.
pub struct PermissionPolicy {
    pub mode: PermissionMode,
    pub rules: Vec<PermissionRule>,
    pub deny_rules: Vec<PermissionRule>,
}

impl PermissionPolicy {
    pub fn new(mode: PermissionMode) -> Self {
        Self {
            mode,
            rules: Vec::new(),
            deny_rules: Vec::new(),
        }
    }

    pub fn authorize(
        &self,
        tool_name: &str,
        side_effect: ToolSideEffectLevel,
        requires_approval: bool,
    ) -> PermissionDecision {
        // 1. Check deny rules (always reject)
        for rule in &self.deny_rules {
            if tool_matches(tool_name, &rule.pattern) {
                if let PermissionDecision::Deny { ref reason } = rule.action {
                    return PermissionDecision::Deny {
                        reason: reason.clone(),
                    };
                }
            }
        }

        // 2. Mode-based escalation
        match self.mode {
            PermissionMode::ReadOnly => {
                if side_effect >= ToolSideEffectLevel::Write {
                    return PermissionDecision::Deny {
                        reason: format!(
                            "Tool '{}' requires write access but agent is in ReadOnly mode",
                            tool_name
                        ),
                    };
                }
                if side_effect >= ToolSideEffectLevel::ProviderCall {
                    return PermissionDecision::Ask {
                        reason: "External API call in ReadOnly mode".into(),
                    };
                }
            }
            PermissionMode::WorkspaceWrite => {
                if side_effect >= ToolSideEffectLevel::External {
                    return PermissionDecision::Deny {
                        reason: format!(
                            "Tool '{}' requires external access beyond workspace",
                            tool_name
                        ),
                    };
                }
            }
            PermissionMode::DangerFullAccess => {}
        }

        // 3. Explicit approval requirement
        if requires_approval {
            return PermissionDecision::Ask {
                reason: format!("Tool '{}' requires explicit approval", tool_name),
            };
        }

        // 4. Check allow rules
        for rule in &self.rules {
            if tool_matches(tool_name, &rule.pattern) {
                return rule.action.clone();
            }
        }

        PermissionDecision::Allow
    }

    /// Authorize with full invocation context — checks sensitive paths and
    /// dangerous commands in addition to the base policy.
    pub fn authorize_with_context(&self, ctx: &ToolInvocationContext) -> PermissionDecision {
        // 0. Built-in sensitive path protection (always active).
        if let Some(ref path) = ctx.resolved_path {
            for pattern in SENSITIVE_PATH_PATTERNS {
                if simple_fnmatch(path, pattern) {
                    return PermissionDecision::Deny {
                        reason: format!(
                            "Access denied: {} is a sensitive credential path (matched pattern '{}')",
                            path, pattern
                        ),
                    };
                }
            }
        }

        // 0b. Dangerous command patterns.
        if let Some(ref cmd) = ctx.command_preview {
            for pattern in DANGEROUS_COMMAND_PATTERNS {
                if simple_fnmatch(cmd, pattern) {
                    return PermissionDecision::Deny {
                        reason: format!(
                            "Command denied: '{}' matches dangerous pattern '{}'",
                            cmd, pattern
                        ),
                    };
                }
            }
        }

        // Delegate to base authorize.
        self.authorize(&ctx.tool_name, ctx.side_effect, ctx.requires_approval)
    }
}

/// Simple fnmatch-style matching for path/command patterns.
fn simple_fnmatch(text: &str, pattern: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    // Convert glob pattern to prefix/suffix checks
    let text_lower = text.to_lowercase();
    let pattern_lower = pattern.to_lowercase();
    if pattern_lower == text_lower {
        return true;
    }
    if let Some(suffix) = pattern_lower.strip_prefix('*') {
        if text_lower.ends_with(suffix) {
            return true;
        }
    }
    if let Some(prefix) = pattern_lower.strip_suffix('*') {
        if text_lower.starts_with(prefix) {
            return true;
        }
    }
    // Handle *middle* patterns like "*/.ssh/*"
    if pattern_lower.contains('*') {
        let parts: Vec<&str> = pattern_lower.split('*').collect();
        if parts.len() >= 3 {
            let start = parts[0];
            let middle = parts[1];
            if text_lower.starts_with(start) && text_lower.contains(middle) {
                return true;
            }
        }
    }
    false
}

fn tool_matches(tool_name: &str, pattern: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if let Some(prefix) = pattern.strip_suffix('*') {
        return tool_name.starts_with(prefix);
    }
    tool_name == pattern
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_readonly_denies_write() {
        let policy = PermissionPolicy::new(PermissionMode::ReadOnly);
        let d = policy.authorize("generate_chapter_draft", ToolSideEffectLevel::Write, false);
        assert!(matches!(d, PermissionDecision::Deny { .. }));
    }

    #[test]
    fn test_readonly_allows_read() {
        let policy = PermissionPolicy::new(PermissionMode::ReadOnly);
        let d = policy.authorize("load_current_chapter", ToolSideEffectLevel::Read, false);
        assert!(matches!(d, PermissionDecision::Allow));
    }

    #[test]
    fn test_approval_triggers_ask() {
        let policy = PermissionPolicy::new(PermissionMode::WorkspaceWrite);
        let d = policy.authorize("generate_chapter_draft", ToolSideEffectLevel::Write, true);
        assert!(matches!(d, PermissionDecision::Ask { .. }));
    }

    #[test]
    fn test_deny_rule_overrides() {
        let mut policy = PermissionPolicy::new(PermissionMode::DangerFullAccess);
        policy.deny_rules.push(PermissionRule {
            pattern: "generate_*".into(),
            action: PermissionDecision::Deny {
                reason: "blocked".into(),
            },
        });
        let d = policy.authorize("generate_chapter_draft", ToolSideEffectLevel::Write, false);
        assert!(matches!(d, PermissionDecision::Deny { .. }));
    }

    #[test]
    fn test_tool_matches() {
        assert!(tool_matches("search_lorebook", "search_*"));
        assert!(tool_matches("load", "*"));
        assert!(!tool_matches("search", "load_*"));
    }
}

#[cfg(test)]
mod context_tests {
    use super::*;
    use crate::tool_registry::ToolSideEffectLevel;

    #[test]
    fn permission_denies_sensitive_ssh_path() {
        let policy = PermissionPolicy::new(PermissionMode::DangerFullAccess);
        let ctx = ToolInvocationContext {
            tool_name: "read_file".to_string(),
            side_effect: ToolSideEffectLevel::Read,
            requires_approval: false,
            resolved_path: Some("/home/user/.ssh/id_rsa".to_string()),
            command_preview: None,
            source_refs: vec![],
            task_id: None,
        };
        let decision = policy.authorize_with_context(&ctx);
        assert!(matches!(decision, PermissionDecision::Deny { .. }));
    }

    #[test]
    fn permission_denies_dangerous_rm_rf_command() {
        let policy = PermissionPolicy::new(PermissionMode::DangerFullAccess);
        let ctx = ToolInvocationContext {
            tool_name: "run_shell".to_string(),
            side_effect: ToolSideEffectLevel::Write,
            requires_approval: false,
            resolved_path: None,
            command_preview: Some("rm -rf /".to_string()),
            source_refs: vec![],
            task_id: None,
        };
        let decision = policy.authorize_with_context(&ctx);
        assert!(
            matches!(decision, PermissionDecision::Deny { .. }),
            "dangerous command should be denied even in FullAccess: {:?}",
            decision
        );
    }

    #[test]
    fn permission_context_overrides_full_access_for_credentials() {
        let policy = PermissionPolicy::new(PermissionMode::DangerFullAccess);
        let ctx = ToolInvocationContext {
            tool_name: "read_file".to_string(),
            side_effect: ToolSideEffectLevel::Read,
            requires_approval: false,
            resolved_path: Some("/root/.aws/credentials".to_string()),
            command_preview: None,
            source_refs: vec![],
            task_id: None,
        };
        let decision = policy.authorize_with_context(&ctx);
        assert!(
            matches!(decision, PermissionDecision::Deny { .. }),
            "credential path should be denied even in FullAccess: {:?}",
            decision
        );
    }

    #[test]
    fn safe_path_allowed_in_workspace_write() {
        let policy = PermissionPolicy::new(PermissionMode::WorkspaceWrite);
        let ctx = ToolInvocationContext {
            tool_name: "load_chapter".to_string(),
            side_effect: ToolSideEffectLevel::Read,
            requires_approval: false,
            resolved_path: Some("/project/chapter-1.md".to_string()),
            command_preview: None,
            source_refs: vec![],
            task_id: None,
        };
        let decision = policy.authorize_with_context(&ctx);
        assert!(
            matches!(decision, PermissionDecision::Allow),
            "safe path should be allowed: {:?}",
            decision
        );
    }
}
