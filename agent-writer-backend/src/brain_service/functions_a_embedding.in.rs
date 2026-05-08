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

pub async fn embed_chapter(
    app: &tauri::AppHandle,
    settings: &llm_runtime::LlmSettings,
    chapter_title: &str,
    content: &str,
) -> Result<(), String> {
    let chunks = chunk_text(content, CHUNK_MAX_CHARS);
    if chunks.is_empty() {
        return Ok(());
    }

    let (embedded_chunks, report) =
        embed_project_brain_chunks(settings, chapter_title, &chunks, 30).await;
    if !matches!(report.status, ProjectBrainEmbeddingBatchStatus::Complete) {
        tracing::warn!(
            "Project Brain embedding batch for '{}' finished with {:?}: embedded={} skipped={} truncated={} errors={:?}",
            chapter_title,
            report.status,
            report.embedded_count,
            report.skipped_count,
            report.truncated_count,
            report.errors
        );
    }

    if embedded_chunks.is_empty() {
        return Ok(());
    }

    let path = storage::brain_path(app)?;
    let mut db = VectorDB::load(&path).map_err(|e| {
        format!(
            "Project Brain index at '{}' is unreadable; restore a backup or rebuild the index: {}",
            path.display(),
            e
        )
    })?;
    let active_revision = embedded_chunks
        .first()
        .and_then(|chunk| chunk.source_revision.as_deref())
        .unwrap_or_default()
        .to_string();
    db.archive_chapter_revision(chapter_title, &active_revision);
    for chunk in embedded_chunks {
        db.upsert(chunk);
    }

    db.save(&path)
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
