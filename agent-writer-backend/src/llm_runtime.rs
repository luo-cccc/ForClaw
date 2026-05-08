use agent_harness_core::provider::LlmMessage;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};

#[derive(Clone)]
pub struct LlmSettings {
    pub api_key: String,
    pub api_base: String,
    pub model: String,
    pub embedding_model: String,
    pub embedding_input_limit_chars: usize,
    pub chat_temperature: f64,
    pub json_temperature: f64,
    pub chat_max_tokens: u32,
    pub json_max_tokens: u32,
}

pub enum StreamControl {
    Continue,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LlmRequestProfile {
    GeneralChat,
    Json,
    ChapterDraft,
    ChapterContinuation,
    ChapterCompress,
    GhostPreview,
    Analysis,
    ParallelDraft,
    ManualRewrite,
    ToolContinuation,
    ProjectBrainStream,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmRequestOptions {
    pub temperature: f64,
    pub max_tokens: u32,
    pub disable_reasoning: bool,
}

const DEFAULT_CHAT_TEMPERATURE: f64 = 0.7;
const DEFAULT_JSON_TEMPERATURE: f64 = 0.0;
const DEFAULT_CHAT_MAX_TOKENS: u32 = 4_096;
const DEFAULT_JSON_MAX_TOKENS: u32 = 1_024;
const JSON_RETRY_MAX_TOKENS_CAP: u32 = 4_096;

#[derive(Debug, Deserialize)]
struct RequestProfileConfig {
    general_chat: LlmRequestOptions,
    json: LlmRequestOptions,
    chapter_draft: LlmRequestOptions,
    chapter_continuation: LlmRequestOptions,
    chapter_compress: LlmRequestOptions,
    ghost_preview: LlmRequestOptions,
    analysis: LlmRequestOptions,
    parallel_draft: LlmRequestOptions,
    manual_rewrite: LlmRequestOptions,
    tool_continuation: LlmRequestOptions,
    project_brain_stream: LlmRequestOptions,
}

fn request_profile_config() -> &'static RequestProfileConfig {
    static CONFIG: std::sync::OnceLock<RequestProfileConfig> = std::sync::OnceLock::new();
    CONFIG.get_or_init(|| {
        serde_json::from_str(include_str!("../../config/llm-request-profiles.json"))
            .expect("llm-request-profiles.json must be valid")
    })
}

pub fn settings(api_key: String) -> LlmSettings {
    LlmSettings {
        api_key,
        api_base: std::env::var("OPENAI_API_BASE")
            .unwrap_or_else(|_| "https://openrouter.ai/api/v1".to_string()),
        model: std::env::var("OPENAI_MODEL")
            .unwrap_or_else(|_| "deepseek/deepseek-v4-flash".to_string()),
        embedding_model: std::env::var("OPENAI_EMBEDDING_MODEL")
            .unwrap_or_else(|_| "text-embedding-3-small".to_string()),
        embedding_input_limit_chars: std::env::var("OPENAI_EMBEDDING_INPUT_LIMIT_CHARS")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .filter(|value| *value >= 128)
            .unwrap_or(8_000),
        chat_temperature: parse_bounded_f64_env(
            "OPENAI_CHAT_TEMPERATURE",
            DEFAULT_CHAT_TEMPERATURE,
            0.0,
            2.0,
        ),
        json_temperature: parse_bounded_f64_env(
            "OPENAI_JSON_TEMPERATURE",
            DEFAULT_JSON_TEMPERATURE,
            0.0,
            2.0,
        ),
        chat_max_tokens: parse_bounded_u32_env(
            "OPENAI_CHAT_MAX_TOKENS",
            DEFAULT_CHAT_MAX_TOKENS,
            16,
            65_536,
        ),
        json_max_tokens: parse_bounded_u32_env(
            "OPENAI_JSON_MAX_TOKENS",
            DEFAULT_JSON_MAX_TOKENS,
            16,
            65_536,
        ),
    }
}

fn parse_bounded_f64_env(name: &str, default: f64, min: f64, max: f64) -> f64 {
    parse_bounded_f64(std::env::var(name).ok().as_deref(), default, min, max)
}

fn parse_bounded_f64(raw: Option<&str>, default: f64, min: f64, max: f64) -> f64 {
    raw.and_then(|value| value.parse::<f64>().ok())
        .filter(|value| value.is_finite() && *value >= min && *value <= max)
        .unwrap_or(default)
}

fn parse_bounded_u32_env(name: &str, default: u32, min: u32, max: u32) -> u32 {
    parse_bounded_u32(std::env::var(name).ok().as_deref(), default, min, max)
}

fn parse_bounded_u32(raw: Option<&str>, default: u32, min: u32, max: u32) -> u32 {
    raw.and_then(|value| value.parse::<u32>().ok())
        .filter(|value| *value >= min && *value <= max)
        .unwrap_or(default)
}

fn profile_temperature_env(profile: LlmRequestProfile) -> Option<&'static str> {
    match profile {
        LlmRequestProfile::GeneralChat => Some("OPENAI_CHAT_TEMPERATURE"),
        LlmRequestProfile::Json => Some("OPENAI_JSON_TEMPERATURE"),
        LlmRequestProfile::ChapterDraft => Some("OPENAI_CHAPTER_DRAFT_TEMPERATURE"),
        LlmRequestProfile::ChapterContinuation => Some("OPENAI_CHAPTER_CONTINUATION_TEMPERATURE"),
        LlmRequestProfile::ChapterCompress => Some("OPENAI_CHAPTER_COMPRESS_TEMPERATURE"),
        LlmRequestProfile::GhostPreview => Some("OPENAI_GHOST_PREVIEW_TEMPERATURE"),
        LlmRequestProfile::Analysis => Some("OPENAI_ANALYSIS_TEMPERATURE"),
        LlmRequestProfile::ParallelDraft => Some("OPENAI_PARALLEL_DRAFT_TEMPERATURE"),
        LlmRequestProfile::ManualRewrite => Some("OPENAI_MANUAL_REWRITE_TEMPERATURE"),
        LlmRequestProfile::ToolContinuation => Some("OPENAI_TOOL_CONTINUATION_TEMPERATURE"),
        LlmRequestProfile::ProjectBrainStream => Some("OPENAI_PROJECT_BRAIN_TEMPERATURE"),
    }
}

