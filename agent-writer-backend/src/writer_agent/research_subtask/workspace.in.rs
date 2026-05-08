pub fn create_subtask_workspace(
    project_data_dir: &Path,
    kind: WriterSubtaskKind,
    subtask_id: &str,
) -> Result<WriterSubtaskWorkspace, String> {
    let workspace_dir = subtask_workspace_dir(project_data_dir, subtask_id)?;
    let artifact_dir = workspace_dir.join(SUBTASK_ARTIFACT_DIR);
    std::fs::create_dir_all(&artifact_dir).map_err(|e| {
        format!(
            "Failed to create Writer Agent subtask artifact dir '{}': {}",
            artifact_dir.display(),
            e
        )
    })?;
    Ok(WriterSubtaskWorkspace {
        subtask_id: normalized_subtask_id(subtask_id)?,
        kind,
        workspace_dir: workspace_dir.to_string_lossy().to_string(),
        artifact_dir: artifact_dir.to_string_lossy().to_string(),
    })
}

pub fn subtask_workspace_dir(project_data_dir: &Path, subtask_id: &str) -> Result<PathBuf, String> {
    Ok(project_data_dir
        .join(SUBTASK_ROOT_DIR)
        .join(normalized_subtask_id(subtask_id)?))
}

pub fn safe_subtask_artifact_path(
    project_data_dir: &Path,
    subtask_id: &str,
    relative_path: &str,
) -> Result<PathBuf, String> {
    let artifact_dir =
        subtask_workspace_dir(project_data_dir, subtask_id)?.join(SUBTASK_ARTIFACT_DIR);
    let relative = safe_relative_path(relative_path, "subtask artifact")?;
    let joined = artifact_dir.join(relative);
    let root = artifact_dir
        .canonicalize()
        .unwrap_or_else(|_| artifact_dir.clone());
    let parent = joined.parent().unwrap_or(&artifact_dir);
    let stays_in_workspace = if parent.exists() {
        parent
            .canonicalize()
            .map(|canonical| canonical.starts_with(&root))
            .unwrap_or(false)
    } else {
        joined.starts_with(&artifact_dir)
    };
    if !stays_in_workspace {
        return Err(format!(
            "Subtask artifact path escapes isolated workspace: {}",
            relative_path
        ));
    }
    Ok(joined)
}

pub fn write_subtask_artifact(
    project_data_dir: &Path,
    subtask_id: &str,
    relative_path: &str,
    content: &str,
) -> Result<String, String> {
    let path = safe_subtask_artifact_path(project_data_dir, subtask_id, relative_path)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            format!(
                "Failed to create subtask artifact parent '{}': {}",
                parent.display(),
                e
            )
        })?;
    }
    crate::storage::atomic_write(&path, content)?;
    Ok(format!(
        "subtask:{}:artifact:{}",
        normalized_subtask_id(subtask_id)?,
        relative_path.replace('\\', "/")
    ))
}

pub fn tool_policy_for_subtask(kind: WriterSubtaskKind) -> ToolPolicyContract {
    match kind {
        WriterSubtaskKind::Research => ToolPolicyContract {
            max_side_effect_level: ToolSideEffectLevel::ProviderCall,
            allow_approval_required: false,
            required_tool_tags: vec!["project".to_string()],
        },
        WriterSubtaskKind::Diagnostic => ToolPolicyContract {
            max_side_effect_level: ToolSideEffectLevel::Read,
            allow_approval_required: false,
            required_tool_tags: vec!["project".to_string()],
        },
        WriterSubtaskKind::Drafting => ToolPolicyContract {
            max_side_effect_level: ToolSideEffectLevel::ProviderCall,
            allow_approval_required: false,
            required_tool_tags: vec!["generation".to_string(), "preview".to_string()],
        },
    }
}

pub fn tool_filter_for_subtask(kind: WriterSubtaskKind) -> ToolFilter {
    let policy = tool_policy_for_subtask(kind);
    ToolFilter {
        intent: match kind {
            WriterSubtaskKind::Research => None,
            WriterSubtaskKind::Diagnostic => Some(Intent::AnalyzeText),
            WriterSubtaskKind::Drafting => Some(Intent::GenerateContent),
        },
        include_requires_approval: policy.allow_approval_required,
        include_disabled: false,
        max_side_effect_level: Some(policy.max_side_effect_level),
        required_tags: policy.required_tool_tags,
    }
}

pub fn subtask_started_payload(
    kind: WriterSubtaskKind,
    workspace: &WriterSubtaskWorkspace,
    objective: &str,
) -> WriterSubtaskRunEventPayload {
    WriterSubtaskRunEventPayload {
        subtask_id: workspace.subtask_id.clone(),
        kind,
        status: "started".to_string(),
        objective: objective.trim().to_string(),
        summary: "Subtask workspace and tool policy prepared.".to_string(),
        evidence_count: 0,
        artifact_count: 0,
        blocked_operation_count: 0,
        evidence_refs: Vec::new(),
        artifact_refs: Vec::new(),
        blocked_operation_kinds: Vec::new(),
        tool_policy: subtask_tool_policy_summary(kind),
    }
}

pub fn subtask_completed_payload(result: &WriterSubtaskResult) -> WriterSubtaskRunEventPayload {
    WriterSubtaskRunEventPayload {
        subtask_id: result.subtask_id.clone(),
        kind: result.kind,
        status: "completed".to_string(),
        objective: result.objective.clone(),
        summary: result.summary.clone(),
        evidence_count: result.evidence_refs.len(),
        artifact_count: result.artifact_refs.len(),
        blocked_operation_count: result.blocked_operation_kinds.len(),
        evidence_refs: result
            .evidence_refs
            .iter()
            .map(|evidence| evidence.reference.clone())
            .collect(),
        artifact_refs: result.artifact_refs.clone(),
        blocked_operation_kinds: result.blocked_operation_kinds.clone(),
        tool_policy: subtask_tool_policy_summary(result.kind),
    }
}

pub fn subtask_tool_policy_summary(kind: WriterSubtaskKind) -> WriterSubtaskToolPolicySummary {
    let policy = tool_policy_for_subtask(kind);
    WriterSubtaskToolPolicySummary {
        max_side_effect_level: format!("{:?}", policy.max_side_effect_level),
        allow_approval_required: policy.allow_approval_required,
        required_tool_tags: policy.required_tool_tags,
    }
}
