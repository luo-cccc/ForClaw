use super::helpers::{chapter_context_pack_built_reports, json_object_keys};
use super::ModelStartedEventContext;
use super::{SaveCompletedEventContext, WriterAgentKernel};
use crate::writer_agent::feedback::ProposalFeedback;
use crate::writer_agent::kernel::{
    approval_sources, operation_affected_scope, operation_kind_label, WriterAgentTask,
    WriterContextPackBuiltRunEvent,
};
use crate::writer_agent::memory;
use crate::writer_agent::operation::{OperationApproval, WriterOperation};

impl WriterAgentKernel {
    pub fn record_chapter_context_pack_built_run_event(
        &mut self,
        context: &crate::chapter_generation::BuiltChapterContext,
        created_at: u64,
    ) {
        let task_id = format!(
            "{}:{}:ChapterGeneration:context_pack",
            self.session_id, context.request_id
        );
        let source_reports = chapter_context_pack_built_reports(context);
        let mut source_refs = vec![
            format!("receipt:{}", context.receipt.task_id),
            format!("chapter:{}", context.target.title),
            format!("revision:{}", context.base_revision),
        ];
        source_refs.extend(
            context
                .sources
                .iter()
                .filter(|source| source.included_chars > 0)
                .map(|source| format!("{}:{}", source.source_type, source.id)),
        );
        self.record_context_pack_built_event(
            WriterContextPackBuiltRunEvent {
                task_id: task_id.clone(),
                task: "ChapterGeneration".to_string(),
                source_count: context.budget.source_count,
                total_chars: context.budget.included_chars,
                budget_limit: context.budget.max_chars,
                wasted: context
                    .budget
                    .max_chars
                    .saturating_sub(context.budget.included_chars),
                truncated_source_count: context.budget.truncated_source_count,
                source_reports,
            },
            source_refs,
            created_at,
        );
    }

    pub(in crate::writer_agent::kernel) fn record_context_pack_built_event(
        &mut self,
        event: WriterContextPackBuiltRunEvent,
        source_refs: Vec<String>,
        created_at: u64,
    ) {
        self.record_run_event(
            "context_pack_built",
            created_at,
            Some(event.task_id.clone()),
            source_refs,
            serde_json::json!(event),
        );
    }

    pub(in crate::writer_agent::kernel) fn record_approval_decided_run_event(
        &mut self,
        operation: &WriterOperation,
        approval: Option<&OperationApproval>,
        approved: bool,
        reason: &str,
        created_at: u64,
    ) {
        let proposal_id = approval.and_then(|context| context.proposal_id.clone());
        let mut source_refs = Vec::new();
        if let Some(proposal_id) = proposal_id.as_ref() {
            source_refs.push(format!("proposal:{}", proposal_id));
        }
        if let Some(scope) = operation_affected_scope(operation) {
            source_refs.push(scope);
        }
        if let Some(context) = approval {
            source_refs.extend(approval_sources(context));
        }
        self.record_run_event(
            "approval_decided",
            created_at,
            proposal_id.clone(),
            source_refs,
            serde_json::json!({
                "proposalId": proposal_id,
                "operationKind": operation_kind_label(operation),
                "affectedScope": operation_affected_scope(operation),
                "decision": if approved { "approved" } else { "rejected" },
                "approvalSource": approval.map(|context| context.source.clone()),
                "actor": approval.map(|context| context.actor.clone()),
                "surfacedToUser": approval.map(|context| context.surfaced_to_user).unwrap_or(false),
                "reason": reason,
            }),
        );
    }

    pub(in crate::writer_agent::kernel) fn record_feedback_run_event(
        &mut self,
        feedback: &ProposalFeedback,
        feedback_result: Option<&str>,
    ) {
        self.record_run_event(
            "feedback_recorded",
            feedback.created_at,
            Some(feedback.proposal_id.clone()),
            vec![feedback.proposal_id.clone()],
            serde_json::json!({
                "proposalId": feedback.proposal_id,
                "action": feedback.action,
                "reason": feedback.reason,
                "hasFinalText": feedback.final_text.as_ref().is_some_and(|text| !text.trim().is_empty()),
                "feedbackResult": feedback_result,
            }),
        );
    }

