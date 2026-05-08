pub async fn embed_project_brain_text(
    settings: &llm_runtime::LlmSettings,
    input: &str,
    timeout_secs: u64,
) -> Result<Vec<f32>, String> {
    let profile = project_brain_embedding_profile(settings);
    let (input, _) = trim_embedding_input(input, profile.input_limit_chars);
    if input.trim().is_empty() {
        return Err("Project Brain embedding input is empty".to_string());
    }
    let embedding =
        embed_project_brain_input_with_retry(settings, &profile, &input, timeout_secs).await?;
    validate_embedding_dimensions(&profile, &embedding)?;
    Ok(embedding)
}

pub async fn embed_project_brain_chunks(
    settings: &llm_runtime::LlmSettings,
    chapter_title: &str,
    chunks: &[(String, Vec<String>, Option<String>)],
    timeout_secs: u64,
) -> (Vec<Chunk>, ProjectBrainEmbeddingBatchReport) {
    let source_revision = storage::content_revision(
        &chunks
            .iter()
            .map(|(chunk_text, _, _)| chunk_text.as_str())
            .collect::<Vec<_>>()
            .join("\n\n"),
    );
    let source_ref = format!("chapter:{}", chapter_title);
    let profile = project_brain_embedding_profile(settings);
    let mut embedded_chunks = Vec::new();
    let mut report = ProjectBrainEmbeddingBatchReport {
        profile: profile.clone(),
        requested_count: chunks.len(),
        embedded_count: 0,
        skipped_count: 0,
        truncated_count: 0,
        status: ProjectBrainEmbeddingBatchStatus::Empty,
        errors: Vec::new(),
    };

    for (i, (chunk_text, keywords, topic)) in chunks.iter().enumerate() {
        if chunk_text.trim().chars().count() < MIN_CHUNK_CHARS {
            report.skipped_count += 1;
            continue;
        }
        let (limited_text, truncated) = trim_embedding_input(chunk_text, profile.input_limit_chars);
        if truncated {
            report.truncated_count += 1;
        }

        let embedding = match embed_project_brain_input_with_retry(
            settings,
            &profile,
            &limited_text,
            timeout_secs,
        )
        .await
        {
            Ok(embedding) => embedding,
            Err(error) => {
                report.skipped_count += 1;
                report.errors.push(format!(
                    "{}#{} embed request failed: {}",
                    chapter_title, i, error
                ));
                continue;
            }
        };
        if let Err(error) = validate_embedding_dimensions(&profile, &embedding) {
            report.skipped_count += 1;
            report.errors.push(format!(
                "{}#{} invalid embedding: {}",
                chapter_title, i, error
            ));
            continue;
        }

        embedded_chunks.push(Chunk {
            id: format!("{}-{}-{}", chapter_title, source_revision, i),
            chapter: chapter_title.to_string(),
            text: limited_text,
            embedding,
            keywords: keywords.clone(),
            topic: topic.clone(),
            source_ref: Some(source_ref.clone()),
            source_revision: Some(source_revision.clone()),
            source_kind: Some("chapter".to_string()),
            chunk_index: Some(i),
            archived: false,
        });
        report.embedded_count += 1;
    }

    report.status = project_brain_embedding_batch_status(
        report.requested_count,
        report.embedded_count,
        report.skipped_count,
        &report.errors,
    );

    (embedded_chunks, report)
}

pub fn validate_external_research_ingest_approval(
    author_approved: bool,
    approval_reason: &str,
) -> Result<(), String> {
    if !author_approved {
        return Err(
            "External research ingestion writes to Project Brain and requires explicit author approval"
                .to_string(),
        );
    }
    if approval_reason.trim().is_empty() {
        return Err("External research ingestion requires an author approval reason".to_string());
    }
    Ok(())
}

pub fn project_brain_embedding_profile(
    settings: &llm_runtime::LlmSettings,
) -> ProjectBrainEmbeddingProviderProfile {
    project_brain_embedding_profile_from_config(
        &settings.api_base,
        &settings.embedding_model,
        settings.embedding_input_limit_chars,
    )
}