fn profile_max_tokens_env(profile: LlmRequestProfile) -> Option<&'static str> {
    match profile {
        LlmRequestProfile::GeneralChat => Some("OPENAI_CHAT_MAX_TOKENS"),
        LlmRequestProfile::Json => Some("OPENAI_JSON_MAX_TOKENS"),
        LlmRequestProfile::ChapterDraft => Some("OPENAI_CHAPTER_DRAFT_MAX_TOKENS"),
        LlmRequestProfile::ChapterContinuation => Some("OPENAI_CHAPTER_CONTINUATION_MAX_TOKENS"),
        LlmRequestProfile::ChapterCompress => Some("OPENAI_CHAPTER_COMPRESS_MAX_TOKENS"),
        LlmRequestProfile::GhostPreview => Some("OPENAI_GHOST_PREVIEW_MAX_TOKENS"),
        LlmRequestProfile::Analysis => Some("OPENAI_ANALYSIS_MAX_TOKENS"),
        LlmRequestProfile::ParallelDraft => Some("OPENAI_PARALLEL_DRAFT_MAX_TOKENS"),
        LlmRequestProfile::ManualRewrite => Some("OPENAI_MANUAL_REWRITE_MAX_TOKENS"),
        LlmRequestProfile::ToolContinuation => Some("OPENAI_TOOL_CONTINUATION_MAX_TOKENS"),
        LlmRequestProfile::ProjectBrainStream => Some("OPENAI_PROJECT_BRAIN_MAX_TOKENS"),
    }
}

