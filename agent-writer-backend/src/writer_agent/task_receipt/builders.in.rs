pub fn failure_bundle_from_tool_execution(
    task_id: Option<&str>,
    execution: &agent_harness_core::ToolExecution,
    created_at_ms: u64,
) -> Option<WriterFailureEvidenceBundle> {
    let error = execution.error.as_ref()?;
    let first_remediation_code = execution
        .remediation
        .first()
        .map(|remediation| remediation.code.as_str());
    let mut evidence_refs = vec![format!("tool:{}", execution.tool_name)];
    if let Some(task_id) = task_id.filter(|value| !value.trim().is_empty()) {
        evidence_refs.push(format!("task:{}", task_id.trim()));
    }
    evidence_refs.extend(
        execution
            .remediation
            .iter()
            .map(|remediation| format!("remediation:{}", remediation.code)),
    );
    let mut remediation = execution
        .remediation
        .iter()
        .map(|item| format!("{}: {}", item.code, item.message))
        .collect::<Vec<_>>();
    if remediation.is_empty() {
        remediation.push(
            "tool_handler_failed: Record the tool failure evidence and retry only after the tool inventory, permission policy, or workspace state changes."
                .to_string(),
        );
    }

    Some(WriterFailureEvidenceBundle::new(
        tool_failure_category(first_remediation_code, error),
        tool_failure_code(first_remediation_code),
        format!("Tool '{}' failed: {}", execution.tool_name, error),
        true,
        task_id.map(|value| value.trim().to_string()),
        evidence_refs,
        serde_json::json!({
            "toolName": execution.tool_name,
            "input": execution.input,
            "output": execution.output,
            "error": execution.error,
            "durationMs": execution.duration_ms,
            "remediation": execution.remediation,
        }),
        remediation,
        created_at_ms,
    ))
}

pub fn build_continuity_diagnostic_receipt(
    task_id: impl Into<String>,
    observation: &WriterObservation,
    objective: &str,
    context_pack: &WritingContextPack,
    created_at_ms: u64,
) -> WriterTaskReceipt {
    let mut required_evidence = context_pack
        .sources
        .iter()
        .filter(|source| source.char_count > 0)
        .filter(|source| is_diagnostic_required_source(&source.source))
        .map(|source| diagnostic_source_name(&source.source))
        .collect::<Vec<_>>();
    if required_evidence.is_empty() {
        required_evidence.push("editor_observation".to_string());
    }

    let mut source_refs = Vec::new();
    if required_evidence
        .iter()
        .any(|evidence| evidence == "editor_observation")
    {
        source_refs.push("editor_observation".to_string());
    }
    source_refs.push(format!("observation:{}", observation.id));
    if let Some(chapter) = observation.chapter_title.as_ref() {
        source_refs.push(format!("chapter:{}", chapter));
    }
    if let Some(revision) = observation.chapter_revision.as_ref() {
        source_refs.push(format!("revision:{}", revision));
    }
    source_refs.extend(
        context_pack
            .sources
            .iter()
            .filter(|source| source.char_count > 0)
            .map(|source| diagnostic_source_name(&source.source)),
    );

    WriterTaskReceipt::new(
        task_id,
        "ContinuityDiagnostic",
        observation.chapter_title.clone(),
        truncate_chars(objective, 240),
        required_evidence,
        vec![
            "diagnostic_report".to_string(),
            "evidence_summary".to_string(),
            "reviewable_recommendations".to_string(),
        ],
        vec![
            "chapter_draft".to_string(),
            "saved_chapter".to_string(),
            "memory_write".to_string(),
            "typed_write_operation".to_string(),
        ],
        source_refs,
        observation.chapter_revision.clone(),
        created_at_ms,
    )
}

