pub fn knowledge_index_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    Ok(storage::active_project_data_dir(app)?.join(KNOWLEDGE_INDEX_FILENAME))
}

pub fn rebuild_project_brain_knowledge_index(
    app: &tauri::AppHandle,
) -> Result<ProjectBrainKnowledgeIndex, String> {
    let project_id = storage::active_project_id(app)?;
    let brain_path = storage::brain_path(app)?;
    let brain = VectorDB::load(&brain_path).map_err(|e| {
        format!(
            "Project Brain index at '{}' is unreadable; restore a backup or rebuild the index: {}",
            brain_path.display(),
            e
        )
    })?;
    let outline = storage::load_outline(app)?;
    let lorebook = storage::load_lorebook(app)?;
    let index = build_project_brain_knowledge_index(&project_id, &brain, &outline, &lorebook);
    save_project_brain_knowledge_index(app, &index)?;
    Ok(index)
}

pub fn load_project_brain_knowledge_index(
    app: &tauri::AppHandle,
) -> Result<ProjectBrainKnowledgeIndex, String> {
    let path = knowledge_index_path(app)?;
    if !path.exists() {
        return rebuild_project_brain_knowledge_index(app);
    }
    let data = std::fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read knowledge index '{}': {}", path.display(), e))?;
    serde_json::from_str(&data).map_err(|e| {
        format!(
            "Failed to parse knowledge index '{}': {}",
            path.display(),
            e
        )
    })
}

pub fn save_project_brain_knowledge_index(
    app: &tauri::AppHandle,
    index: &ProjectBrainKnowledgeIndex,
) -> Result<(), String> {
    let path = knowledge_index_path(app)?;
    let json = serde_json::to_string_pretty(index).map_err(|e| e.to_string())?;
    storage::atomic_write(&path, &json)
}

pub fn compare_project_brain_source_revisions(
    app: &tauri::AppHandle,
    source_ref: &str,
) -> Result<ProjectBrainSourceCompare, String> {
    let source_ref = source_ref.trim();
    if source_ref.is_empty() {
        return Err("Project Brain source ref is required for revision compare".to_string());
    }

    let brain_path = storage::brain_path(app)?;
    let brain = VectorDB::load(&brain_path).map_err(|e| {
        format!(
            "Project Brain index at '{}' is unreadable; restore a backup or rebuild the index: {}",
            brain_path.display(),
            e
        )
    })?;
    Ok(compare_project_brain_source_revisions_from_db(
        source_ref, &brain,
    ))
}

pub fn restore_project_brain_source_revision(
    app: &tauri::AppHandle,
    source_ref: &str,
    revision: &str,
) -> Result<ProjectBrainSourceRevisionRestore, String> {
    let source_ref = source_ref.trim();
    let revision = revision.trim();
    if source_ref.is_empty() {
        return Err("Project Brain source ref is required for revision restore".to_string());
    }
    if revision.is_empty() {
        return Err("Project Brain source revision is required for revision restore".to_string());
    }

    let brain_path = storage::brain_path(app)?;
    let mut brain = VectorDB::load(&brain_path).map_err(|e| {
        format!(
            "Project Brain index at '{}' is unreadable; restore a backup or rebuild the index: {}",
            brain_path.display(),
            e
        )
    })?;
    let report = restore_project_brain_source_revision_in_db(source_ref, revision, &mut brain)?;
    let json = serde_json::to_string_pretty(&brain.chunks).map_err(|e| e.to_string())?;
    storage::atomic_write(&brain_path, &json)?;
    rebuild_project_brain_knowledge_index(app)?;
    Ok(report)
}

pub fn restore_project_brain_source_revision_in_db(
    source_ref: &str,
    revision: &str,
    brain: &mut VectorDB,
) -> Result<ProjectBrainSourceRevisionRestore, String> {
    let source_ref = source_ref.trim();
    let revision = revision.trim();
    if source_ref.is_empty() {
        return Err("Project Brain source ref is required for revision restore".to_string());
    }
    if revision.is_empty() {
        return Err("Project Brain source revision is required for revision restore".to_string());
    }

    let mut source_kind = "unknown".to_string();
    let mut previous_active_revisions = BTreeSet::new();
    let mut has_requested_revision = false;
    let mut total_source_chunk_count = 0usize;
    let mut active_chunk_count = 0usize;
    let mut archived_chunk_count = 0usize;
    let mut changed_chunk_count = 0usize;

    for chunk in &brain.chunks {
        if chunk.source_ref.as_deref() != Some(source_ref) {
            continue;
        }
        total_source_chunk_count += 1;
        if let Some(kind) = chunk
            .source_kind
            .as_deref()
            .map(str::trim)
            .filter(|kind| !kind.is_empty())
        {
            source_kind = kind.to_string();
        }
        if !chunk.archived {
            if let Some(active_revision) = chunk
                .source_revision
                .as_deref()
                .map(str::trim)
                .filter(|active_revision| !active_revision.is_empty())
            {
                previous_active_revisions.insert(active_revision.to_string());
            }
        }
        if chunk.source_revision.as_deref().map(str::trim) == Some(revision) {
            has_requested_revision = true;
        }
    }

    if total_source_chunk_count == 0 {
        return Err(format!(
            "Project Brain source '{}' has no indexed chunks to restore",
            source_ref
        ));
    }
    if !has_requested_revision {
        return Err(format!(
            "Project Brain source '{}' has no revision '{}'",
            source_ref, revision
        ));
    }

    for chunk in &mut brain.chunks {
        if chunk.source_ref.as_deref() != Some(source_ref) {
            continue;
        }
        let should_archive = chunk.source_revision.as_deref().map(str::trim) != Some(revision);
        if chunk.archived != should_archive {
            changed_chunk_count += 1;
            chunk.archived = should_archive;
        }
        if chunk.archived {
            archived_chunk_count += 1;
        } else {
            active_chunk_count += 1;
        }
    }

    Ok(ProjectBrainSourceRevisionRestore {
        source_ref: source_ref.to_string(),
        source_kind,
        restored_revision: revision.to_string(),
        previous_active_revisions: previous_active_revisions.into_iter().collect(),
        changed_chunk_count,
        active_chunk_count,
        archived_chunk_count,
        total_source_chunk_count,
        evidence_refs: vec![
            format!("source_ref:{}", source_ref),
            format!("source_revision:{}", revision),
        ],
    })
}

