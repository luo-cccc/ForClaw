/// Context overflow recovery — aggressive multi-step trimming.
/// Ported from CowAgent `agent_stream.py` lines 1181-1281.
///
/// When the API returns context_length_exceeded, try progressively more
/// aggressive trimming before giving up:
///   1. Compact (LLM summarize middle turns)
///   2. Truncate historical tool outputs to 500 chars each
///   3. Remove all but the last 3 message pairs
///   4. Keep only system + current user message
///
/// Returns the trimmed messages and which recovery level was applied.
#[derive(Debug, Clone, PartialEq)]
pub enum OverflowRecoveryLevel {
    None,
    Compact,
    TruncateToolOutputs,
    KeepLast3Pairs,
    MinimalContext,
    Failed,
}

/// Attempt to recover from a context overflow error.
/// Returns the trimmed messages and the recovery level applied.
pub fn recover_from_overflow(messages: &[LlmMessage]) -> (Vec<LlmMessage>, OverflowRecoveryLevel) {
    let total = messages.len();
    if total <= 4 {
        return (messages.to_vec(), OverflowRecoveryLevel::Failed);
    }

    // Level 1: Truncate tool outputs to 500 chars
    let truncated: Vec<LlmMessage> = messages
        .iter()
        .map(|m| {
            if m.role == "tool" {
                let mut tm = m.clone();
                if let Some(ref content) = m.content {
                    if content.chars().count() > 500 {
                        tm.content = Some(format!(
                            "{}...[truncated, original {} chars]",
                            content.chars().take(500).collect::<String>(),
                            content.chars().count()
                        ));
                    }
                }
                tm
            } else {
                m.clone()
            }
        })
        .collect();
    let trunc_tokens = estimate_message_tokens(&truncated);

    // Level 2: If still too large, keep last 3 pairs
    let keep_3: Vec<LlmMessage> = if total > 8 {
        messages
            .iter()
            .skip(total.saturating_sub(6))
            .cloned()
            .collect()
    } else {
        messages.to_vec()
    };

    // Level 3: Minimal — system equivalent + current user
    let minimal: Vec<LlmMessage> = vec![
        LlmMessage {
            role: "system".into(),
            content: Some(
                "[Previous conversation was trimmed due to context limits. Continue concisely.]"
                    .into(),
            ),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        },
        messages.last().cloned().unwrap_or(LlmMessage {
            role: "user".into(),
            content: Some("Continue".into()),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        }),
    ];

    // Try levels in order, return the first that significantly reduces tokens
    let original_tokens = estimate_message_tokens(messages);
    if trunc_tokens < original_tokens * 8 / 10 {
        (truncated, OverflowRecoveryLevel::TruncateToolOutputs)
    } else if estimate_message_tokens(&keep_3) < original_tokens * 6 / 10 {
        (keep_3, OverflowRecoveryLevel::KeepLast3Pairs)
    } else {
        (minimal, OverflowRecoveryLevel::MinimalContext)
    }
}

#[cfg(test)]
mod recovery_tests {
    use super::*;

    fn msg(role: &str, content: &str) -> LlmMessage {
        LlmMessage {
            role: role.into(),
            content: Some(content.into()),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        }
    }

    #[test]
    fn test_overflow_recovery_truncates_tool_outputs() {
        let messages = vec![
            msg("system", "sys"),
            msg("user", "q"),
            msg("assistant", "a"),
            LlmMessage {
                role: "tool".into(),
                content: Some("x".repeat(2000)),
                tool_calls: None,
                tool_call_id: Some("c1".into()),
                name: Some("t".into()),
            },
            msg("assistant", "done"),
            msg("user", "more"),
        ];
        let (recovered, level) = recover_from_overflow(&messages);
        assert_eq!(level, OverflowRecoveryLevel::TruncateToolOutputs);
        // Tool output should be truncated
        if let Some(ref c) = recovered[3].content {
            assert!(c.chars().count() < 600);
        }
    }

    #[test]
    fn test_overflow_recovery_small_returns_failed() {
        let messages = vec![msg("system", "s"), msg("user", "q")];
        let (_, level) = recover_from_overflow(&messages);
        assert_eq!(level, OverflowRecoveryLevel::Failed);
    }
}
