pub fn cross_reference_project_brain_nodes(
    app: &tauri::AppHandle,
    source_node_id: &str,
    target_node_id: &str,
) -> Result<ProjectBrainCrossReferenceResult, String> {
    let source_node_id = source_node_id.trim();
    let target_node_id = target_node_id.trim();
    if source_node_id.is_empty() || target_node_id.is_empty() {
        return Err("Both source and target node IDs are required for cross-reference".to_string());
    }
    if source_node_id == target_node_id {
        return Err("Cannot cross-reference a node with itself".to_string());
    }

    let index = load_project_brain_knowledge_index(app)?;
    let source = index
        .nodes
        .iter()
        .find(|node| node.id == source_node_id)
        .ok_or_else(|| {
            format!(
                "Source node '{}' not found in knowledge index",
                source_node_id
            )
        })?;
    let target = index
        .nodes
        .iter()
        .find(|node| node.id == target_node_id)
        .ok_or_else(|| {
            format!(
                "Target node '{}' not found in knowledge index",
                target_node_id
            )
        })?;

    let shared_keywords: Vec<String> = source
        .keywords
        .iter()
        .filter(|keyword| target.keywords.contains(keyword))
        .cloned()
        .collect();

    let confidence = if shared_keywords.len() >= 3 {
        0.85
    } else if !shared_keywords.is_empty() {
        0.55
    } else {
        0.25
    };

    let relation = if source.kind == target.kind {
        "extends"
    } else if shared_keywords.len() >= 2 {
        "supports"
    } else if shared_keywords.is_empty() {
        "references"
    } else {
        "relates_to"
    };

    let suggested_action = if confidence >= 0.7 {
        format!(
            "Strong cross-reference: '{}' {} '{}' ({} shared keywords). Consider linking in context.",
            source.label,
            relation,
            target.label,
            shared_keywords.len()
        )
    } else if confidence >= 0.4 {
        format!(
            "Weak cross-reference: '{}' {} '{}'. Review connection before linking.",
            source.label, relation, target.label
        )
    } else {
        format!(
            "Minimal cross-reference: '{}' and '{}' share little evidence. Manual review recommended.",
            source.label, target.label
        )
    };

    let reference = ProjectBrainCrossReference {
        source_node_id: source_node_id.to_string(),
        target_node_id: target_node_id.to_string(),
        relation: relation.to_string(),
        confidence,
        evidence_keywords: shared_keywords.clone(),
        created_at_ms: crate::agent_runtime::now_ms(),
    };

    Ok(ProjectBrainCrossReferenceResult {
        reference,
        source_label: source.label.clone(),
        target_label: target.label.clone(),
        shared_keywords,
        suggested_action,
    })
}

pub fn ingest_external_research_source(
    app: &tauri::AppHandle,
    provider: &str,
    url_or_path: &str,
    title: &str,
    content: &str,
    author_approved: bool,
    approval_reason: &str,
) -> Result<ExternalResearchIngestResult, String> {
    let provider = provider.trim();
    let url_or_path = url_or_path.trim();
    let title = title.trim();
    let content = content.trim();
    let approval_reason = approval_reason.trim();

    if provider.is_empty() || title.is_empty() || content.is_empty() {
        return Err("External research provider, title, and content are all required".to_string());
    }
    validate_external_research_ingest_approval(author_approved, approval_reason)?;

    let revision = crate::storage::content_revision(content);
    let source_ref = format!("external:{}:{}", provider, revision);
    let source_kind = "external_research";

    let content_char_count = content.chars().count();
    let snippet_chars = content_char_count.min(480);
    let content_snippet: String = content.chars().take(snippet_chars).collect();

    let source = ExternalResearchSource {
        provider: provider.to_string(),
        url_or_path: url_or_path.to_string(),
        title: title.to_string(),
        content_snippet,
        relevance_score: 0.7,
        source_kind: source_kind.to_string(),
        ingestion_mode: "manual_author_approved".to_string(),
        author_approved,
    };

    let brain_path = storage::brain_path(app)?;
    let mut brain = VectorDB::load(&brain_path).map_err(|error| {
        format!(
            "Project Brain index at '{}' is unreadable: {}",
            brain_path.display(),
            error
        )
    })?;

    let chunks: Vec<String> = content
        .split("\n\n")
        .filter(|p| !p.trim().is_empty())
        .map(|p| p.trim().to_string())
        .collect();
    let chunk_count = chunks.len();
    let mut node_ids = Vec::new();

    for (idx, chunk_text) in chunks.iter().enumerate() {
        let node_id = format!("{}:chunk:{}", source_ref, idx);
        let keywords: Vec<String> = chunk_text
            .split_whitespace()
            .filter(|w| w.chars().count() >= 2)
            .take(8)
            .map(|w| w.to_string())
            .collect();
        brain.chunks.push(Chunk {
            id: node_id.clone(),
            chapter: title.to_string(),
            text: chunk_text.clone(),
            embedding: Vec::new(),
            keywords: keywords.clone(),
            topic: None,
            source_ref: Some(source_ref.clone()),
            source_revision: Some(revision.clone()),
            source_kind: Some(source_kind.to_string()),
            chunk_index: Some(idx),
            archived: false,
        });

        node_ids.push(node_id);
    }

    let json = serde_json::to_string_pretty(&brain.chunks).map_err(|e| e.to_string())?;
    storage::atomic_write(&brain_path, &json)?;
    rebuild_project_brain_knowledge_index(app)?;

    let evidence_refs = node_ids
        .iter()
        .map(|id| format!("knowledge_node:{}", id))
        .collect();

    let result = ExternalResearchIngestResult {
        source,
        chunk_count,
        node_ids,
        evidence_refs,
        created_at_ms: crate::agent_runtime::now_ms(),
    };

    Ok(result)
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
