pub mod openai_compat;
pub mod types;

use std::sync::Arc;

pub use types::*;

/// Trait for LLM providers. Generic over the streaming implementation.
/// Mirrors Claw Code's ProviderClient enum + OpenCode's Provider interface.
#[async_trait::async_trait]
pub trait Provider: Send + Sync {
    /// Unique provider identifier.
    fn name(&self) -> &str;

    /// List available models for this provider.
    fn models(&self) -> Vec<String>;

    /// Call the LLM with streaming, yielding StreamEvents via callback.
    /// The callback approach gives the caller control over cancellation and backpressure.
    async fn stream_call(
        &self,
        request: LlmRequest,
        on_event: Box<dyn Fn(StreamEvent) + Send + Sync>,
    ) -> Result<LlmResponse, String>;

    /// Non-streaming call — returns complete response.
    async fn call(&self, request: LlmRequest) -> Result<LlmResponse, String>;

    /// Get embeddings for a text.
    async fn embed(&self, text: &str) -> Result<Vec<f32>, String>;

    /// Estimate token count for messages. Provider-specific heuristic.
    fn estimate_tokens(&self, messages: &[LlmMessage]) -> u64;

    /// Model context window metadata used for preflight guards.
    fn context_window_tokens(&self) -> u64 {
        crate::context_window_guard::DEFAULT_CONTEXT_WINDOW_TOKENS
    }

    /// Check connectivity and API key validity.
    async fn health_check(&self) -> Result<(), String>;
}

/// Provider registry — holds all configured providers.
/// Mirrors OpenCode's provider discovery system.
pub struct ProviderRegistry {
    providers: Vec<Arc<dyn Provider>>,
    default_model: String,
}

impl ProviderRegistry {
    pub fn new() -> Self {
        Self {
            providers: Vec::new(),
            default_model: String::new(),
        }
    }

    pub fn register(&mut self, provider: Arc<dyn Provider>) {
        self.providers.push(provider);
    }

    pub fn set_default_model(&mut self, model: &str) {
        self.default_model = model.to_string();
    }

    /// Find a provider that supports the given model.
    /// Falls back to the first registered provider.
    pub fn resolve(&self, model: &str) -> Option<Arc<dyn Provider>> {
        for p in &self.providers {
            if p.models().iter().any(|m| m == model) {
                return Some(p.clone());
            }
        }
        self.providers.first().cloned()
    }

    /// Get the default provider.
    pub fn default(&self) -> Option<Arc<dyn Provider>> {
        if self.default_model.is_empty() {
            self.providers.first().cloned()
        } else {
            self.resolve(&self.default_model)
        }
    }
}

impl Default for ProviderRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::openai_compat::OpenAiCompatProvider;

    fn make_provider() -> OpenAiCompatProvider {
        OpenAiCompatProvider::new("https://api.openai.com/v1", "sk-test", "gpt-4o-mini")
    }

    #[test]
    fn test_provider_models() {
        let p = make_provider();
        assert!(p.models().contains(&"gpt-4o-mini".to_string()));
    }

    #[test]
    fn test_registry_resolve() {
        let mut registry = ProviderRegistry::new();
        let p = Arc::new(make_provider());
        registry.register(p);
        registry.set_default_model("gpt-4o-mini");
        let resolved = registry.resolve("gpt-4o-mini");
        assert!(resolved.is_some());
    }

    #[test]
    fn test_registry_fallback_to_first() {
        let mut registry = ProviderRegistry::new();
        registry.register(Arc::new(make_provider()));
        let resolved = registry.resolve("unknown-model");
        assert!(resolved.is_some()); // falls back to first
    }
}