    pub fn record_failure_evidence_bundle(
        &mut self,
        bundle: &crate::writer_agent::task_receipt::WriterFailureEvidenceBundle,
    ) {
        self.record_run_event(
            "error",
            bundle.created_at_ms,
            bundle.task_id.clone(),
            bundle.evidence_refs.clone(),
            serde_json::json!(bundle),
        );
    }

    pub(in crate::writer_agent::kernel) fn record_metacognitive_gate_block_run_event(
        &mut self,
        task: &WriterAgentTask,
        task_id: impl Into<String>,
        reason: &str,
        snapshot: &crate::writer_agent::metacognition::WriterMetacognitiveSnapshot,
        created_at: u64,
    ) {
        let task_id = task_id.into();
        self.record_run_event(
            "metacognitive_gate_blocked",
            created_at,
            Some(task_id.clone()),
            vec![format!("task:{:?}", task), "metacognitive_gate".to_string()],
            serde_json::json!({
                "taskId": task_id,
                "task": task,
                "reason": reason,
                "riskLevel": snapshot.risk_level,
                "recommendedAction": snapshot.recommended_action,
                "confidence": snapshot.confidence,
                "summary": snapshot.summary,
                "reasons": snapshot.reasons,
                "remediation": snapshot.remediation,
            }),
        );
    }

    pub(in crate::writer_agent::kernel) fn record_task_receipt_run_event(
        &mut self,
        receipt: &crate::writer_agent::task_receipt::WriterTaskReceipt,
    ) {
        let mut source_refs = vec![
            format!("receipt:{}", receipt.task_id),
            format!("task_kind:{}", receipt.task_kind),
        ];
        if let Some(chapter) = receipt.chapter.as_ref() {
            source_refs.push(format!("chapter:{}", chapter));
        }
        if let Some(revision) = receipt.base_revision.as_ref() {
            source_refs.push(format!("revision:{}", revision));
        }
        source_refs.extend(receipt.source_refs.iter().cloned());
        self.record_run_event(
            "task_receipt",
            receipt.created_at_ms,
            Some(receipt.task_id.clone()),
            source_refs,
            serde_json::json!(receipt),
        );
    }

    pub(in crate::writer_agent::kernel) fn record_task_artifact_run_event(
        &mut self,
        artifact: &crate::writer_agent::task_receipt::WriterTaskArtifact,
    ) {
        self.record_run_event(
            "task_artifact",
            artifact.created_at_ms,
            Some(artifact.task_id.clone()),
            artifact.source_refs.clone(),
            serde_json::json!(artifact),
        );
    }

    pub(in crate::writer_agent::kernel) fn record_post_write_diagnostic_report(
        &mut self,
        report: &crate::writer_agent::post_write_diagnostics::WriterPostWriteDiagnosticReport,
    ) {
        self.record_run_event(
            "post_write_diagnostics",
            report.created_at_ms,
            Some(report.observation_id.clone()),
            report.source_refs.clone(),
            serde_json::json!(report),
        );
    }

