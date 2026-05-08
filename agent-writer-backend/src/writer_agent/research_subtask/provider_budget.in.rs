pub fn external_research_provider_budget_report(
    input: &WriterSubtaskProviderBudgetInput,
) -> Result<WriterProviderBudgetReport, String> {
    let _subtask_id = normalized_subtask_id(&input.subtask_id)?;
    if input.kind != WriterSubtaskKind::Research {
        return Err(
            "External research provider budget is only valid for research subtasks".to_string(),
        );
    }
    let model = input.model.trim();
    if model.is_empty() {
        return Err("External research provider model is empty".to_string());
    }
    let estimated_input_tokens = estimate_research_provider_input_tokens(input);
    let requested_output_tokens = if input.requested_output_tokens == 0 {
        DEFAULT_EXTERNAL_RESEARCH_OUTPUT_TOKENS
    } else {
        input.requested_output_tokens
    };
    Ok(evaluate_provider_budget(WriterProviderBudgetRequest::new(
        WriterProviderBudgetTask::ExternalResearch,
        model,
        estimated_input_tokens,
        requested_output_tokens,
    )))
}

pub fn failure_bundle_from_subtask_provider_budget(
    kind: WriterSubtaskKind,
    subtask_id: &str,
    objective: &str,
    report: WriterProviderBudgetReport,
    artifact_refs: Vec<String>,
    created_at_ms: u64,
) -> Result<Option<WriterFailureEvidenceBundle>, String> {
    let subtask_id = normalized_subtask_id(subtask_id)?;
    let objective = objective.trim();
    if objective.is_empty() {
        return Err("Writer Agent subtask objective is empty".to_string());
    }
    let artifact_refs = normalize_strings(artifact_refs);
    for artifact in &artifact_refs {
        let expected_prefix = format!("subtask:{}:artifact:", subtask_id);
        if !artifact.starts_with(&expected_prefix) {
            return Err(format!(
                "subtask artifact ref is outside the isolated workspace: {}",
                artifact
            ));
        }
    }
    if !matches!(
        report.decision,
        WriterProviderBudgetDecision::ApprovalRequired | WriterProviderBudgetDecision::Blocked
    ) {
        return Ok(None);
    }

    let kind_label = subtask_kind_label(kind);
    let code = if report.decision == WriterProviderBudgetDecision::Blocked {
        "RESEARCH_SUBTASK_PROVIDER_BUDGET_BLOCKED"
    } else {
        "RESEARCH_SUBTASK_PROVIDER_BUDGET_APPROVAL_REQUIRED"
    };
    let message = format!(
        "{} subtask '{}' provider budget requires review before calling the external research provider.",
        kind_label, subtask_id
    );
    let mut remediation = report.remediation.clone();
    remediation.push(
        "subtask_external_research_budget: Keep the research evidence-only; reduce query/context scope or collect author approval before retrying.".to_string(),
    );

    Ok(Some(WriterFailureEvidenceBundle::new(
        WriterFailureCategory::ProviderFailed,
        code,
        message,
        true,
        Some(subtask_id.clone()),
        normalize_strings(
            vec![
                format!("subtask:{}", subtask_id),
                format!("subtask:{}:kind:{}", subtask_id, kind_label),
                format!("model:{}", report.model),
                format!("estimated_tokens:{}", report.estimated_total_tokens),
                format!("estimated_cost_micros:{}", report.estimated_cost_micros),
            ]
            .into_iter()
            .chain(artifact_refs.iter().cloned())
            .collect(),
        ),
        serde_json::json!({
            "subtaskId": subtask_id,
            "kind": kind_label,
            "objective": objective,
            "artifactRefs": artifact_refs,
            "providerBudget": report,
        }),
        remediation,
        created_at_ms,
    )))
}

pub fn build_evidence_only_subtask_result(
    kind: WriterSubtaskKind,
    subtask_id: &str,
    objective: &str,
    summary: &str,
    evidence_refs: Vec<EvidenceRef>,
    artifact_refs: Vec<String>,
    attempted_operations: &[WriterOperation],
    created_at_ms: u64,
) -> Result<WriterSubtaskResult, String> {
    Ok(WriterSubtaskResult {
        subtask_id: normalized_subtask_id(subtask_id)?,
        kind,
        objective: objective.trim().to_string(),
        summary: summary.trim().to_string(),
        evidence_refs,
        artifact_refs: normalize_strings(artifact_refs),
        blocked_operation_kinds: denied_subtask_operations(kind, attempted_operations),
        created_at_ms,
    })
}

pub fn validate_evidence_only_subtask_result(result: &WriterSubtaskResult) -> Vec<String> {
    let mut errors = Vec::new();
    if normalized_subtask_id(&result.subtask_id).is_err() {
        errors.push("subtask id is invalid".to_string());
    }
    if result.objective.trim().is_empty() {
        errors.push("subtask objective is empty".to_string());
    }
    if result.summary.trim().is_empty() {
        errors.push("subtask summary is empty".to_string());
    }
    if result.evidence_refs.is_empty() && result.artifact_refs.is_empty() {
        errors.push("subtask result has no evidence refs or artifact refs".to_string());
    }
    for evidence in &result.evidence_refs {
        if evidence.reference.trim().is_empty() || evidence.snippet.trim().is_empty() {
            errors.push("subtask evidence ref is missing reference or snippet".to_string());
        }
    }
    for artifact in &result.artifact_refs {
        let expected_prefix = format!("subtask:{}:artifact:", result.subtask_id);
        if !artifact.starts_with(&expected_prefix) {
            errors.push(format!(
                "subtask artifact ref is outside the isolated workspace: {}",
                artifact
            ));
        }
    }
    errors
}

