use super::helpers::{context_pack_built_reports, observation_source_refs, proposal_source_refs};
use super::WriterAgentKernel;
use crate::writer_agent::context::{AgentTask, WritingContextPack};
use crate::writer_agent::kernel::{
    attach_story_contract_quality_gate_to_task_packet, build_task_packet_for_observation,
    operation_affected_scope, operation_kind_label, snippet, WriterContextPackBuiltRunEvent,
    WriterOperationLifecycleState, WriterOperationLifecycleTrace, WriterRunEvent,
    WriterTaskPacketTrace,
};
use crate::writer_agent::observation::WriterObservation;
use crate::writer_agent::operation::WriterOperation;
use crate::writer_agent::proposal::AgentProposal;
use agent_harness_core::TaskPacket;

impl WriterAgentKernel {
    pub fn run_events(&self, limit: usize) -> Vec<WriterRunEvent> {
        let persisted = self
            .memory
            .list_run_events(&self.project_id, &self.session_id, limit)
            .unwrap_or_default()
            .into_iter()
            .map(|event| WriterRunEvent {
                seq: event.seq,
                ts_ms: event.ts_ms,
                project_id: event.project_id,
                session_id: event.session_id,
                task_id: event.task_id,
                event_type: event.event_type,
                source_refs: event.source_refs,
                data: event.data,
            })
            .collect::<Vec<_>>();
        if persisted.is_empty() {
            self.run_events.recent(limit)
        } else {
            persisted
        }
    }

    pub fn record_task_packet(
        &mut self,
        observation_id: impl Into<String>,
        task: impl Into<String>,
        packet: TaskPacket,
    ) -> Result<(), String> {
        packet.validate().map_err(|error| error.to_string())?;
        self.push_task_packet_trace(observation_id.into(), task.into(), packet);
        Ok(())
    }

    pub(in crate::writer_agent::kernel) fn record_task_packet_for(
        &mut self,
        task: AgentTask,
        observation: &WriterObservation,
        context_pack: &WritingContextPack,
        objective: &str,
        success_criteria: Vec<String>,
    ) {
        let mut packet = build_task_packet_for_observation(
            &self.project_id,
            &self.session_id,
            task.clone(),
            observation,
            context_pack,
            objective,
            success_criteria,
        );
        let (contract_quality, contract_quality_gaps) = self.contract_quality_with_gaps();
        attach_story_contract_quality_gate_to_task_packet(
            &mut packet,
            &task,
            contract_quality,
            &contract_quality_gaps,
        );
        if let Err(error) = packet.validate() {
            tracing::warn!(
                "Skipping invalid writer task packet for {:?}: {}",
                task,
                error
            );
            return;
        }
        self.push_task_packet_trace(observation.id.clone(), format!("{:?}", task), packet);
    }

    pub(in crate::writer_agent::kernel) fn push_task_packet_trace(
        &mut self,
        observation_id: String,
        task: String,
        packet: TaskPacket,
    ) {
        let coverage = packet.foundation_coverage();
        let trace = WriterTaskPacketTrace {
            id: packet.id.clone(),
            observation_id,
            task,
            objective: packet.objective.clone(),
            scope: packet.scope_label(),
            intent: packet.intent.as_ref().map(|intent| format!("{:?}", intent)),
            required_context_count: packet.required_context.len(),
            belief_count: packet.beliefs.len(),
            success_criteria_count: packet.success_criteria.len(),
            max_side_effect_level: format!("{:?}", packet.tool_policy.max_side_effect_level),
            feedback_checkpoint_count: packet.feedback.checkpoints.len(),
            foundation_complete: coverage.is_complete(),
            packet,
        };
        self.record_run_event(
            "task_packet_created",
            trace.packet.created_at_ms,
            Some(trace.id.clone()),
            vec![trace.observation_id.clone()],
            serde_json::json!({
                "id": trace.id,
                "observationId": trace.observation_id,
                "task": trace.task,
                "objective": trace.objective,
                "scope": trace.scope,
                "intent": trace.intent,
                "requiredContextCount": trace.required_context_count,
                "beliefCount": trace.belief_count,
                "successCriteriaCount": trace.success_criteria_count,
                "maxSideEffectLevel": trace.max_side_effect_level,
                "feedbackCheckpointCount": trace.feedback_checkpoint_count,
                "foundationComplete": trace.foundation_complete,
                "requiredSources": trace.packet.required_context
                    .iter()
                    .filter(|context| context.required)
                    .map(|context| context.source_type.clone())
                    .collect::<Vec<_>>(),
            }),
        );
        self.task_packets.push(trace);
    }