fn profile_reasoning_env(profile: LlmRequestProfile) -> Option<&'static str> {
    match profile {
        LlmRequestProfile::GeneralChat => Some("OPENAI_CHAT_DISABLE_REASONING"),
        LlmRequestProfile::Json => Some("OPENAI_JSON_DISABLE_REASONING"),
        LlmRequestProfile::ChapterDraft => Some("OPENAI_CHAPTER_DRAFT_DISABLE_REASONING"),
        LlmRequestProfile::ChapterContinuation => {
            Some("OPENAI_CHAPTER_CONTINUATION_DISABLE_REASONING")
        }
        LlmRequestProfile::ChapterCompress => Some("OPENAI_CHAPTER_COMPRESS_DISABLE_REASONING"),
        LlmRequestProfile::GhostPreview => Some("OPENAI_GHOST_PREVIEW_DISABLE_REASONING"),
        LlmRequestProfile::Analysis => Some("OPENAI_ANALYSIS_DISABLE_REASONING"),
        LlmRequestProfile::ParallelDraft => Some("OPENAI_PARALLEL_DRAFT_DISABLE_REASONING"),
        LlmRequestProfile::ManualRewrite => Some("OPENAI_MANUAL_REWRITE_DISABLE_REASONING"),
        LlmRequestProfile::ToolContinuation => Some("OPENAI_TOOL_CONTINUATION_DISABLE_REASONING"),
        LlmRequestProfile::ProjectBrainStream => Some("OPENAI_PROJECT_BRAIN_DISABLE_REASONING"),
    }
}

fn parse_bool_env(name: &str, default: bool) -> bool {
    std::env::var(name)
        .ok()
        .and_then(|value| match value.trim().to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" | "on" => Some(true),
            "0" | "false" | "no" | "off" => Some(false),
            _ => None,
        })
        .unwrap_or(default)
}

fn default_profile_options(
    settings: &LlmSettings,
    profile: LlmRequestProfile,
) -> LlmRequestOptions {
    let config = request_profile_config();
    match profile {
        LlmRequestProfile::GeneralChat => LlmRequestOptions {
            temperature: settings.chat_temperature,
            max_tokens: settings.chat_max_tokens,
            disable_reasoning: config.general_chat.disable_reasoning,
        },
        LlmRequestProfile::Json => LlmRequestOptions {
            temperature: settings.json_temperature,
            max_tokens: settings.json_max_tokens,
            disable_reasoning: config.json.disable_reasoning,
        },
        LlmRequestProfile::ChapterDraft => config.chapter_draft,
        LlmRequestProfile::ChapterContinuation => config.chapter_continuation,
        LlmRequestProfile::ChapterCompress => config.chapter_compress,
        LlmRequestProfile::GhostPreview => config.ghost_preview,
        LlmRequestProfile::Analysis => config.analysis,
        LlmRequestProfile::ParallelDraft => config.parallel_draft,
        LlmRequestProfile::ManualRewrite => config.manual_rewrite,
        LlmRequestProfile::ToolContinuation => config.tool_continuation,
        LlmRequestProfile::ProjectBrainStream => config.project_brain_stream,
    }
}

pub fn request_options(settings: &LlmSettings, profile: LlmRequestProfile) -> LlmRequestOptions {
    let defaults = default_profile_options(settings, profile);
    LlmRequestOptions {
        temperature: profile_temperature_env(profile)
            .map(|name| parse_bounded_f64_env(name, defaults.temperature, 0.0, 2.0))
            .unwrap_or(defaults.temperature),
        max_tokens: profile_max_tokens_env(profile)
            .map(|name| parse_bounded_u32_env(name, defaults.max_tokens, 16, 65_536))
            .unwrap_or(defaults.max_tokens),
        disable_reasoning: profile_reasoning_env(profile)
            .map(|name| parse_bool_env(name, defaults.disable_reasoning))
            .unwrap_or(defaults.disable_reasoning),
    }
}

fn endpoint(api_base: &str, path: &str) -> String {
    format!(
        "{}/{}",
        api_base.trim_end_matches('/'),
        path.trim_start_matches('/')
    )
}

