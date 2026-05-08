use async_trait::async_trait;
use futures_util::StreamExt;

use crate::context_window_guard::{guard_request, resolve_context_window_info};
use crate::retry::{backoff_duration, ErrorClass};

use super::{LlmMessage, LlmRequest, LlmResponse, Provider, StreamEvent, UsageInfo};

/// OpenAI-compatible provider. Works with OpenAI, OpenRouter, DeepSeek, Groq, xAI,
/// and any /v1/chat/completions endpoint.
/// Ported from Claw Code `api/src/providers/openai_compat.rs`.
pub struct OpenAiCompatProvider {
    pub api_base: String,
    pub api_key: String,
    pub model: String,
    pub embedding_model: String,
    client: reqwest::Client,
}

impl OpenAiCompatProvider {
    pub fn new(api_base: &str, api_key: &str, model: &str) -> Self {
        Self {
            api_base: api_base.trim_end_matches('/').to_string(),
            api_key: api_key.to_string(),
            model: model.to_string(),
            embedding_model: "text-embedding-3-small".to_string(),
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(300))
                .build()
                .unwrap_or_default(),
        }
    }

    pub(crate) fn build_api_messages(request: &LlmRequest) -> Vec<serde_json::Value> {
        let mut api_messages = Vec::new();
        if let Some(system) = request
            .system
            .as_ref()
            .filter(|system| !system.trim().is_empty())
        {
            api_messages.push(serde_json::json!({
                "role": "system",
                "content": system,
            }));
        }

        api_messages.extend(request.messages.iter().map(|m| {
            let mut api = serde_json::json!({"role": m.role});
            if let Some(ref c) = m.content {
                api["content"] = serde_json::json!(c);
            }
            if let Some(ref tc) = m.tool_calls {
                api["tool_calls"] = serde_json::json!(tc);
            }
            if let Some(ref cid) = m.tool_call_id {
                api["tool_call_id"] = serde_json::json!(cid);
            }
            if let Some(ref n) = m.name {
                api["name"] = serde_json::json!(n);
            }
            api
        }));
        api_messages
    }

    async fn parse_sse_stream(
        response: reqwest::Response,
        on_event: &(dyn Fn(StreamEvent) + Send + Sync),
    ) -> Result<LlmResponse, String> {
        let mut stream = response.bytes_stream();
        let mut buffer = String::new();
        let mut content = String::new();
        let mut tool_calls_map: std::collections::HashMap<String, (String, String)> =
            std::collections::HashMap::new();
        let mut finish_reason = String::new();
        let mut usage = None;

        while let Some(chunk_result) = stream.next().await {
            let chunk = chunk_result.map_err(|e| format!("Stream read error: {}", e))?;
            let chunk_str = String::from_utf8_lossy(&chunk);
            buffer.push_str(&chunk_str);

            while let Some(nl_pos) = buffer.find('\n') {
                let line = buffer[..nl_pos].trim().to_string();
                buffer = buffer[nl_pos + 1..].to_string();

                if line.is_empty() || line == "data: [DONE]" {
                    continue;
                }

                if let Some(data) = line.strip_prefix("data: ") {
                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(data) {
                        if let Some(choices) = json.get("choices").and_then(|v| v.as_array()) {
                            if let Some(first) = choices.first() {
                                // Text delta
                                if let Some(delta) = first.get("delta") {
                                    if let Some(text) =
                                        delta.get("content").and_then(|v| v.as_str())
                                    {
                                        content.push_str(text);
                                        on_event(StreamEvent::TextDelta {
                                            content: text.to_string(),
                                        });
                                    }
                                    // Tool call deltas
                                    if let Some(tc_deltas) =
                                        delta.get("tool_calls").and_then(|v| v.as_array())
                                    {
                                        for tc_delta in tc_deltas {
                                            let id = tc_delta
                                                .get("id")
                                                .and_then(|v| v.as_str())
                                                .unwrap_or("");
                                            let name = tc_delta
                                                .get("function")
                                                .and_then(|v| v.get("name"))
                                                .and_then(|v| v.as_str())
                                                .unwrap_or("");
                                            let args = tc_delta
                                                .get("function")
                                                .and_then(|v| v.get("arguments"))
                                                .and_then(|v| v.as_str())
                                                .unwrap_or("");

                                            let entry = tool_calls_map
                                                .entry(id.to_string())
                                                .or_insert_with(|| {
                                                    (name.to_string(), String::new())
                                                });
                                            if !name.is_empty() {
                                                entry.0 = name.to_string();
                                            }
                                            entry.1.push_str(args);

                                            on_event(StreamEvent::ToolCallDelta {
                                                id: id.to_string(),
                                                name: entry.0.clone(),
                                                arguments_delta: args.to_string(),
                                            });
                                        }
                                    }
                                }
                                // Finish reason
                                if let Some(fr) =
                                    first.get("finish_reason").and_then(|v| v.as_str())
                                {
                                    finish_reason = fr.to_string();
                                }
                            }
                        }
                        // Usage
                        if let Some(u) = json.get("usage") {
                            let cached = u
                                .get("prompt_tokens_details")
                                .and_then(|d| d.get("cached_tokens"))
                                .and_then(|v| v.as_u64());
                            usage = Some(UsageInfo {
                                input_tokens: u
                                    .get("prompt_tokens")
                                    .and_then(|v| v.as_u64())
                                    .unwrap_or(0),
                                output_tokens: u
                                    .get("completion_tokens")
                                    .and_then(|v| v.as_u64())
                                    .unwrap_or(0),
                                cached_tokens: cached,
                            });
                        }
                    }
                }
            }
        }

        // Emit tool call end events for all accumulated tool calls
        let tool_calls: Vec<super::ToolCall> = tool_calls_map
            .into_iter()
            .map(|(id, (name, args))| {
                on_event(StreamEvent::ToolCallEnd {
                    id: id.clone(),
                    name: name.clone(),
                    arguments: args.clone(),
                });
                super::ToolCall {
                    id,
                    call_type: "function".to_string(),
                    function: super::ToolCallFunction {
                        name,
                        arguments: args,
                    },
                }
            })
            .collect();

        on_event(StreamEvent::MessageStop {
            finish_reason: finish_reason.clone(),
        });

        Ok(LlmResponse {
            content: if content.is_empty() {
                None
            } else {
                Some(content)
            },
            tool_calls: if tool_calls.is_empty() {
                None
            } else {
                Some(tool_calls)
            },
            finish_reason,
            usage,
        })
    }
}

