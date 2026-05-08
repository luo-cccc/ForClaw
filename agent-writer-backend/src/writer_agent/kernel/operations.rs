use super::*;

impl WriterAgentKernel {
    pub fn execute_operation(
        &mut self,
        operation: WriterOperation,
        current_content: &str,
        current_revision: &str,
    ) -> Result<OperationResult, String> {
        if operation_is_write_capable(&operation) {
            let result = OperationResult {
                success: false,
                operation,
                error: Some(super::operation::OperationError::approval_required(
                    "Write-capable operations require an explicit surfaced approval context",
                )),
                revision_after: None,
            };
            self.record_operation_result_lifecycle(&result, None, None);
            return Ok(result);
        }

        let result = self.execute_operation_inner(operation, current_content, current_revision)?;
        self.record_operation_result_lifecycle(&result, None, None);
        Ok(result)
    }

    fn execute_operation_inner(
        &mut self,
        operation: WriterOperation,
        current_content: &str,
        current_revision: &str,
    ) -> Result<OperationResult, String> {
        execute_writer_operation(
            &mut self.memory,
            &self.active_chapter,
            operation,
            current_content,
            current_revision,
        )
    }

    pub fn approve_editor_operation(
        &mut self,
        operation: WriterOperation,
        current_revision: &str,
    ) -> Result<OperationResult, String> {
        self.approve_editor_operation_with_approval(operation, current_revision, None)
    }

    pub fn approve_editor_operation_with_approval(
        &mut self,
        operation: WriterOperation,
        current_revision: &str,
        approval: Option<&super::operation::OperationApproval>,
    ) -> Result<OperationResult, String> {
        let requires_approval = operation_is_write_capable(&operation);
        let approval_decided_at = now_ms();
        if requires_approval && !approval.is_some_and(|context| context.is_valid_for_write()) {
            self.record_approval_decided_run_event(
                &operation,
                approval,
                false,
                "missing or invalid surfaced approval context",
                approval_decided_at,
            );
            let result = OperationResult {
                success: false,
                operation,
                error: Some(super::operation::OperationError::approval_required(
                    "Write-capable operations require an explicit surfaced approval context",
                )),
                revision_after: None,
            };
            self.record_operation_result_lifecycle(&result, approval, None);
            return Ok(result);
        }

        if requires_approval
            && crate::writer_agent::metacognition::metacognitive_operation_is_write_sensitive(
                &operation,
            )
        {
            let meta = self.trace_snapshot(40).metacognitive_snapshot;
            if let Some(reason) =
                crate::writer_agent::metacognition::metacognitive_write_gate_reason(&meta)
            {
                self.record_metacognitive_gate_block_run_event(
                    &WriterAgentTask::ProposalEvaluation,
                    approval
                        .and_then(|context| context.proposal_id.clone())
                        .unwrap_or_else(|| "approved-operation".to_string()),
                    &reason,
                    &meta,
                    approval_decided_at,
                );
                self.record_approval_decided_run_event(
                    &operation,
                    approval,
                    false,
                    "metacognitive write gate blocked this operation",
                    approval_decided_at,
                );
                let result = OperationResult {
                    success: false,
                    operation,
                    error: Some(super::operation::OperationError::approval_required(&reason)),
                    revision_after: None,
                };
                self.record_operation_result_lifecycle(&result, approval, Some(reason));
                return Ok(result);
            }
        }

        if let Some(context) = approval {
            self.memory
                .record_decision(
                    self.active_chapter.as_deref().unwrap_or("project"),
                    &format!("Approved operation: {}", operation_kind_label(&operation)),
                    "approved_writer_operation",
                    &[],
                    &format!(
                        "{} approved from {}: {}",
                        context.actor, context.source, context.reason
                    ),
                    &approval_sources(context),
                )
                .ok();
        }

        if requires_approval {
            self.record_approval_decided_run_event(
                &operation,
                approval,
                true,
                "valid surfaced approval context",
                approval_decided_at,
            );
            self.push_operation_lifecycle(
                approval.and_then(|context| context.proposal_id.clone()),
                &operation,
                WriterOperationLifecycleState::Approved,
                approval.map(|context| context.source.clone()),
                None,
                None,
                approval_decided_at,
            );
        }

        let result = match &operation {
            WriterOperation::TextInsert { revision, .. }
            | WriterOperation::TextReplace { revision, .. } => {
                if revision != current_revision {
                    Ok(OperationResult {
                        success: false,
                        operation,
                        error: Some(super::operation::OperationError::conflict(
                            "Proposal is stale; the chapter changed since it was created",
                        )),
                        revision_after: None,
                    })
                } else {
                    Ok(OperationResult {
                        success: true,
                        operation,
                        error: None,
                        revision_after: Some(current_revision.to_string()),
                    })
                }
            }
            _ => self.execute_operation_inner(operation, "", current_revision),
        }?;
        self.record_operation_result_lifecycle(&result, approval, None);
        Ok(result)
    }