pub fn client(timeout_secs: u64) -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(timeout_secs))
        .build()
        .map_err(|e| format!("Failed to build HTTP client: {}", e))
}

fn estimate_json_message_tokens(messages: &[serde_json::Value]) -> u64 {
    let converted = messages
        .iter()
        .map(|message| LlmMessage {
            role: message
                .get("role")
                .and_then(|value| value.as_str())
                .unwrap_or("user")
                .to_string(),
            content: message
                .get("content")
                .and_then(|value| value.as_str())
                .map(str::to_string),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        })
        .collect::<Vec<_>>();
    agent_harness_core::context_window_guard::estimate_request_tokens(&converted, None)
}

fn guard_chat_request(
    settings: &LlmSettings,
    messages: &[serde_json::Value],
    requested_output_tokens: u64,
) -> Result<(), String> {
    guard_chat_request_with_info(
        agent_harness_core::resolve_context_window_info(&settings.model),
        messages,
        requested_output_tokens,
    )
}

fn guard_chat_request_with_info(
    info: agent_harness_core::ContextWindowInfo,
    messages: &[serde_json::Value],
    requested_output_tokens: u64,
) -> Result<(), String> {
    let guard = agent_harness_core::evaluate_context_window(
        info,
        estimate_json_message_tokens(messages),
        requested_output_tokens,
    );
    if guard.should_block {
        Err(guard
            .message
            .unwrap_or_else(|| "Model context window too small".to_string()))
    } else {
        if let Some(message) = guard.message.filter(|_| guard.should_warn) {
            tracing::warn!("{}", message);
        }
        Ok(())
    }
}

fn redact_api_error_body(text: &str) -> String {
    let mut redacted = text.to_string();
    for marker in ["sk-", "Bearer "] {
        while let Some(start) = redacted.find(marker) {
            let token_end = redacted[start..]
                .find(|ch: char| ch.is_whitespace() || ch == '"' || ch == '\'' || ch == ',')
                .map(|offset| start + offset)
                .unwrap_or(redacted.len());
            redacted.replace_range(start..token_end, "[REDACTED]");
        }
    }
    redacted
}

fn openrouter_reasoning_controls_supported(settings: &LlmSettings) -> bool {
    settings
        .api_base
        .to_ascii_lowercase()
        .contains("openrouter.ai")
}

fn apply_provider_options(
    settings: &LlmSettings,
    payload: &mut serde_json::Value,
    options: LlmRequestOptions,
) {
    if options.disable_reasoning && openrouter_reasoning_controls_supported(settings) {
        payload["reasoning"] = serde_json::json!({
            "effort": "none",
            "exclude": true
        });
    }
}

fn chat_request_options(settings: &LlmSettings, json_mode: bool) -> LlmRequestOptions {
    if json_mode {
        request_options(settings, LlmRequestProfile::Json)
    } else {
        request_options(settings, LlmRequestProfile::GeneralChat)
    }
}

fn json_retry_options(first: LlmRequestOptions) -> Option<LlmRequestOptions> {
    let retry_max_tokens = first
        .max_tokens
        .saturating_mul(2)
        .max(DEFAULT_JSON_MAX_TOKENS)
        .min(JSON_RETRY_MAX_TOKENS_CAP);
    if retry_max_tokens <= first.max_tokens && first.temperature <= 0.0 {
        return None;
    }
    Some(LlmRequestOptions {
        temperature: 0.0,
        max_tokens: retry_max_tokens,
        disable_reasoning: true,
    })
}

pub async fn chat_text(
    settings: &LlmSettings,
    messages: Vec<serde_json::Value>,
    json_mode: bool,
    timeout_secs: u64,
) -> Result<String, String> {
    let options = chat_request_options(settings, json_mode);
    chat_text_with_options(settings, messages, json_mode, timeout_secs, options).await
}

pub async fn chat_text_profile(
    settings: &LlmSettings,
    messages: Vec<serde_json::Value>,
    profile: LlmRequestProfile,
    timeout_secs: u64,
) -> Result<String, String> {
    let json_mode = profile == LlmRequestProfile::Json;
    chat_text_with_options(
        settings,
        messages,
        json_mode,
        timeout_secs,
        request_options(settings, profile),
    )
    .await
}

