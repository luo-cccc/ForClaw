pub fn safe_knowledge_index_file_path(
    project_data_dir: &Path,
    relative_path: &str,
) -> Result<PathBuf, String> {
    let requested = Path::new(relative_path);
    if requested.is_absolute()
        || requested
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(format!(
            "Knowledge index path must stay inside the active project: {}",
            relative_path
        ));
    }
    let joined = project_data_dir.join(requested);
    let root = project_data_dir
        .canonicalize()
        .unwrap_or_else(|_| project_data_dir.to_path_buf());
    let parent = joined
        .parent()
        .unwrap_or(project_data_dir)
        .canonicalize()
        .unwrap_or_else(|_| project_data_dir.to_path_buf());
    if !parent.starts_with(&root) {
        return Err(format!(
            "Knowledge index path escapes active project: {}",
            relative_path
        ));
    }
    Ok(joined)
}

pub fn project_brain_embedding_batch_status(
    requested_count: usize,
    embedded_count: usize,
    skipped_count: usize,
    errors: &[String],
) -> ProjectBrainEmbeddingBatchStatus {
    if embedded_count == 0 {
        ProjectBrainEmbeddingBatchStatus::Empty
    } else if embedded_count == requested_count && skipped_count == 0 && errors.is_empty() {
        ProjectBrainEmbeddingBatchStatus::Complete
    } else {
        ProjectBrainEmbeddingBatchStatus::Partial
    }
}

pub fn project_brain_source_revision(content: &str) -> String {
    storage::content_revision(content)
}

pub fn build_project_brain_knowledge_index(
    project_id: &str,
    brain: &VectorDB,
    outline: &[storage::OutlineNode],
    lorebook: &[storage::LoreEntry],
) -> ProjectBrainKnowledgeIndex {
    let mut nodes = Vec::new();

    for entry in lorebook {
        nodes.push(ProjectBrainKnowledgeNode {
            id: format!("lore:{}", stable_node_id(&entry.id, &entry.keyword)),
            kind: "lore".to_string(),
            label: entry.keyword.clone(),
            source_ref: format!("lorebook:{}", entry.id),
            source_revision: None,
            source_kind: Some("lorebook".to_string()),
            chunk_index: None,
            archived: false,
            keywords: unique_keywords(vec![entry.keyword.clone()], &entry.content),
            summary: snippet_text(&entry.content, 220),
        });
    }

    for node in outline {
        nodes.push(ProjectBrainKnowledgeNode {
            id: format!(
                "outline:{}",
                stable_node_id(&node.chapter_title, &node.summary)
            ),
            kind: "outline".to_string(),
            label: node.chapter_title.clone(),
            source_ref: format!("outline:{}", node.chapter_title),
            source_revision: None,
            source_kind: Some("outline".to_string()),
            chunk_index: None,
            archived: false,
            keywords: unique_keywords(vec![node.chapter_title.clone()], &node.summary),
            summary: snippet_text(&node.summary, 220),
        });
    }

    for chunk in &brain.chunks {
        let label = if chunk.chapter.trim().is_empty() {
            chunk.id.clone()
        } else {
            chunk.chapter.clone()
        };
        let source_ref = chunk
            .source_ref
            .clone()
            .unwrap_or_else(|| format!("project_brain:{}", chunk.id));
        nodes.push(ProjectBrainKnowledgeNode {
            id: format!("chunk:{}", stable_node_id(&chunk.id, &chunk.chapter)),
            kind: "chunk".to_string(),
            label,
            source_ref,
            source_revision: chunk.source_revision.clone(),
            source_kind: chunk.source_kind.clone(),
            chunk_index: chunk.chunk_index,
            archived: chunk.archived,
            keywords: unique_keywords(chunk.keywords.clone(), &chunk.text),
            summary: snippet_text(&chunk.text, 220),
        });
    }

    let edges = build_knowledge_edges(&nodes);
    let source_history = build_source_history(&nodes);
    ProjectBrainKnowledgeIndex {
        project_id: project_id.to_string(),
        source_count: lorebook.len() + outline.len() + brain.chunks.len(),
        nodes,
        edges,
        source_history,
    }
}

