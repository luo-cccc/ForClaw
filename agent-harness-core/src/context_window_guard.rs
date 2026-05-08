use crate::provider::LlmMessage;

pub const DEFAULT_CONTEXT_WINDOW_TOKENS: u64 = 120_000;
pub const CONTEXT_WINDOW_HARD_MIN_TOKENS: u64 = 4_000;
pub const CONTEXT_WINDOW_WARN_BELOW_TOKENS: u64 = 8_000;
const HARD_MIN_RATIO: f64 = 0.10;
const WARN_BELOW_RATIO: f64 = 0.20;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextWindowSource {
    ModelMetadata,
    Env,
    Default,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextWindowInfo {
    pub tokens: u64,
    pub reference_tokens: Option<u64>,
    pub source: ContextWindowSource,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextWindowGuard {
    pub tokens: u64,
    pub reference_tokens: u64,
    pub source: ContextWindowSource,
    pub hard_min_tokens: u64,
    pub warn_below_tokens: u64,
    pub estimated_input_tokens: u64,
    pub requested_output_tokens: u64,
    pub should_warn: bool,
    pub should_block: bool,
    pub message: Option<String>,
}

pub fn estimate_text_tokens(text: &str) -> u64 {
    text.chars().count() as u64 / 3
}

pub fn estimate_request_tokens(messages: &[LlmMessage], system_prompt: Option<&str>) -> u64 {
    let message_tokens = messages
        .iter()
        .map(|message| {
            message
                .content
                .as_ref()
                .map(|content| estimate_text_tokens(content))
                .unwrap_or(0)
                + 8
        })
        .sum::<u64>();
    message_tokens + system_prompt.map(estimate_text_tokens).unwrap_or(0)
}

pub fn resolve_context_window_info(model_id: &str) -> ContextWindowInfo {
    if let Ok(value) = std::env::var("OPENAI_CONTEXT_TOKENS") {
        if let Some(tokens) = parse_positive_u64(&value) {
            return ContextWindowInfo {
                tokens,
                reference_tokens: None,
                source: ContextWindowSource::Env,
            };
        }
    }

    let lower = model_id.to_ascii_lowercase();
    let known = [
        ("deepseek/deepseek-v4-flash", 64_000),
        ("deepseek-v4-flash", 64_000),
        ("deepseek/deepseek-chat", 64_000),
        ("deepseek-chat", 64_000),
        ("deepseek/deepseek-coder", 128_000),
        ("deepseek-coder", 128_000),
        ("gpt-4o-mini", 128_000),
        ("gpt-4o", 128_000),
        ("gpt-4.1", 1_047_576),
        ("gpt-4.1-mini", 1_047_576),
        ("gpt-4.1-nano", 1_047_576),
        ("gpt-5", 400_000),
        ("gpt-5-mini", 400_000),
        ("gpt-5-nano", 400_000),
        ("claude-3-5-sonnet", 200_000),
        ("claude-3.5-sonnet", 200_000),
        ("claude-3-7-sonnet", 200_000),
        ("claude-3.7-sonnet", 200_000),
        ("claude-sonnet-4", 200_000),
        ("gemini-1.5-pro", 1_000_000),
        ("gemini-1.5-flash", 1_000_000),
        ("gemini-2.0-flash", 1_000_000),
    ];

    for (needle, tokens) in known {
        if lower.contains(needle) {
            return ContextWindowInfo {
                tokens,
                reference_tokens: None,
                source: ContextWindowSource::ModelMetadata,
            };
        }
    }

    ContextWindowInfo {
        tokens: DEFAULT_CONTEXT_WINDOW_TOKENS,
        reference_tokens: None,
        source: ContextWindowSource::Default,
    }
}

pub fn evaluate_context_window(
    info: ContextWindowInfo,
    estimated_input_tokens: u64,
    requested_output_tokens: u64,
) -> ContextWindowGuard {
    let reference_tokens = info.reference_tokens.unwrap_or(info.tokens);
    let hard_min_tokens = ((reference_tokens as f64 * HARD_MIN_RATIO).floor() as u64)
        .max(CONTEXT_WINDOW_HARD_MIN_TOKENS)
        .min(reference_tokens.max(CONTEXT_WINDOW_HARD_MIN_TOKENS));
    let warn_below_tokens = ((reference_tokens as f64 * WARN_BELOW_RATIO).floor() as u64)
        .max(CONTEXT_WINDOW_WARN_BELOW_TOKENS);
    let required = estimated_input_tokens.saturating_add(requested_output_tokens);
    let should_block = info.tokens < hard_min_tokens || required >= info.tokens;
    let should_warn =
        should_block || info.tokens < warn_below_tokens || required >= info.tokens * 4 / 5;
    let message = if should_block {
        Some(format!(
            "Model context window too small: ctx={} tokens, estimated input={}, requested output={}, hard minimum={}. Set OPENAI_CONTEXT_TOKENS correctly or choose a larger-context model.",
            info.tokens, estimated_input_tokens, requested_output_tokens, hard_min_tokens
        ))
    } else if should_warn {
        Some(format!(
            "Low remaining context: ctx={} tokens, estimated input={}, requested output={}, warn below={}.",
            info.tokens, estimated_input_tokens, requested_output_tokens, warn_below_tokens
        ))
    } else {
        None
    };

    ContextWindowGuard {
        tokens: info.tokens,
        reference_tokens,
        source: info.source,
        hard_min_tokens,
        warn_below_tokens,
        estimated_input_tokens,
        requested_output_tokens,
        should_warn,
        should_block,
        message,
    }
}

pub fn guard_request(
    model_id: &str,
    messages: &[LlmMessage],
    system_prompt: Option<&str>,
    requested_output_tokens: u64,
) -> ContextWindowGuard {
    evaluate_context_window(
        resolve_context_window_info(model_id),
        estimate_request_tokens(messages, system_prompt),
        requested_output_tokens,
    )
}

fn parse_positive_u64(value: &str) -> Option<u64> {
    let parsed = value.trim().parse::<u64>().ok()?;
    (parsed > 0).then_some(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(content: &str) -> LlmMessage {
        LlmMessage {
            role: "user".to_string(),
            content: Some(content.to_string()),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        }
    }

    #[test]
    fn model_metadata_resolves_known_openrouter_model() {
        let info = resolve_context_window_info("deepseek/deepseek-v4-flash");

        assert_eq!(info.tokens, 64_000);
        assert_eq!(info.source, ContextWindowSource::ModelMetadata);
    }

    #[test]
    fn guard_blocks_when_prompt_plus_output_exceeds_window() {
        let guard = evaluate_context_window(
            ContextWindowInfo {
                tokens: 4_096,
                reference_tokens: None,
                source: ContextWindowSource::Env,
            },
            3_800,
            512,
        );

        assert!(guard.should_block);
        assert!(guard
            .message
            .as_deref()
            .is_some_and(|message| message.contains("too small")));
    }

    #[test]
    fn request_estimator_counts_system_prompt() {
        let guard = guard_request(
            "unknown-model",
            &[message(&"风".repeat(300))],
            Some("系统"),
            100,
        );

        assert!(!guard.should_block);
        assert!(guard.estimated_input_tokens >= 100);
    }
}
