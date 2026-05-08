fn is_diagnostic_required_source(source: &ContextSource) -> bool {
    matches!(
        source,
        ContextSource::CursorPrefix
            | ContextSource::SelectedText
            | ContextSource::CanonSlice
            | ContextSource::ChapterMission
            | ContextSource::ProjectBrief
            | ContextSource::BookState
            | ContextSource::ResultFeedback
            | ContextSource::NextBeat
            | ContextSource::ArcSnapshot
            | ContextSource::VolumeSnapshot
            | ContextSource::PromiseSlice
            | ContextSource::DecisionSlice
            | ContextSource::StoryImpactRadius
    )
}

fn diagnostic_source_name(source: &ContextSource) -> String {
    format!("{:?}", source)
}

fn truncate_chars(text: &str, limit: usize) -> String {
    text.chars().take(limit).collect()
}

pub fn normalize_strings(values: Vec<String>) -> Vec<String> {
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

fn tool_failure_category(code: Option<&str>, error: &str) -> WriterFailureCategory {
    let code = code.unwrap_or_default();
    let lower_error = error.to_ascii_lowercase();
    if matches!(
        code,
        "approval_required" | "external_access_denied" | "tool_denied"
    ) || lower_error.contains("approval")
        || lower_error.contains("permission")
        || lower_error.contains("denied")
    {
        WriterFailureCategory::ToolDenied
    } else {
        WriterFailureCategory::ToolFailed
    }
}

fn tool_failure_code(code: Option<&str>) -> String {
    let Some(code) = code.filter(|value| !value.trim().is_empty()) else {
        return "TOOL_EXECUTION_FAILED".to_string();
    };
    let normalized = code
        .trim()
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_uppercase()
            } else {
                '_'
            }
        })
        .collect::<String>();
    if normalized.starts_with("TOOL_") {
        normalized
    } else {
        format!("TOOL_{}", normalized)
    }
}