fn build_source_history(nodes: &[ProjectBrainKnowledgeNode]) -> Vec<ProjectBrainSourceHistory> {
    #[derive(Default)]
    struct SourceAccumulator {
        source_kind: Option<String>,
        revisions: BTreeMap<String, ProjectBrainSourceRevision>,
        node_count: usize,
        chunk_count: usize,
        active_revisions: HashSet<String>,
        latest_summary: String,
    }

    let mut by_source = BTreeMap::<String, SourceAccumulator>::new();
    for node in nodes {
        let source_ref = node.source_ref.trim();
        if source_ref.is_empty() {
            continue;
        }
        let entry = by_source.entry(source_ref.to_string()).or_default();
        entry.node_count += 1;
        if node.kind == "chunk" {
            entry.chunk_count += 1;
        }
        if entry.source_kind.is_none() {
            entry.source_kind = node
                .source_kind
                .clone()
                .filter(|kind| !kind.trim().is_empty())
                .or_else(|| Some(node.kind.clone()));
        }
        if !node.summary.trim().is_empty() {
            entry.latest_summary = node.summary.clone();
        }
        if let Some(revision) = node
            .source_revision
            .as_deref()
            .map(str::trim)
            .filter(|revision| !revision.is_empty())
        {
            if !node.archived {
                entry.active_revisions.insert(revision.to_string());
            }
            let revision_entry = entry
                .revisions
                .entry(revision.to_string())
                .or_insert_with(|| ProjectBrainSourceRevision {
                    revision: revision.to_string(),
                    node_count: 0,
                    chunk_indexes: Vec::new(),
                    active: false,
                });
            revision_entry.node_count += 1;
            if let Some(chunk_index) = node.chunk_index {
                revision_entry.chunk_indexes.push(chunk_index);
            }
        }
    }

    by_source
        .into_iter()
        .map(|(source_ref, entry)| {
            let mut revisions = entry.revisions.into_values().collect::<Vec<_>>();
            for revision in &mut revisions {
                revision.chunk_indexes.sort_unstable();
                revision.chunk_indexes.dedup();
                revision.active = entry.active_revisions.contains(&revision.revision);
            }
            ProjectBrainSourceHistory {
                source_ref,
                source_kind: entry.source_kind.unwrap_or_else(|| "unknown".to_string()),
                revisions,
                node_count: entry.node_count,
                chunk_count: entry.chunk_count,
                latest_summary: snippet_text(&entry.latest_summary, 220),
            }
        })
        .collect()
}

fn build_knowledge_edges(nodes: &[ProjectBrainKnowledgeNode]) -> Vec<ProjectBrainKnowledgeEdge> {
    let mut keyword_to_nodes = BTreeMap::<String, Vec<&ProjectBrainKnowledgeNode>>::new();
    for node in nodes {
        for keyword in &node.keywords {
            keyword_to_nodes
                .entry(keyword.to_string())
                .or_default()
                .push(node);
        }
    }

    let mut seen = HashSet::new();
    let mut edges = Vec::new();
    for (keyword, linked_nodes) in keyword_to_nodes {
        if linked_nodes.len() < 2 {
            continue;
        }
        for left in 0..linked_nodes.len() {
            for right in left + 1..linked_nodes.len() {
                let from = &linked_nodes[left].id;
                let to = &linked_nodes[right].id;
                let key = if from <= to {
                    format!("{}|{}|{}", from, to, keyword)
                } else {
                    format!("{}|{}|{}", to, from, keyword)
                };
                if !seen.insert(key) {
                    continue;
                }
                edges.push(ProjectBrainKnowledgeEdge {
                    from: from.clone(),
                    to: to.clone(),
                    relation: format!("shared_keyword:{}", keyword),
                    evidence_ref: keyword.clone(),
                });
            }
        }
    }
    edges
}

fn unique_keywords(mut seed: Vec<String>, text: &str) -> Vec<String> {
    seed.extend(agent_harness_core::extract_keywords(text));
    let mut seen = HashSet::new();
    seed.into_iter()
        .map(|keyword| keyword.trim().to_string())
        .filter(|keyword| keyword.chars().count() >= 2 && seen.insert(keyword.to_lowercase()))
        .take(12)
        .collect()
}

fn normalized_limited_keywords(seed: Vec<String>, limit: usize) -> Vec<String> {
    let mut seen = HashSet::new();
    seed.into_iter()
        .flat_map(|keyword| unique_keywords(vec![keyword.clone()], &keyword))
        .map(|keyword| keyword.trim().to_string())
        .filter(|keyword| keyword.chars().count() >= 2 && seen.insert(keyword.to_lowercase()))
        .take(limit)
        .collect()
}

fn normalized_keyword_set(seed: &[String]) -> BTreeSet<String> {
    normalized_limited_keywords(seed.to_vec(), 64)
        .into_iter()
        .collect()
}

fn compare_summary_terms(primary: &str, baseline: &str) -> Vec<String> {
    let baseline_terms = normalized_keyword_set(&agent_harness_core::extract_keywords(baseline));
    normalized_limited_keywords(agent_harness_core::extract_keywords(primary), 24)
        .into_iter()
        .filter(|term| !baseline_terms.contains(term))
        .take(8)
        .collect()
}

fn stable_node_id(primary: &str, fallback: &str) -> String {
    let source = if primary.trim().is_empty() {
        fallback
    } else {
        primary
    };
    storage::content_revision(source)
        .split('-')
        .next()
        .unwrap_or("0000000000000000")
        .to_string()
}
