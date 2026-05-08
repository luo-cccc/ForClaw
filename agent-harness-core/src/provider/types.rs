use serde::{Deserialize, Serialize};

/// A message in the conversation — provider-agnostic format.
/// Mirrors Claw Code's ConversationMessage + OpenCode's message-v2 schema.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmMessage {
    pub role: String,
    pub content: Option<String>,
    pub tool_calls: Option<Vec<ToolCall>>,
    pub tool_call_id: Option<String>,
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub call_type: String,
    pub function: ToolCallFunction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallFunction {
    pub name: String,
    pub arguments: String,
}

/// Events emitted during an LLM stream.
/// Unified representation across providers.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum StreamEvent {
    #[serde(rename = "text_delta")]
    TextDelta { content: String },
    #[serde(rename = "tool_call_delta")]
    ToolCallDelta {
        id: String,
        name: String,
        arguments_delta: String,
    },
    #[serde(rename = "tool_call_end")]
    ToolCallEnd {
        id: String,
        name: String,
        arguments: String,
    },
    #[serde(rename = "message_stop")]
    MessageStop { finish_reason: String },
    #[serde(rename = "error")]
    Error { message: String, retryable: bool },
}

/// Result of an LLM call.
#[derive(Debug, Clone)]
pub struct LlmResponse {
    pub content: Option<String>,
    pub tool_calls: Option<Vec<ToolCall>>,
    pub finish_reason: String,
    pub usage: Option<UsageInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageInfo {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cached_tokens: Option<u64>,
}

/// Configuration for an LLM call.
#[derive(Debug, Clone)]
pub struct LlmRequest {
    pub messages: Vec<LlmMessage>,
    pub tools: Option<Vec<serde_json::Value>>,
    pub temperature: Option<f64>,
    pub max_tokens: Option<u32>,
    pub system: Option<String>,
    pub stream: bool,
}