async fn chat_text_with_options(
    settings: &LlmSettings,
    messages: Vec<serde_json::Value>,
    json_mode: bool,
    timeout_secs: u64,
    options: LlmRequestOptions,
) -> Result<String, String> {
    guard_chat_request(settings, &messages, u64::from(options.max_tokens))?;
    let client = client(timeout_secs)?;
    let mut payload = serde_json::json!({
        "model": settings.model,
        "messages": messages,
        "stream": false,
        "temperature": options.temperature,
        "max_tokens": options.max_tokens
    });

    if json_mode {
        payload["response_format"] = serde_json::json!({"type": "json_object"});
    }
    apply_provider_options(settings, &mut payload, options);

    let resp = client
        .post(endpoint(&settings.api_base, "chat/completions"))
        .header("Authorization", format!("Bearer {}", settings.api_key))
        .header("Content-Type", "application/json")
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(format!(
            "API error {}: {}",
            status.as_u16(),
            redact_api_error_body(&text)
        ));
    }

    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("JSON parse: {}", e))?;
    Ok(body["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("")
        .to_string())
}

#[derive(Deserialize)]
struct EmbeddingData {
    embedding: Vec<f32>,
}

#[derive(Deserialize)]
struct EmbeddingResponse {
    data: Vec<EmbeddingData>,
}

pub async fn embed(
    settings: &LlmSettings,
    input: &str,
    timeout_secs: u64,
) -> Result<Vec<f32>, String> {
    let client = client(timeout_secs)?;
    let resp = client
        .post(endpoint(&settings.api_base, "embeddings"))
        .header("Authorization", format!("Bearer {}", settings.api_key))
        .header("Content-Type", "application/json")
        .json(&serde_json::json!({
            "model": settings.embedding_model,
            "input": input
        }))
        .send()
        .await
        .map_err(|e| format!("Embed request failed: {}", e))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(format!(
            "Embed API error {}: {}",
            status.as_u16(),
            redact_api_error_body(&text)
        ));
    }

    let body: EmbeddingResponse = resp
        .json()
        .await
        .map_err(|e| format!("Embedding JSON parse: {}", e))?;

    body.data
        .into_iter()
        .next()
        .map(|data| data.embedding)
        .filter(|embedding| !embedding.is_empty())
        .ok_or_else(|| "Missing embedding in response".to_string())
}

pub async fn chat_json(
    settings: &LlmSettings,
    messages: Vec<serde_json::Value>,
    timeout_secs: u64,
) -> Result<serde_json::Value, String> {
    let options = chat_request_options(settings, true);
    let text = chat_text(settings, messages.clone(), true, timeout_secs).await?;
    match serde_json::from_str(&text) {
        Ok(value) => Ok(value),
        Err(first_error) => {
            let Some(retry_options) = json_retry_options(options) else {
                return Err(format!("Failed to parse JSON response: {}", first_error));
            };
            tracing::warn!(
                "JSON provider response failed to parse; retrying with max_tokens={}",
                retry_options.max_tokens
            );
            let retry_text =
                chat_text_with_options(settings, messages, true, timeout_secs, retry_options)
                    .await?;
            serde_json::from_str(&retry_text).map_err(|retry_error| {
                format!(
                    "Failed to parse JSON response after retry: first={}, retry={}",
                    first_error, retry_error
                )
            })
        }
    }
}