    pub(in crate::writer_agent::kernel) fn record_save_completed_run_event(
        &mut self,
        ctx: SaveCompletedEventContext,
        proposal_id: Option<String>,
        operation_kind: Option<String>,
        report: Option<
            &crate::writer_agent::post_write_diagnostics::WriterPostWriteDiagnosticReport,
        >,
        created_at_ms: u64,
    ) {
        let observation_id = ctx.observation_id;
        let save_result = ctx.save_result;
        let chapter_title = ctx.chapter_title;
        let chapter_revision = ctx.chapter_revision;
        let mut source_refs = vec![observation_id.clone()];
        if let Some(chapter) = chapter_title.as_ref() {
            source_refs.push(format!("chapter:{}", chapter));
        }
        if let Some(revision) = chapter_revision.as_ref() {
            source_refs.push(format!("revision:{}", revision));
        }
        if let Some(proposal_id) = proposal_id.as_ref() {
            source_refs.push(format!("proposal:{}", proposal_id));
        }
        if let Some(operation_kind) = operation_kind.as_ref() {
            source_refs.push(format!("operation:{}", operation_kind));
        }
        if let Some(report) = report {
            source_refs.extend(report.source_refs.iter().cloned());
        }

        self.record_run_event(
            "save_completed",
            created_at_ms,
            Some(observation_id.clone()),
            source_refs,
            serde_json::json!({
                "observationId": observation_id,
                "chapterTitle": chapter_title,
                "chapterRevision": chapter_revision,
                "saveResult": save_result,
                "proposalId": proposal_id,
                "operationKind": operation_kind,
                "postWriteReportId": report.map(|report| report.observation_id.clone()),
                "diagnosticTotalCount": report.map(|report| report.total_count).unwrap_or(0),
                "diagnosticErrorCount": report.map(|report| report.error_count).unwrap_or(0),
                "diagnosticWarningCount": report.map(|report| report.warning_count).unwrap_or(0),
            }),
        );
    }

    pub fn record_provider_budget_report(
        &mut self,
        task_id: impl Into<String>,
        report: &crate::writer_agent::provider_budget::WriterProviderBudgetReport,
        source_refs: Vec<String>,
        created_at_ms: u64,
    ) {
        let task_id = task_id.into();
        self.record_run_event(
            "provider_budget",
            created_at_ms,
            Some(task_id.clone()),
            source_refs,
            serde_json::json!({
                "taskId": task_id,
                "task": report.task,
                "model": report.model,
                "decision": report.decision,
                "approvalRequired": report.approval_required,
                "estimatedTotalTokens": report.estimated_total_tokens,
                "estimatedCostMicros": report.estimated_cost_micros,
                "reasons": &report.reasons,
                "remediation": &report.remediation,
                "providerBudget": report,
            }),
        );
    }

    pub fn record_model_started_run_event(
        &mut self,
        ctx: ModelStartedEventContext,
        source_refs: Vec<String>,
        report: Option<&crate::writer_agent::provider_budget::WriterProviderBudgetReport>,
        created_at_ms: u64,
    ) {
        let task_id = ctx.task_id;
        let model = ctx.model;
        let provider = ctx.provider;
        self.record_run_event(
            "model_started",
            created_at_ms,
            Some(task_id.clone()),
            source_refs,
            serde_json::json!({
                "taskId": task_id,
                "task": ctx.task,
                "model": model,
                "provider": provider,
                "stream": ctx.stream,
                "estimatedInputTokens": report.map(|report| report.estimated_input_tokens),
                "requestedOutputTokens": report.map(|report| report.requested_output_tokens),
                "estimatedTotalTokens": report.map(|report| report.estimated_total_tokens),
                "estimatedCostMicros": report.map(|report| report.estimated_cost_micros),
                "budgetDecision": report.map(|report| report.decision),
                "approvalRequired": report.map(|report| report.approval_required).unwrap_or(false),
            }),
        );
    }