pub fn failure_bundle_from_subtask_tool_execution(
    kind: WriterSubtaskKind,
    subtask_id: &str,
    objective: &str,
    execution: &agent_harness_core::ToolExecution,
    artifact_refs: Vec<String>,
    created_at_ms: u64,
) -> Result<Option<WriterFailureEvidenceBundle>, String> {
    let subtask_id = normalized_subtask_id(subtask_id)?;
    let objective = objective.trim();
    if objective.is_empty() {
        return Err("Writer Agent subtask objective is empty".to_string());
    }
    let artifact_refs = normalize_strings(artifact_refs);
    for artifact in &artifact_refs {
        let expected_prefix = format!("subtask:{}:artifact:", subtask_id);
        if !artifact.starts_with(&expected_prefix) {
            return Err(format!(
                "subtask artifact ref is outside the isolated workspace: {}",
                artifact
            ));
        }
    }

    let Some(mut bundle) =
        failure_bundle_from_tool_execution(Some(&subtask_id), execution, created_at_ms)
    else {
        return Ok(None);
    };
    let kind_label = subtask_kind_label(kind);
    let tool_details = bundle.details.clone();
    let error = execution.error.as_deref().unwrap_or("unknown tool error");
    bundle.task_id = Some(subtask_id.clone());
    bundle.message = format!(
        "{} subtask '{}' failed while running tool '{}': {}",
        kind_label, subtask_id, execution.tool_name, error
    );
    bundle.evidence_refs = normalize_strings(
        bundle
            .evidence_refs
            .into_iter()
            .chain([
                format!("subtask:{}", subtask_id),
                format!("subtask:{}:kind:{}", subtask_id, kind_label),
            ])
            .chain(artifact_refs.iter().cloned())
            .collect(),
    );
    bundle.details = serde_json::json!({
        "subtaskId": subtask_id,
        "kind": kind_label,
        "objective": objective,
        "artifactRefs": artifact_refs,
        "toolExecution": tool_details,
    });
    bundle.remediation = normalize_strings(
        bundle
            .remediation
            .into_iter()
            .chain([format!(
                "subtask_{}_failure: Keep this subtask evidence-only; inspect isolated artifacts and retry only after the tool/provider configuration or query scope changes.",
                kind_label
            )])
            .collect(),
    );
    Ok(Some(bundle))
}

pub fn denied_subtask_operations(
    _kind: WriterSubtaskKind,
    attempted_operations: &[WriterOperation],
) -> Vec<String> {
    normalize_strings(
        attempted_operations
            .iter()
            .map(|operation| subtask_operation_kind_label(operation).to_string())
            .collect(),
    )
}

pub fn subtask_operation_kind_label(operation: &WriterOperation) -> &'static str {
    match operation {
        WriterOperation::TextInsert { .. } => "text.insert",
        WriterOperation::TextReplace { .. } => "text.replace",
        WriterOperation::TextAnnotate { .. } => "text.annotate",
        WriterOperation::CanonUpsertEntity { .. } => "canon.upsert_entity",
        WriterOperation::CanonUpdateAttribute { .. } => "canon.update_attribute",
        WriterOperation::CanonUpsertRule { .. } => "canon.upsert_rule",
        WriterOperation::PromiseAdd { .. } => "promise.add",
        WriterOperation::PromiseResolve { .. } => "promise.resolve",
        WriterOperation::PromiseDefer { .. } => "promise.defer",
        WriterOperation::PromiseAbandon { .. } => "promise.abandon",
        WriterOperation::CharacterUpsert { .. } => "character.upsert",
        WriterOperation::CharacterStateUpsert { .. } => "character_state.upsert",
        WriterOperation::RelationshipUpsert { .. } => "relationship.upsert",
        WriterOperation::PromiseBindSubject { .. } => "promise.bind_subject",
        WriterOperation::StyleUpdatePreference { .. } => "style.update_preference",
        WriterOperation::StoryContractUpsert { .. } => "story_contract.upsert",
        WriterOperation::ChapterMissionUpsert { .. } => "chapter_mission.upsert",
        WriterOperation::OutlineUpdate { .. } => "outline.update",
        WriterOperation::KnowledgeUpsert { .. } => "knowledge.upsert",
        WriterOperation::KnowledgeOwnershipUpsert { .. } => "knowledge_ownership.upsert",
        WriterOperation::IdentityLayerUpsert { .. } => "identity_layer.upsert",
        WriterOperation::RevealEventRecord { .. } => "reveal_event.record",
        WriterOperation::SceneUpsert { .. } => "scene.upsert",
        WriterOperation::SceneStateUpsert { .. } => "scene_state.upsert",
        WriterOperation::SceneObligationUpsert { .. } => "scene_obligation.upsert",
        WriterOperation::SceneResultRecord { .. } => "scene_result.record",
        WriterOperation::TimeSliceUpsert { .. } => "time_slice.upsert",
        WriterOperation::ChapterTimeMappingUpsert { .. } => "chapter_time_mapping.upsert",
        WriterOperation::TimelineEventRecord { .. } => "timeline_event.record",
    }
}