pub async fn stream_chat_profile(
    settings: &LlmSettings,
    messages: Vec<serde_json::Value>,
    profile: LlmRequestProfile,
    timeout_secs: u64,
    mut on_delta: impl FnMut(String) -> Result<StreamControl, String>,
) -> Result<String, String> {
    let options = request_options(settings, profile);
    guard_chat_request(settings, &messages, u64::from(options.max_tokens))?;
    let client = client(timeout_secs)?;
    let mut payload = serde_json::json!({
        "model": settings.model,
        "messages": messages,
        "stream": true,
        "temperature": options.temperature,
        "max_tokens": options.max_tokens
    });
    apply_provider_options(settings, &mut payload, options);

    let resp = client
        .post(endpoint(&settings.api_base, "chat/completions"))
        .header("Authorization", format!("Bearer {}", settings.api_key))
        .header("Content-Type", "application/json")
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(format!(
            "API error {}: {}",
            status.as_u16(),
            redact_api_error_body(&text)
        ));
    }

    let mut stream = resp.bytes_stream();
    let mut sse_buffer = String::new();
    let mut full = String::new();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("Stream error: {}", e))?;
        let text = String::from_utf8_lossy(&chunk);
        sse_buffer.push_str(&text);

        while let Some(line_end) = sse_buffer.find('\n') {
            let line = sse_buffer[..line_end].trim().to_string();
            sse_buffer = sse_buffer[line_end + 1..].to_string();
            if line.is_empty() {
                continue;
            }
            let data = if let Some(d) = line.strip_prefix("data: ") {
                d
            } else {
                continue;
            };
            if data == "[DONE]" {
                continue;
            }
            let parsed: serde_json::Value =
                serde_json::from_str(data).map_err(|e| format!("JSON parse error: {}", e))?;
            let content = parsed["choices"][0]["delta"]["content"]
                .as_str()
                .unwrap_or("")
                .to_string();
            if content.is_empty() {
                continue;
            }

            full.push_str(&content);
            on_delta(content)?;
        }
    }

    Ok(full)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_settings() -> LlmSettings {
        LlmSettings {
            api_key: "sk-test".to_string(),
            api_base: "https://openrouter.ai/api/v1".to_string(),
            model: "deepseek/deepseek-v4-flash".to_string(),
            embedding_model: "text-embedding-3-small".to_string(),
            embedding_input_limit_chars: 8_000,
            chat_temperature: DEFAULT_CHAT_TEMPERATURE,
            json_temperature: DEFAULT_JSON_TEMPERATURE,
            chat_max_tokens: DEFAULT_CHAT_MAX_TOKENS,
            json_max_tokens: DEFAULT_JSON_MAX_TOKENS,
        }
    }

    #[test]
    fn guard_allows_default_openrouter_model_for_small_prompt() {
        let messages = vec![serde_json::json!({
            "role": "user",
            "content": "林墨拔出寒影刀。"
        })];

        assert!(guard_chat_request(&test_settings(), &messages, 512).is_ok());
    }

    #[test]
    fn guard_blocks_when_prompt_exceeds_configured_context() {
        let messages = vec![serde_json::json!({
            "role": "user",
            "content": "风".repeat(12_000)
        })];
        let result = guard_chat_request_with_info(
            agent_harness_core::context_window_guard::ContextWindowInfo {
                tokens: 4_096,
                reference_tokens: None,
                source: agent_harness_core::context_window_guard::ContextWindowSource::Env,
            },
            &messages,
            512,
        );

        assert!(result.is_err());
        assert!(result
            .err()
            .as_deref()
            .is_some_and(|message| message.contains("too small")));
    }

    #[test]
    fn bounded_float_env_parser_rejects_invalid_values() {
        assert_eq!(parse_bounded_f64(Some("0.3"), 0.7, 0.0, 2.0), 0.3);
        assert_eq!(parse_bounded_f64(Some("2.5"), 0.7, 0.0, 2.0), 0.7);
        assert_eq!(parse_bounded_f64(Some("NaN"), 0.7, 0.0, 2.0), 0.7);
        assert_eq!(parse_bounded_f64(Some("bad"), 0.7, 0.0, 2.0), 0.7);
    }

    #[test]
    fn bounded_token_env_parser_rejects_invalid_values() {
        assert_eq!(parse_bounded_u32(Some("2048"), 4096, 16, 65_536), 2048);
        assert_eq!(parse_bounded_u32(Some("4"), 4096, 16, 65_536), 4096);
        assert_eq!(parse_bounded_u32(Some("100000"), 4096, 16, 65_536), 4096);
        assert_eq!(parse_bounded_u32(Some("bad"), 4096, 16, 65_536), 4096);
    }

    #[test]
    fn chat_request_options_use_json_specific_defaults() {
        let settings = test_settings();

        assert_eq!(
            chat_request_options(&settings, false),
            LlmRequestOptions {
                temperature: DEFAULT_CHAT_TEMPERATURE,
                max_tokens: DEFAULT_CHAT_MAX_TOKENS,
                disable_reasoning: false
            }
        );
        assert_eq!(
            chat_request_options(&settings, true),
            LlmRequestOptions {
                temperature: DEFAULT_JSON_TEMPERATURE,
                max_tokens: DEFAULT_JSON_MAX_TOKENS,
                disable_reasoning: true
            }
        );
    }

    #[test]
    fn profile_request_options_use_feature_specific_defaults() {
        let settings = test_settings();
        let config = request_profile_config();

        assert_eq!(
            request_options(&settings, LlmRequestProfile::ChapterDraft),
            LlmRequestOptions {
                temperature: config.chapter_draft.temperature,
                max_tokens: config.chapter_draft.max_tokens,
                disable_reasoning: true
            }
        );
        assert_eq!(
            request_options(&settings, LlmRequestProfile::ChapterContinuation),
            LlmRequestOptions {
                temperature: config.chapter_continuation.temperature,
                max_tokens: config.chapter_continuation.max_tokens,
                disable_reasoning: true
            }
        );
        assert_eq!(
            request_options(&settings, LlmRequestProfile::ChapterCompress),
            LlmRequestOptions {
                temperature: config.chapter_compress.temperature,
                max_tokens: config.chapter_compress.max_tokens,
                disable_reasoning: true
            }
        );
        assert_eq!(
            request_options(&settings, LlmRequestProfile::GhostPreview),
            LlmRequestOptions {
                temperature: config.ghost_preview.temperature,
                max_tokens: config.ghost_preview.max_tokens,
                disable_reasoning: true
            }
        );
        assert_eq!(
            request_options(&settings, LlmRequestProfile::ProjectBrainStream),
            LlmRequestOptions {
                temperature: config.project_brain_stream.temperature,
                max_tokens: config.project_brain_stream.max_tokens,
                disable_reasoning: true
            }
        );
        assert!(request_options(&settings, LlmRequestProfile::ParallelDraft).disable_reasoning);
    }

    #[test]
    fn json_retry_options_expand_small_budget_and_lower_temperature() {
        let retry = json_retry_options(LlmRequestOptions {
            temperature: 0.1,
            max_tokens: 220,
            disable_reasoning: true,
        })
        .expect("small JSON budget should retry");

        assert_eq!(retry.temperature, 0.0);
        assert_eq!(retry.max_tokens, DEFAULT_JSON_MAX_TOKENS);
    }

    #[test]
    fn openrouter_reasoning_controls_are_provider_scoped() {
        let settings = test_settings();
        let options = LlmRequestOptions {
            temperature: 0.0,
            max_tokens: 512,
            disable_reasoning: true,
        };
        let mut payload = serde_json::json!({});

        apply_provider_options(&settings, &mut payload, options);

        assert_eq!(payload["reasoning"]["effort"], "none");
        assert_eq!(payload["reasoning"]["exclude"], true);

        let openai_settings = LlmSettings {
            api_base: "https://api.openai.com/v1".to_string(),
            ..settings
        };
        let mut payload = serde_json::json!({});
        apply_provider_options(&openai_settings, &mut payload, options);

        assert!(payload.get("reasoning").is_none());
    }

    #[test]
    fn redacts_likely_api_keys_from_error_text() {
        let text = redact_api_error_body("invalid key sk-live-secret and Bearer sk-other");

        assert!(!text.contains("sk-live-secret"));
        assert!(!text.contains("sk-other"));
        assert!(text.contains("[REDACTED]"));
    }
}