    pub fn record_operation_durable_save(
        &mut self,
        proposal_id: Option<String>,
        operation: WriterOperation,
        save_result: String,
    ) -> Result<(), String> {
        self.record_operation_durable_save_with_post_write(
            proposal_id,
            operation,
            save_result,
            None,
            None,
            None,
        )
    }

    pub fn record_operation_durable_save_with_post_write(
        &mut self,
        proposal_id: Option<String>,
        operation: WriterOperation,
        save_result: String,
        saved_text: Option<String>,
        chapter_title: Option<String>,
        chapter_revision: Option<String>,
    ) -> Result<(), String> {
        if !operation_is_write_capable(&operation) {
            return Ok(());
        }

        let normalized = if save_result.trim().is_empty() {
            "saved".to_string()
        } else {
            save_result
        };
        let state = if save_result_is_success(&normalized) {
            WriterOperationLifecycleState::DurablySaved
        } else {
            WriterOperationLifecycleState::Rejected
        };
        let resolved_proposal_id =
            proposal_id.or_else(|| self.proposal_id_for_operation(&operation));
        let approval_source = resolved_proposal_id
            .as_deref()
            .and_then(|id| self.latest_approval_source_for_operation(id, &operation));
        let created_at = now_ms();
        self.push_operation_lifecycle(
            resolved_proposal_id.clone(),
            &operation,
            state.clone(),
            approval_source,
            Some(normalized.clone()),
            None,
            created_at,
        );
        if state == WriterOperationLifecycleState::DurablySaved {
            let report = self.record_saved_operation_post_write_diagnostics(
                resolved_proposal_id.as_deref(),
                &operation,
                saved_text.as_deref(),
                chapter_title.clone(),
                chapter_revision.clone(),
                created_at,
            );
            let observation_id = report
                .as_ref()
                .map(|report| report.observation_id.clone())
                .unwrap_or_else(|| format!("operation-save-{}", created_at));
            self.record_save_completed_run_event(
                SaveCompletedEventContext {
                    observation_id,
                    chapter_title: chapter_title.or_else(|| operation_chapter(&operation)),
                    chapter_revision,
                    save_result: normalized,
                },
                resolved_proposal_id,
                Some(operation_kind_label(&operation).to_string()),
                report.as_ref(),
                created_at,
            );
        }
        Ok(())
    }

    fn record_saved_operation_post_write_diagnostics(
        &mut self,
        source_proposal_id: Option<&str>,
        operation: &WriterOperation,
        saved_text: Option<&str>,
        chapter_title: Option<String>,
        chapter_revision: Option<String>,
        created_at: u64,
    ) -> Option<crate::writer_agent::post_write_diagnostics::WriterPostWriteDiagnosticReport> {
        let saved_text = saved_text.map(str::trim).filter(|text| !text.is_empty())?;
        let (paragraph, paragraph_offset, cursor) =
            operation_post_write_diagnostic_window(saved_text, operation)?;
        let chapter = chapter_title
            .or_else(|| operation_chapter(operation))
            .or_else(|| self.active_chapter.clone())
            .unwrap_or_else(|| "Chapter-1".to_string());
        let observation = WriterObservation {
            id: format!("operation-save-{}", created_at),
            created_at,
            source: observation::ObservationSource::ChapterSave,
            reason: observation::ObservationReason::Save,
            project_id: self.project_id.clone(),
            chapter_title: Some(chapter.clone()),
            chapter_revision,
            cursor: Some(observation::TextRange {
                from: cursor,
                to: cursor,
            }),
            selection: None,
            prefix: text_tail(saved_text, 3_000),
            suffix: String::new(),
            paragraph,
            full_text_digest: None,
            editor_dirty: false,
        };
        let diagnostics = self.diagnostics.diagnose(
            &observation.paragraph,
            paragraph_offset,
            &chapter,
            &self.project_id,
            &self.memory,
        );
        let mut report =
            crate::writer_agent::post_write_diagnostics::build_post_write_diagnostic_report(
                &observation,
                &diagnostics,
                created_at,
            );
        let mut source_refs = Vec::new();
        if let Some(source_proposal_id) = source_proposal_id {
            source_refs.push(format!("proposal:{}", source_proposal_id));
        }
        source_refs.push(format!("operation:{}", operation_kind_label(operation)));
        if let Some(scope) = operation_affected_scope(operation) {
            source_refs.push(scope);
        }
        extend_unique_source_refs(&mut report.source_refs, source_refs);
        self.record_post_write_diagnostic_report(&report);
        self.observations.push(observation.clone());
        let mut proposals = Vec::new();
        for diagnostic in diagnostics {
            proposals.push(diagnostic_to_proposal(
                diagnostic,
                &observation,
                &observation.id,
                &proposal_id(&self.session_id, self.proposal_counter),
            ));
            self.proposal_counter += 1;
        }
        self.register_proposals(proposals, &std::collections::HashMap::new());
        Some(report)
    }