    pub fn record_tool_called_run_event(
        &mut self,
        task_id: impl Into<String>,
        tool_name: impl Into<String>,
        phase: impl Into<String>,
        input: Option<&serde_json::Value>,
        result: Option<&agent_harness_core::ToolExecution>,
        source_refs: Vec<String>,
        created_at_ms: u64,
    ) {
        let task_id = task_id.into();
        let tool_name = tool_name.into();
        let phase = phase.into();
        let input_keys = input.map(json_object_keys).unwrap_or_default();
        let input_bytes = input
            .and_then(|value| serde_json::to_vec(value).ok())
            .map(|bytes| bytes.len())
            .unwrap_or(0);
        let output_bytes = result
            .and_then(|execution| serde_json::to_vec(&execution.output).ok())
            .map(|bytes| bytes.len())
            .unwrap_or(0);
        let remediation_codes = result
            .map(|execution| {
                execution
                    .remediation
                    .iter()
                    .map(|item| item.code.clone())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let success = result.map(|execution| execution.error.is_none());
        self.record_run_event(
            "tool_called",
            created_at_ms,
            Some(task_id.clone()),
            source_refs,
            serde_json::json!({
                "taskId": task_id,
                "toolName": tool_name,
                "phase": phase,
                "success": success,
                "durationMs": result.map(|execution| execution.duration_ms),
                "inputKeys": input_keys,
                "inputBytes": input_bytes,
                "outputBytes": output_bytes,
                "error": result.and_then(|execution| execution.error.clone()),
                "remediationCodes": remediation_codes,
            }),
        );
    }

    pub fn record_subtask_started_run_event(
        &mut self,
        payload: &crate::writer_agent::research_subtask::WriterSubtaskRunEventPayload,
        created_at_ms: u64,
    ) {
        self.record_subtask_run_event("subtask_started", payload, created_at_ms);
    }

    pub fn record_subtask_completed_run_event(
        &mut self,
        payload: &crate::writer_agent::research_subtask::WriterSubtaskRunEventPayload,
        created_at_ms: u64,
    ) {
        self.record_subtask_run_event("subtask_completed", payload, created_at_ms);
    }

    fn record_subtask_run_event(
        &mut self,
        event_type: &str,
        payload: &crate::writer_agent::research_subtask::WriterSubtaskRunEventPayload,
        created_at_ms: u64,
    ) {
        let source_refs = payload
            .evidence_refs
            .iter()
            .cloned()
            .chain(payload.artifact_refs.iter().cloned())
            .collect::<Vec<_>>();
        self.record_run_event(
            event_type,
            created_at_ms,
            Some(payload.subtask_id.clone()),
            source_refs,
            serde_json::to_value(payload).unwrap_or_else(|_| serde_json::json!({})),
        );
    }

    pub(in crate::writer_agent::kernel) fn record_run_event(
        &mut self,
        event_type: &str,
        ts_ms: u64,
        task_id: Option<String>,
        source_refs: Vec<String>,
        data: serde_json::Value,
    ) {
        let event = self.run_events.append(
            &self.project_id,
            &self.session_id,
            format!("writer.{}", event_type),
            ts_ms,
            task_id,
            source_refs,
            data,
        );
        self.memory
            .record_run_event(&memory::RunEventSummary {
                seq: event.seq,
                project_id: event.project_id,
                session_id: event.session_id,
                task_id: event.task_id,
                event_type: event.event_type,
                source_refs: event.source_refs,
                data: event.data,
                ts_ms: event.ts_ms,
            })
            .ok();
    }

    pub(in crate::writer_agent::kernel) fn record_story_impact_radius_run_event(
        &mut self,
        observation_id: &str,
        radius: &crate::writer_agent::story_impact::WriterStoryImpactRadius,
        budget: &crate::writer_agent::story_impact::StoryImpactBudgetReport,
        ts_ms: u64,
    ) {
        let mut source_refs: Vec<String> = vec![observation_id.to_string()];
        source_refs.extend(radius.impacted_sources.iter().cloned());
        source_refs.sort();
        source_refs.dedup();
        let node_kinds: Vec<&str> = radius
            .impacted_nodes
            .iter()
            .map(|n| n.kind.as_str())
            .collect();
        self.record_run_event(
            "story_impact_radius_built",
            ts_ms,
            Some(observation_id.to_string()),
            source_refs,
            serde_json::json!({
                "observationId": observation_id,
                "seedCount": radius.seed_nodes.len(),
                "impactedNodeCount": radius.impacted_nodes.len(),
                "edgeCount": radius.edges.len(),
                "risk": format!("{:?}", radius.risk),
                "truncated": radius.truncated,
                "truncatedNodeCount": budget.truncated_node_count,
                "budgetLimit": budget.budget_limit,
                "providedChars": budget.provided_chars,
                "requestedChars": budget.requested_chars,
                "impactedNodeKinds": node_kinds,
                "reasons": budget.reasons,
            }),
        );
    }
}
