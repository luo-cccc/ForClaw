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

pub async fn answer_query(
    app: &tauri::AppHandle,
    settings: &llm_runtime::LlmSettings,
    query: &str,
    on_delta: impl FnMut(String) -> Result<llm_runtime::StreamControl, String>,
) -> Result<(), String> {
    answer_query_with_focus(
        app,
        settings,
        query,
        &ProjectBrainFocus::from_query(query),
        None,
        on_delta,
    )
    .await
}

pub async fn answer_query_with_focus(
    app: &tauri::AppHandle,
    settings: &llm_runtime::LlmSettings,
    query: &str,
    focus: &ProjectBrainFocus,
    provider_budget_approval: Option<&WriterProviderBudgetApproval>,
    on_delta: impl FnMut(String) -> Result<llm_runtime::StreamControl, String>,
) -> Result<(), String> {
    let search_text = focus.search_text();
    let query_embedding = embed_project_brain_text(settings, &search_text, 30)
        .await
        .map_err(|e| format!("Embed error: {}", e))?;

    let brain_path = storage::brain_path(app)?;
    let db = VectorDB::load(&brain_path).map_err(|e| {
        format!(
            "Project Brain index at '{}' is unreadable; restore a backup or rebuild the index: {}",
            brain_path.display(),
            e
        )
    })?;
    let results = search_project_brain_results_with_focus(&db, focus, &query_embedding);
    let context = build_context(&results);

    let messages = vec![
        serde_json::json!({"role": "system", "content": format!(
            "You are an expert on this novel. Answer the user's question using ONLY the provided book excerpts. \
             If the excerpts don't contain relevant information, say so honestly.\n\nBook excerpts:\n{}",
            context
        )}),
        serde_json::json!({"role": "user", "content": query}),
    ];
    let budget_report = apply_provider_budget_approval(
        project_brain_query_provider_budget(settings, &messages),
        provider_budget_approval,
    );
    let created_at_ms = crate::agent_runtime::now_ms();
    let task_id = format!(
        "project-brain-query-{}",
        storage::content_revision(&format!("{}:{}", query, created_at_ms))
            .split('-')
            .next()
            .unwrap_or("0000000000000000")
    );
    let source_refs = project_brain_query_source_refs(query, &results, &budget_report);
    record_project_brain_provider_budget(
        app,
        &task_id,
        &budget_report,
        source_refs.clone(),
        created_at_ms,
    );
    if budget_report.approval_required {
        record_project_brain_budget_failure(
            app,
            task_id,
            source_refs,
            budget_report.clone(),
            created_at_ms,
        );
        emit_project_brain_provider_budget_error(app, &budget_report);
        return Err("PROJECT_BRAIN_PROVIDER_BUDGET_APPROVAL_REQUIRED".to_string());
    }
    record_project_brain_model_started(
        app,
        &task_id,
        &budget_report,
        source_refs,
        crate::agent_runtime::now_ms(),
    );

    llm_runtime::stream_chat_profile(
        settings,
        messages,
        llm_runtime::LlmRequestProfile::ProjectBrainStream,
        60,
        on_delta,
    )
    .await?;

    Ok(())
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

fn project_brain_query_source_refs(
    query: &str,
    results: &[(f32, Vec<String>, &Chunk)],
    report: &WriterProviderBudgetReport,
) -> Vec<String> {
    let query_hash = storage::content_revision(query)
        .split('-')
        .next()
        .unwrap_or("0000000000000000")
        .to_string();
    let mut refs = vec![
        format!("project_brain_query:{}", query_hash),
        format!("model:{}", report.model),
        format!("estimated_tokens:{}", report.estimated_total_tokens),
        format!("estimated_cost_micros:{}", report.estimated_cost_micros),
    ];
    refs.extend(results.iter().flat_map(|(_, _, chunk)| {
        let mut refs = vec![
            format!("project_brain:{}", chunk.id),
            format!("chapter:{}", chunk.chapter),
        ];
        if let Some(source_ref) = chunk.source_ref.as_deref() {
            refs.push(format!("source_ref:{}", source_ref));
        }
        if let Some(source_revision) = chunk.source_revision.as_deref() {
            refs.push(format!("source_revision:{}", source_revision));
        }
        refs
    }));
    refs
}

fn record_project_brain_provider_budget(
    app: &tauri::AppHandle,
    task_id: &str,
    report: &WriterProviderBudgetReport,
    source_refs: Vec<String>,
    created_at_ms: u64,
) {
    let state = app.state::<crate::AppState>();
    let Ok(mut kernel) = state.writer_kernel.lock() else {
        return;
    };
    kernel.record_provider_budget_report(task_id.to_string(), report, source_refs, created_at_ms);
}

fn record_project_brain_model_started(
    app: &tauri::AppHandle,
    task_id: &str,
    report: &WriterProviderBudgetReport,
    source_refs: Vec<String>,
    created_at_ms: u64,
) {
    let state = app.state::<crate::AppState>();
    let Ok(mut kernel) = state.writer_kernel.lock() else {
        return;
    };
    kernel.record_model_started_run_event(
        ModelStartedEventContext {
            task_id: task_id.to_string(),
            task: report.task,
            model: report.model.clone(),
            provider: "openai-compatible".to_string(),
            stream: true,
        },
        source_refs,
        Some(report),
        created_at_ms,
    );
}

fn record_project_brain_budget_failure(
    app: &tauri::AppHandle,
    task_id: String,
    source_refs: Vec<String>,
    report: WriterProviderBudgetReport,
    created_at_ms: u64,
) {
    let state = app.state::<crate::AppState>();
    let Ok(mut kernel) = state.writer_kernel.lock() else {
        return;
    };
    let bundle = WriterFailureEvidenceBundle::new(
        WriterFailureCategory::ProviderFailed,
        "PROJECT_BRAIN_PROVIDER_BUDGET_APPROVAL_REQUIRED",
        "Project Brain answer provider budget requires explicit approval before calling the model.",
        true,
        Some(task_id),
        source_refs,
        serde_json::json!({ "providerBudget": report }),
        vec![
            "Surface the Project Brain token/cost estimate to the author before retrying."
                .to_string(),
            "Narrow the query or reduce Project Brain context if approval is not granted."
                .to_string(),
        ],
        created_at_ms,
    );
    kernel.record_failure_evidence_bundle(&bundle);
}

fn emit_project_brain_provider_budget_error(
    app: &tauri::AppHandle,
    report: &WriterProviderBudgetReport,
) {
    let _ = app.emit(
        crate::events::AGENT_ERROR,
        serde_json::json!({
            "message": "Project Brain provider budget requires explicit approval before calling the model.",
            "source": "provider_budget",
            "error": {
                "code": "PROJECT_BRAIN_PROVIDER_BUDGET_APPROVAL_REQUIRED",
                "message": "Project Brain provider budget requires explicit approval before calling the model.",
                "recoverable": true,
                "details": {
                    "providerBudget": report,
                },
            },
        }),
    );
}
