fn subtask_kind_label(kind: WriterSubtaskKind) -> &'static str {
    match kind {
        WriterSubtaskKind::Research => "research",
        WriterSubtaskKind::Diagnostic => "diagnostic",
        WriterSubtaskKind::Drafting => "drafting",
    }
}

fn normalized_subtask_id(subtask_id: &str) -> Result<String, String> {
    let id = subtask_id.trim();
    if id.is_empty() || id.len() > 96 {
        return Err("Writer Agent subtask id must be 1-96 chars".to_string());
    }
    if !id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(format!("Invalid Writer Agent subtask id: {}", subtask_id));
    }
    Ok(id.to_string())
}

fn safe_relative_path(relative_path: &str, label: &str) -> Result<PathBuf, String> {
    let path = Path::new(relative_path);
    if relative_path.trim().is_empty() || path.is_absolute() {
        return Err(format!(
            "{} path must be relative: {}",
            label, relative_path
        ));
    }
    if path.components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::Prefix(_) | Component::RootDir
        )
    }) {
        return Err(format!(
            "{} path must stay inside the isolated workspace: {}",
            label, relative_path
        ));
    }
    Ok(path.to_path_buf())
}

fn estimate_research_provider_input_tokens(input: &WriterSubtaskProviderBudgetInput) -> u64 {
    EXTERNAL_RESEARCH_TOOL_OVERHEAD_TOKENS
        + estimate_chars_as_tokens(&input.objective)
        + estimate_chars_as_tokens(&input.query)
        + input.context_chars as u64 / 3
}

fn estimate_chars_as_tokens(value: &str) -> u64 {
    value.trim().chars().count() as u64 / 3
}

fn normalize_strings(values: Vec<String>) -> Vec<String> {
    let mut normalized = Vec::new();
    for value in values {
        let value = value.trim();
        if value.is_empty() || normalized.iter().any(|existing| existing == value) {
            continue;
        }
        normalized.push(value.to_string());
    }
    normalized
}
