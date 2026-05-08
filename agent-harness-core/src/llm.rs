use crate::config::HarnessConfig;
use futures_util::StreamExt;

/// 通用 LLM 客户端 — 支持流式聊天、嵌入、非流式 JSON 输出
#[derive(Clone)]
pub struct LLMClient {
    pub api_key: String,
    pub api_base: String,
    pub model: String,
    pub config: HarnessConfig,
}

impl LLMClient {
    pub fn from_env(config: HarnessConfig) -> Result<Self, String> {
        Ok(Self {
            api_key: std::env::var("OPENAI_API_KEY")
                .map_err(|_| "OPENAI_API_KEY not set".to_string())?,
            api_base: std::env::var("OPENAI_API_BASE")
                .unwrap_or_else(|_| "https://api.openai.com/v1".to_string()),
            model: std::env::var("OPENAI_MODEL").unwrap_or_else(|_| "gpt-4o-mini".to_string()),
            config,
        })
    }

    fn client(&self) -> Result<reqwest::Client, String> {
        reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(
                self.config.request_timeout_secs,
            ))
            .build()
            .map_err(|e| format!("Failed to build client: {}", e))
    }

    /// SSE 流式聊天 — 对每个 delta 调用 on_chunk
    pub async fn chat_stream(
        &self,
        messages: &[serde_json::Value],
        mut on_chunk: impl FnMut(String),
    ) -> Result<String, String> {
        let client = self.client()?;
        let resp = client
            .post(format!("{}/chat/completions", self.api_base))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&serde_json::json!({
                "model": self.model,
                "messages": messages,
                "stream": true
            }))
            .send()
            .await
            .map_err(|e| format!("Request failed: {}", e))?;

        if !resp.status().is_success() {
            return Err(format!("API error: {}", resp.status()));
        }

        let mut stream = resp.bytes_stream();
        let mut sse_buf = String::new();
        let mut full = String::new();

        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| format!("Stream error: {}", e))?;
            let text = String::from_utf8_lossy(&chunk);
            sse_buf.push_str(&text);

            while let Some(line_end) = sse_buf.find('\n') {
                let line = sse_buf[..line_end].trim().to_string();
                sse_buf = sse_buf[line_end + 1..].to_string();
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
                let parsed: serde_json::Value = serde_json::from_str(data).unwrap_or_default();
                let content = parsed["choices"][0]["delta"]["content"]
                    .as_str()
                    .unwrap_or("")
                    .to_string();
                if !content.is_empty() {
                    full.push_str(&content);
                    on_chunk(content);
                }
            }
        }

        Ok(full)
    }

    /// 非流式 JSON 输出
    pub async fn chat_json(
        &self,
        messages: &[serde_json::Value],
    ) -> Result<serde_json::Value, String> {
        let client = self.client()?;
        let resp = client
            .post(format!("{}/chat/completions", self.api_base))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&serde_json::json!({
                "model": self.model,
                "messages": messages,
                "stream": false,
                "response_format": {"type": "json_object"}
            }))
            .send()
            .await
            .map_err(|e| format!("Request failed: {}", e))?;

        if !resp.status().is_success() {
            return Err(format!("API error: {}", resp.status()));
        }

        let body: serde_json::Value = resp.json().await.map_err(|e| format!("JSON: {}", e))?;
        let text = body["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("");
        serde_json::from_str(text).map_err(|e| format!("Parse: {}", e))
    }

    /// 文本嵌入
    pub async fn embed(&self, text: &str) -> Result<Vec<f32>, String> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| format!("Client: {}", e))?;

        let resp = client
            .post(format!("{}/embeddings", self.api_base))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&serde_json::json!({
                "model": "text-embedding-3-small",
                "input": text
            }))
            .send()
            .await
            .map_err(|e| format!("Embed: {}", e))?;

        let body: serde_json::Value = resp.json().await.map_err(|e| format!("JSON: {}", e))?;
        body["data"][0]["embedding"]
            .as_array()
            .ok_or("Missing embedding".to_string())?
            .iter()
            .map(|v| Ok(v.as_f64().unwrap_or(0.0) as f32))
            .collect()
    }
}