    pub(in crate::writer_agent::kernel) fn push_operation_lifecycle(
        &mut self,
        proposal_id: Option<String>,
        operation: &WriterOperation,
        state: WriterOperationLifecycleState,
        approval_source: Option<String>,
        save_result: Option<String>,
        feedback_result: Option<String>,
        created_at: u64,
    ) {
        let source_task = proposal_id
            .as_deref()
            .and_then(|id| self.proposals.iter().find(|proposal| proposal.id == id))
            .map(|proposal| format!("{:?}", proposal.kind));
        let operation_kind = operation_kind_label(operation).to_string();
        let affected_scope = operation_affected_scope(operation);
        self.operation_lifecycle
            .push(WriterOperationLifecycleTrace {
                source_task: source_task.clone(),
                proposal_id: proposal_id.clone(),
                operation_kind: operation_kind.clone(),
                approval_source: approval_source.clone(),
                affected_scope: affected_scope.clone(),
                state: state.clone(),
                save_result: save_result.clone(),
                feedback_result: feedback_result.clone(),
                created_at,
            });
        self.record_run_event(
            "operation_lifecycle",
            created_at,
            proposal_id.clone(),
            proposal_id.iter().cloned().collect(),
            serde_json::json!({
                "proposalId": proposal_id,
                "operationKind": operation_kind,
                "sourceTask": source_task,
                "approvalSource": approval_source,
                "affectedScope": affected_scope,
                "state": state,
                "saveResult": save_result,
                "feedbackResult": feedback_result,
            }),
        );
    }

    pub(in crate::writer_agent::kernel) fn record_observation_run_event(
        &mut self,
        observation: &WriterObservation,
    ) {
        self.record_run_event(
            "observation",
            observation.created_at,
            Some(observation.id.clone()),
            observation_source_refs(observation),
            serde_json::json!({
                "id": observation.id,
                "source": observation.source,
                "reason": observation.reason,
                "chapterTitle": observation.chapter_title,
                "chapterRevision": observation.chapter_revision,
                "paragraphSnippet": snippet(&observation.paragraph, 160),
                "hasSelection": observation.has_selection(),
                "editorDirty": observation.editor_dirty,
            }),
        );
    }

    pub(in crate::writer_agent::kernel) fn record_proposal_run_event(
        &mut self,
        proposal: &AgentProposal,
        created_at: u64,
    ) {
        let observation = self
            .observations
            .iter()
            .find(|observation| observation.id == proposal.observation_id);
        self.record_run_event(
            "proposal_created",
            created_at,
            Some(proposal.id.clone()),
            proposal_source_refs(proposal),
            serde_json::json!({
                "id": proposal.id,
                "observationId": proposal.observation_id,
                "kind": proposal.kind,
                "priority": proposal.priority,
                "confidence": proposal.confidence,
                "observationSource": observation.map(|observation| observation.source.clone()),
                "observationReason": observation.map(|observation| observation.reason.clone()),
                "operationCount": proposal.operations.len(),
                "operationKinds": proposal.operations
                    .iter()
                    .map(|operation| operation_kind_label(operation).to_string())
                    .collect::<Vec<_>>(),
                "evidenceCount": proposal.evidence.len(),
                "previewSnippet": snippet(&proposal.preview, 160),
                "expiresAt": proposal.expires_at,
            }),
        );
    }

    pub(in crate::writer_agent::kernel) fn record_memory_candidate_created_run_event(
        &mut self,
        proposal: &AgentProposal,
        slot: String,
        created_at: u64,
    ) {
        let operation_kinds = proposal
            .operations
            .iter()
            .map(|operation| operation_kind_label(operation).to_string())
            .collect::<Vec<_>>();
        self.record_run_event(
            "memory_candidate_created",
            created_at,
            Some(proposal.id.clone()),
            proposal_source_refs(proposal),
            serde_json::json!({
                "proposalId": proposal.id,
                "observationId": proposal.observation_id,
                "kind": proposal.kind,
                "slot": slot,
                "operationKinds": operation_kinds,
                "evidenceCount": proposal.evidence.len(),
                "previewSnippet": snippet(&proposal.preview, 160),
                "requiresAuthorReview": true,
                "writesLedgerImmediately": false,
            }),
        );
    }

    pub(in crate::writer_agent::kernel) fn record_context_pack_built_run_event(
        &mut self,
        observation: &WriterObservation,
        context_pack: &WritingContextPack,
        created_at: u64,
    ) {
        let task_id = format!(
            "{}:{}:{:?}:context_pack",
            self.session_id, observation.id, context_pack.task
        );
        let source_reports = context_pack_built_reports(context_pack);
        let truncated_source_count = source_reports
            .iter()
            .filter(|report| report.truncated)
            .count();
        let mut source_refs = observation_source_refs(observation);
        source_refs.extend(
            context_pack
                .sources
                .iter()
                .map(|source| format!("context_source:{:?}", source.source)),
        );

        self.record_context_pack_built_event(
            WriterContextPackBuiltRunEvent {
                task_id: task_id.clone(),
                task: format!("{:?}", context_pack.task),
                source_count: context_pack.sources.len(),
                total_chars: context_pack.total_chars,
                budget_limit: context_pack.budget_limit,
                wasted: context_pack.budget_report.wasted,
                truncated_source_count,
                source_reports,
            },
            source_refs,
            created_at,
        );
    }
}