pub fn build_diagnostic_report_artifact(
    receipt: &WriterTaskReceipt,
    report: &str,
    created_at_ms: u64,
) -> Result<WriterTaskArtifact, Vec<WriterTaskReceiptMismatch>> {
    let mismatches = receipt.validate_artifact_attempt(&receipt.task_id, "diagnostic_report");
    if !mismatches.is_empty() {
        return Err(mismatches);
    }

    let report = report.trim();
    let content_char_count = report.chars().count();
    let content_truncated = content_char_count > MAX_TASK_ARTIFACT_CONTENT_CHARS;
    let content = truncate_chars(report, MAX_TASK_ARTIFACT_CONTENT_CHARS);
    let artifact_id = format!("{}:diagnostic_report", receipt.task_id);
    let mut source_refs = vec![
        format!("receipt:{}", receipt.task_id),
        format!("artifact:{}", artifact_id),
    ];
    source_refs.extend(receipt.source_refs.iter().cloned());

    Ok(WriterTaskArtifact::new(
        artifact_id,
        receipt.task_id.clone(),
        receipt.task_kind.clone(),
        "diagnostic_report",
        receipt.chapter.clone(),
        receipt.objective.clone(),
        content,
        content_char_count,
        content_truncated,
        receipt.required_evidence.clone(),
        source_refs,
        receipt.base_revision.clone(),
        created_at_ms,
    ))
}

pub fn build_planning_review_receipt(
    task_id: impl Into<String>,
    observation: &WriterObservation,
    objective: &str,
    context_pack: &WritingContextPack,
    created_at_ms: u64,
) -> WriterTaskReceipt {
    let mut required_evidence = context_pack
        .sources
        .iter()
        .filter(|source| source.char_count > 0)
        .filter(|source| is_diagnostic_required_source(&source.source))
        .map(|source| diagnostic_source_name(&source.source))
        .collect::<Vec<_>>();
    if required_evidence.is_empty() {
        required_evidence.push("editor_observation".to_string());
    }

    let mut source_refs = Vec::new();
    if required_evidence
        .iter()
        .any(|evidence| evidence == "editor_observation")
    {
        source_refs.push("editor_observation".to_string());
    }
    source_refs.push(format!("observation:{}", observation.id));
    if let Some(chapter) = observation.chapter_title.as_ref() {
        source_refs.push(format!("chapter:{}", chapter));
    }
    if let Some(revision) = observation.chapter_revision.as_ref() {
        source_refs.push(format!("revision:{}", revision));
    }
    source_refs.extend(
        context_pack
            .sources
            .iter()
            .filter(|source| source.char_count > 0)
            .map(|source| diagnostic_source_name(&source.source)),
    );

    WriterTaskReceipt::new(
        task_id,
        "PlanningReview",
        observation.chapter_title.clone(),
        truncate_chars(objective, 240),
        required_evidence,
        vec![
            "planning_review_report".to_string(),
            "evidence_summary".to_string(),
            "next_action_recommendations".to_string(),
        ],
        vec![
            "chapter_draft".to_string(),
            "saved_chapter".to_string(),
            "memory_write".to_string(),
            "typed_write_operation".to_string(),
        ],
        source_refs,
        observation.chapter_revision.clone(),
        created_at_ms,
    )
}

pub fn build_planning_review_artifact(
    receipt: &WriterTaskReceipt,
    report: &str,
    created_at_ms: u64,
) -> Result<WriterTaskArtifact, Vec<WriterTaskReceiptMismatch>> {
    let mismatches = receipt.validate_artifact_attempt(&receipt.task_id, "planning_review_report");
    if !mismatches.is_empty() {
        return Err(mismatches);
    }

    let report = report.trim();
    let content_char_count = report.chars().count();
    let content_truncated = content_char_count > MAX_TASK_ARTIFACT_CONTENT_CHARS;
    let content = truncate_chars(report, MAX_TASK_ARTIFACT_CONTENT_CHARS);
    let artifact_id = format!("{}:planning_review_report", receipt.task_id);
    let mut source_refs = vec![
        format!("receipt:{}", receipt.task_id),
        format!("artifact:{}", artifact_id),
    ];
    source_refs.extend(receipt.source_refs.iter().cloned());

    Ok(WriterTaskArtifact::new(
        artifact_id,
        receipt.task_id.clone(),
        receipt.task_kind.clone(),
        "planning_review_report",
        receipt.chapter.clone(),
        receipt.objective.clone(),
        content,
        content_char_count,
        content_truncated,
        receipt.required_evidence.clone(),
        source_refs,
        receipt.base_revision.clone(),
        created_at_ms,
    ))
}