    fn record_operation_result_lifecycle(
        &mut self,
        result: &OperationResult,
        approval: Option<&super::operation::OperationApproval>,
        save_result_override: Option<String>,
    ) {
        if !operation_is_write_capable(&result.operation) {
            return;
        }

        let proposal_id = approval
            .and_then(|context| context.proposal_id.clone())
            .or_else(|| self.proposal_id_for_operation(&result.operation));
        let approval_source = approval.map(|context| context.source.clone());
        let save_result = save_result_override.or_else(|| {
            result
                .error
                .as_ref()
                .map(|error| format!("{}:{}", error.code, error.message))
        });
        let state = if result.success {
            WriterOperationLifecycleState::Applied
        } else {
            WriterOperationLifecycleState::Rejected
        };
        self.push_operation_lifecycle(
            proposal_id.clone(),
            &result.operation,
            state,
            approval_source.clone(),
            save_result.clone(),
            None,
            now_ms(),
        );

        if result.success && operation_has_kernel_durable_save(&result.operation) {
            self.push_operation_lifecycle(
                proposal_id,
                &result.operation,
                WriterOperationLifecycleState::DurablySaved,
                approval_source,
                Some("kernel_write:ok".to_string()),
                None,
                now_ms(),
            );
        }
    }

    fn proposal_id_for_operation(&self, operation: &WriterOperation) -> Option<String> {
        let kind = operation_kind_label(operation);
        let scope = operation_affected_scope(operation);
        self.proposals.iter().rev().find_map(|proposal| {
            proposal
                .operations
                .iter()
                .any(|candidate| {
                    operation_kind_label(candidate) == kind
                        && operation_affected_scope(candidate) == scope
                })
                .then(|| proposal.id.clone())
        })
    }

    pub(super) fn lifecycle_has_state(
        &self,
        proposal_id: &str,
        operation: &WriterOperation,
        state: WriterOperationLifecycleState,
    ) -> bool {
        self.operation_lifecycle.iter().any(|trace| {
            trace.proposal_id.as_deref() == Some(proposal_id)
                && trace.operation_kind == operation_kind_label(operation)
                && trace.affected_scope == operation_affected_scope(operation)
                && trace.state == state
        })
    }

    fn latest_approval_source_for_operation(
        &self,
        proposal_id: &str,
        operation: &WriterOperation,
    ) -> Option<String> {
        let kind = operation_kind_label(operation);
        let scope = operation_affected_scope(operation);
        self.operation_lifecycle
            .iter()
            .rev()
            .find(|trace| {
                trace.proposal_id.as_deref() == Some(proposal_id)
                    && trace.operation_kind == kind
                    && trace.affected_scope == scope
                    && trace.state == WriterOperationLifecycleState::Approved
            })
            .and_then(|trace| trace.approval_source.clone())
    }
}

fn operation_chapter(operation: &WriterOperation) -> Option<String> {
    match operation {
        WriterOperation::TextInsert { chapter, .. }
        | WriterOperation::TextReplace { chapter, .. }
        | WriterOperation::TextAnnotate { chapter, .. } => Some(chapter.clone()),
        WriterOperation::PromiseResolve { chapter, .. }
        | WriterOperation::PromiseDefer { chapter, .. }
        | WriterOperation::PromiseAbandon { chapter, .. } => Some(chapter.clone()),
        WriterOperation::ChapterMissionUpsert { mission } => Some(mission.chapter_title.clone()),
        _ => None,
    }
}

fn operation_post_write_diagnostic_window(
    saved_text: &str,
    operation: &WriterOperation,
) -> Option<(String, usize, usize)> {
    let chars = saved_text.chars().collect::<Vec<_>>();
    if chars.is_empty() {
        return None;
    }
    let (target_start, target_end) = match operation {
        WriterOperation::TextInsert { at, text, .. } => {
            let start = (*at).min(chars.len());
            let end = start.saturating_add(text.chars().count()).min(chars.len());
            (start, end.max(start + 1).min(chars.len()))
        }
        WriterOperation::TextReplace { from, text, .. } => {
            let start = (*from).min(chars.len());
            let end = start.saturating_add(text.chars().count()).min(chars.len());
            (start, end.max(start + 1).min(chars.len()))
        }
        _ => {
            let end = chars.len().min(1_800);
            return Some((chars[..end].iter().collect(), 0, end));
        }
    };

    let mut start = target_start;
    while start > 0 && chars[start - 1] != '\n' && target_start.saturating_sub(start) < 900 {
        start -= 1;
    }
    let mut end = target_end;
    while end < chars.len() && chars[end] != '\n' && end.saturating_sub(target_end) < 900 {
        end += 1;
    }
    if start == end {
        return None;
    }
    let cursor = target_end.min(chars.len());
    Some((chars[start..end].iter().collect(), start, cursor))
}

fn text_tail(text: &str, max_chars: usize) -> String {
    let chars = text.chars().collect::<Vec<_>>();
    let start = chars.len().saturating_sub(max_chars);
    chars[start..].iter().collect()
}

fn extend_unique_source_refs(target: &mut Vec<String>, refs: Vec<String>) {
    for source_ref in refs {
        let source_ref = source_ref.trim();
        if source_ref.is_empty() || target.iter().any(|existing| existing == source_ref) {
            continue;
        }
        target.push(source_ref.to_string());
    }
}