pub fn compare_project_brain_source_revisions_from_db(
    source_ref: &str,
    brain: &VectorDB,
) -> ProjectBrainSourceCompare {
    #[derive(Default)]
    struct RevisionAccumulator {
        active: bool,
        node_count: usize,
        chunk_count: usize,
        chunk_indexes: Vec<usize>,
        keywords: Vec<String>,
        summary_parts: Vec<String>,
    }

    let source_ref = source_ref.trim();
    let mut source_kind = "unknown".to_string();
    let mut by_revision = BTreeMap::<String, RevisionAccumulator>::new();
    for chunk in brain
        .chunks
        .iter()
        .filter(|chunk| chunk.source_ref.as_deref() == Some(source_ref))
    {
        if let Some(kind) = chunk
            .source_kind
            .as_deref()
            .map(str::trim)
            .filter(|kind| !kind.is_empty())
        {
            source_kind = kind.to_string();
        }
        let revision = chunk
            .source_revision
            .as_deref()
            .map(str::trim)
            .filter(|revision| !revision.is_empty())
            .unwrap_or("unknown");
        let entry = by_revision.entry(revision.to_string()).or_default();
        entry.node_count += 1;
        entry.chunk_count += 1;
        if !chunk.archived {
            entry.active = true;
        }
        if let Some(chunk_index) = chunk.chunk_index {
            entry.chunk_indexes.push(chunk_index);
        }
        entry.keywords.extend(chunk.keywords.iter().cloned());
        if !chunk.text.trim().is_empty() {
            entry.summary_parts.push(chunk.text.clone());
        }
    }

    let mut revisions = by_revision
        .into_iter()
        .map(|(revision, mut entry)| {
            entry.chunk_indexes.sort_unstable();
            entry.chunk_indexes.dedup();
            let summary = snippet_text(
                &entry
                    .summary_parts
                    .iter()
                    .map(|part| part.trim())
                    .filter(|part| !part.is_empty())
                    .collect::<Vec<_>>()
                    .join("\n"),
                360,
            );
            ProjectBrainSourceCompareRevision {
                revision,
                active: entry.active,
                node_count: entry.node_count,
                chunk_count: entry.chunk_count,
                chunk_indexes: entry.chunk_indexes,
                keywords: normalized_limited_keywords(entry.keywords, 16),
                summary,
            }
        })
        .collect::<Vec<_>>();
    revisions.sort_by(|left, right| {
        right
            .active
            .cmp(&left.active)
            .then_with(|| left.revision.cmp(&right.revision))
    });

    let active_revision = revisions
        .iter()
        .find(|revision| revision.active)
        .map(|revision| revision.revision.clone());
    let active_keywords = revisions
        .iter()
        .find(|revision| revision.active)
        .map(|revision| normalized_keyword_set(&revision.keywords))
        .unwrap_or_default();
    let archived_keywords = revisions
        .iter()
        .filter(|revision| !revision.active)
        .flat_map(|revision| revision.keywords.iter().cloned())
        .collect::<Vec<_>>();
    let archived_keywords = normalized_keyword_set(&archived_keywords);

    let added_keywords = active_keywords
        .difference(&archived_keywords)
        .take(12)
        .cloned()
        .collect::<Vec<_>>();
    let removed_keywords = archived_keywords
        .difference(&active_keywords)
        .take(12)
        .cloned()
        .collect::<Vec<_>>();
    let shared_keywords = active_keywords
        .intersection(&archived_keywords)
        .take(12)
        .cloned()
        .collect::<Vec<_>>();

    let active_summary = revisions
        .iter()
        .find(|revision| revision.active)
        .map(|revision| revision.summary.clone())
        .unwrap_or_default();
    let archived_summary = revisions
        .iter()
        .filter(|revision| !revision.active)
        .map(|revision| revision.summary.as_str())
        .collect::<Vec<_>>()
        .join("\n");

    ProjectBrainSourceCompare {
        source_ref: source_ref.to_string(),
        source_kind,
        active_revision,
        revisions,
        added_keywords: added_keywords.into_iter().collect(),
        removed_keywords: removed_keywords.into_iter().collect(),
        shared_keywords: shared_keywords.into_iter().collect(),
        added_summary: compare_summary_terms(&active_summary, &archived_summary),
        removed_summary: compare_summary_terms(&archived_summary, &active_summary),
        evidence_refs: vec![format!("source_ref:{}", source_ref)],
    }
}

pub fn read_knowledge_index_file(
    project_data_dir: &Path,
    relative_path: &str,
) -> Result<String, String> {
    let path = safe_knowledge_index_file_path(project_data_dir, relative_path)?;
    std::fs::read_to_string(&path).map_err(|e| {
        format!(
            "Failed to read knowledge index file '{}': {}",
            path.display(),
            e
        )
    })
}

