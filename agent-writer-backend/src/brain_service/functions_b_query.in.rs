fn snippet_text(text: &str, limit: usize) -> String {
    text.chars().take(limit).collect()
}

fn openai_embedding_model_specs() -> Vec<ProjectBrainEmbeddingModelSpec> {
    vec![
        ProjectBrainEmbeddingModelSpec {
            model: "text-embedding-3-large".to_string(),
            dimensions: 3072,
        },
        ProjectBrainEmbeddingModelSpec {
            model: "text-embedding-3-small".to_string(),
            dimensions: 1536,
        },
        ProjectBrainEmbeddingModelSpec {
            model: "text-embedding-ada-002".to_string(),
            dimensions: 1536,
        },
    ]
}

fn registry_provider_for_api_base<'a>(
    registry: &'a ProjectBrainEmbeddingProviderRegistry,
    api_base: &str,
) -> Option<&'a ProjectBrainEmbeddingProviderSpec> {
    let lower = api_base.to_ascii_lowercase();
    registry.providers.iter().find(|provider| {
        provider
            .api_base_markers
            .iter()
            .any(|marker| lower.contains(marker))
    })
}

fn registry_model_for_name<'a>(
    registry: &'a ProjectBrainEmbeddingProviderRegistry,
    provider: Option<&'a ProjectBrainEmbeddingProviderSpec>,
    model: &str,
) -> Option<&'a ProjectBrainEmbeddingModelSpec> {
    provider
        .and_then(|provider| provider.models.iter().find(|spec| spec.model == model))
        .or_else(|| {
            registry
                .providers
                .iter()
                .flat_map(|provider| provider.models.iter())
                .find(|spec| spec.model == model)
        })
}

fn validate_embedding_dimensions(
    profile: &ProjectBrainEmbeddingProviderProfile,
    embedding: &[f32],
) -> Result<(), String> {
    if embedding.is_empty() {
        return Err("embedding is empty".to_string());
    }
    if profile.dimensions > 0 && embedding.len() != profile.dimensions {
        return Err(format!(
            "expected {} dimensions from {}:{}, got {}",
            profile.dimensions,
            profile.provider_id,
            profile.model,
            embedding.len()
        ));
    }
    Ok(())
}

async fn embed_project_brain_input_with_retry(
    settings: &llm_runtime::LlmSettings,
    profile: &ProjectBrainEmbeddingProviderProfile,
    input: &str,
    timeout_secs: u64,
) -> Result<Vec<f32>, String> {
    let attempts = profile.retry_limit.saturating_add(1).max(1);
    let mut last_error = String::new();
    for attempt in 1..=attempts {
        match llm_runtime::embed(settings, input, timeout_secs).await {
            Ok(embedding) => return Ok(embedding),
            Err(error) => {
                last_error = error;
                if attempt < attempts {
                    tracing::warn!(
                        "Project Brain embedding attempt {}/{} failed for provider={} model={}",
                        attempt,
                        attempts,
                        profile.provider_id,
                        profile.model
                    );
                }
            }
        }
    }

    Err(format!(
        "Project Brain embedding failed after {} attempt(s): {}",
        attempts, last_error
    ))
}

pub fn project_brain_query_provider_budget(
    settings: &llm_runtime::LlmSettings,
    messages: &[serde_json::Value],
) -> WriterProviderBudgetReport {
    project_brain_query_provider_budget_for_model_and_output_tokens(
        settings.model.clone(),
        messages,
        u64::from(
            llm_runtime::request_options(settings, llm_runtime::LlmRequestProfile::ProjectBrainStream)
                .max_tokens,
        ),
    )
}

pub fn project_brain_query_provider_budget_for_model(
    model: impl Into<String>,
    messages: &[serde_json::Value],
) -> WriterProviderBudgetReport {
    project_brain_query_provider_budget_for_model_and_output_tokens(model, messages, 4_096)
}

fn project_brain_query_provider_budget_for_model_and_output_tokens(
    model: impl Into<String>,
    messages: &[serde_json::Value],
    requested_output_tokens: u64,
) -> WriterProviderBudgetReport {
    let converted = messages
        .iter()
        .map(|message| agent_harness_core::provider::LlmMessage {
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
    let estimated_input_tokens =
        agent_harness_core::context_window_guard::estimate_request_tokens(&converted, None);
    evaluate_provider_budget(WriterProviderBudgetRequest::new(
        WriterProviderBudgetTask::ProjectBrainQuery,
        model.into(),
        estimated_input_tokens,
        requested_output_tokens,
    ))
}