#[async_trait]
impl Provider for OpenAiCompatProvider {
    fn name(&self) -> &str {
        "openai-compat"
    }

    fn models(&self) -> Vec<String> {
        vec![self.model.clone()]
    }

    async fn stream_call(
        &self,
        request: LlmRequest,
        on_event: Box<dyn Fn(StreamEvent) + Send + Sync>,
    ) -> Result<LlmResponse, String> {
        let requested_output_tokens = request.max_tokens.unwrap_or(4096) as u64;
        let guard = guard_request(
            &self.model,
            &request.messages,
            request.system.as_deref(),
            requested_output_tokens,
        );
        if guard.should_block {
            return Err(guard
                .message
                .unwrap_or_else(|| "Model context window too small".to_string()));
        }

        let messages = Self::build_api_messages(&request);

        let mut body = serde_json::json!({
            "model": self.model,
            "messages": messages,
            "stream": true,
            "temperature": request.temperature.unwrap_or(0.7),
        });

        if let Some(ref tools) = request.tools {
            body["tools"] = serde_json::json!(tools);
            body["tool_choice"] = serde_json::json!("auto");
        }
        if let Some(mt) = request.max_tokens {
            body["max_tokens"] = serde_json::json!(mt);
        }

        let mut retry_count = 0u32;
        loop {
            let response = self
                .client
                .post(format!("{}/chat/completions", self.api_base))
                .header("Authorization", format!("Bearer {}", self.api_key))
                .header("Content-Type", "application/json")
                .json(&body)
                .send()
                .await
                .map_err(|e| format!("HTTP request failed: {}", e))?;

            let status = response.status();
            if status.is_success() {
                return Self::parse_sse_stream(response, &*on_event).await;
            }

            let status_code = status.as_u16();
            let body_text = response.text().await.unwrap_or_default();
            let class = ErrorClass::from_http_status_and_body(status_code, &body_text);

            if !class.is_retryable() || retry_count >= 3 {
                return Err(format!("LLM call failed ({}): {}", status_code, body_text));
            }

            let delay = backoff_duration(retry_count);
            tokio::time::sleep(delay).await;
            retry_count += 1;
        }
    }

    async fn call(&self, request: LlmRequest) -> Result<LlmResponse, String> {
        // Non-streaming: delegate to stream_call which aggregates internally.
        self.stream_call(request, Box::new(|_ev| {})).await
    }

    async fn embed(&self, text: &str) -> Result<Vec<f32>, String> {
        let response = self
            .client
            .post(format!("{}/embeddings", self.api_base))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&serde_json::json!({
                "model": self.embedding_model,
                "input": text,
            }))
            .send()
            .await
            .map_err(|e| format!("Embedding request failed: {}", e))?;

        let json: serde_json::Value = response
            .json()
            .await
            .map_err(|e| format!("Failed to parse embedding response: {}", e))?;

        json["data"][0]["embedding"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_f64().map(|f| f as f32))
                    .collect()
            })
            .ok_or_else(|| "Missing embedding in response".to_string())
    }

    fn estimate_tokens(&self, messages: &[LlmMessage]) -> u64 {
        // Rough heuristic: 1 token ≈ 3 chars for CJK-heavy text + 8 token per-message overhead
        messages
            .iter()
            .map(|m| (m.content.as_ref().map(|c| c.chars().count()).unwrap_or(0) as u64) / 3 + 8)
            .sum()
    }

    fn context_window_tokens(&self) -> u64 {
        resolve_context_window_info(&self.model).tokens
    }

    async fn health_check(&self) -> Result<(), String> {
        let response = self
            .client
            .get(format!("{}/models", self.api_base))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .send()
            .await
            .map_err(|e| format!("Health check failed: {}", e))?;

        if response.status().is_success() {
            Ok(())
        } else {
            Err(format!(
                "Health check failed with status: {}",
                response.status()
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_messages_include_system_prompt_before_conversation() {
        let request = LlmRequest {
            messages: vec![LlmMessage {
                role: "user".to_string(),
                content: Some("继续写这一段".to_string()),
                tool_calls: None,
                tool_call_id: None,
                name: None,
            }],
            tools: None,
            temperature: None,
            max_tokens: None,
            system: Some("你是 Forge 的写作 Agent".to_string()),
            stream: true,
        };

        let messages = OpenAiCompatProvider::build_api_messages(&request);

        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0]["role"], "system");
        assert_eq!(messages[0]["content"], "你是 Forge 的写作 Agent");
        assert_eq!(messages[1]["role"], "user");
    }
}