pub fn project_brain_embedding_profile_from_config(
    api_base: &str,
    embedding_model: &str,
    input_limit_chars: usize,
) -> ProjectBrainEmbeddingProviderProfile {
    resolve_project_brain_embedding_profile(api_base, embedding_model, Some(input_limit_chars))
}

pub fn resolve_project_brain_embedding_profile(
    api_base: &str,
    embedding_model: &str,
    input_limit_chars: Option<usize>,
) -> ProjectBrainEmbeddingProviderProfile {
    let registry = project_brain_embedding_provider_registry();
    let provider_spec = registry_provider_for_api_base(&registry, api_base);
    let model_spec = registry_model_for_name(&registry, provider_spec, embedding_model);
    let provider_status = if provider_spec.is_some() {
        ProjectBrainEmbeddingRegistryStatus::RegistryKnown
    } else {
        ProjectBrainEmbeddingRegistryStatus::CompatibilityFallback
    };
    let model_status = if model_spec.is_some() {
        ProjectBrainEmbeddingRegistryStatus::RegistryKnown
    } else {
        ProjectBrainEmbeddingRegistryStatus::CompatibilityFallback
    };
    let provider_id = provider_spec
        .map(|provider| provider.provider_id.clone())
        .unwrap_or_else(|| registry.fallback_provider_id.clone());
    let dimensions = model_spec
        .map(|model| model.dimensions)
        .unwrap_or(registry.fallback_dimensions);
    let input_limit_chars = input_limit_chars
        .filter(|limit| *limit > 0)
        .unwrap_or_else(|| {
            provider_spec
                .map(|provider| provider.default_input_limit_chars)
                .unwrap_or(registry.fallback_input_limit_chars)
        });
    let batch_limit = provider_spec
        .map(|provider| provider.batch_limit)
        .unwrap_or(registry.fallback_batch_limit);
    let retry_limit = provider_spec
        .map(|provider| provider.retry_limit)
        .unwrap_or(registry.fallback_retry_limit);

    ProjectBrainEmbeddingProviderProfile {
        provider_id,
        model: embedding_model.to_string(),
        dimensions,
        input_limit_chars,
        batch_limit,
        retry_limit,
        provider_status,
        model_status,
    }
}

pub fn project_brain_embedding_provider_registry() -> ProjectBrainEmbeddingProviderRegistry {
    let openai_models = openai_embedding_model_specs();
    ProjectBrainEmbeddingProviderRegistry {
        providers: vec![
            ProjectBrainEmbeddingProviderSpec {
                provider_id: "openai".to_string(),
                api_base_markers: vec!["api.openai.com".to_string()],
                default_input_limit_chars: DEFAULT_EMBEDDING_INPUT_LIMIT_CHARS,
                batch_limit: 16,
                retry_limit: 1,
                models: openai_models.clone(),
            },
            ProjectBrainEmbeddingProviderSpec {
                provider_id: "openrouter".to_string(),
                api_base_markers: vec!["openrouter.ai".to_string()],
                default_input_limit_chars: DEFAULT_EMBEDDING_INPUT_LIMIT_CHARS,
                batch_limit: 16,
                retry_limit: 1,
                models: openai_models.clone(),
            },
            ProjectBrainEmbeddingProviderSpec {
                provider_id: "local-openai-compatible".to_string(),
                api_base_markers: vec![
                    "localhost".to_string(),
                    "127.0.0.1".to_string(),
                    "[::1]".to_string(),
                ],
                default_input_limit_chars: 4_000,
                batch_limit: 8,
                retry_limit: 0,
                models: openai_models,
            },
        ],
        fallback_provider_id: "openai-compatible".to_string(),
        fallback_dimensions: DEFAULT_EMBEDDING_DIMENSIONS,
        fallback_input_limit_chars: DEFAULT_EMBEDDING_INPUT_LIMIT_CHARS,
        fallback_batch_limit: 8,
        fallback_retry_limit: 0,
    }
}

pub fn trim_embedding_input(input: &str, limit: usize) -> (String, bool) {
    let trimmed = input.trim();
    if trimmed.chars().count() <= limit {
        return (trimmed.to_string(), false);
    }
    let mut output = trimmed.chars().take(limit).collect::<String>();
    while output.ends_with(char::is_whitespace) {
        output.pop();
    }
    (output, true)
}
