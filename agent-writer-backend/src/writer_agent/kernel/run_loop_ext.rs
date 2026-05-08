use agent_harness_core::agent_loop::{EventCallback, ProviderCallContext, ProviderCallGuard};
use agent_harness_core::provider::{LlmMessage, Provider};
use agent_harness_core::{AgentLoop, EffectiveToolInventory, TaskPacket, ToolHandler};

use crate::writer_agent::context::AgentTask;
use crate::writer_agent::observation::WriterObservation;
use crate::writer_agent::operation::WriterOperation;
use crate::writer_agent::proposal::AgentProposal;
use crate::writer_agent::provider_budget::{
    evaluate_provider_budget, WriterProviderBudgetReport, WriterProviderBudgetRequest,
    WriterProviderBudgetTask,
};
use crate::writer_agent::task_receipt::WriterTaskReceipt;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum WriterAgentTask {
    ManualRequest,
    InlineRewrite,
    GhostWriting,
    ChapterGeneration,
    PlanningReview,
    ContinuityDiagnostic,
    CanonMaintenance,
    ProposalEvaluation,
}

impl WriterAgentTask {
    pub fn as_agent_task(&self) -> AgentTask {
        match self {
            WriterAgentTask::ManualRequest => AgentTask::ManualRequest,
            WriterAgentTask::InlineRewrite => AgentTask::InlineRewrite,
            WriterAgentTask::GhostWriting => AgentTask::GhostWriting,
            WriterAgentTask::ChapterGeneration => AgentTask::ChapterGeneration,
            WriterAgentTask::PlanningReview => AgentTask::PlanningReview,
            WriterAgentTask::ContinuityDiagnostic => AgentTask::ContinuityDiagnostic,
            WriterAgentTask::CanonMaintenance => AgentTask::CanonMaintenance,
            WriterAgentTask::ProposalEvaluation => AgentTask::ProposalEvaluation,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum WriterAgentApprovalMode {
    ReadOnly,
    SurfaceProposals,
    ApprovedWrites,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum WriterAgentStreamMode {
    None,
    Text,
}

#[derive(Debug, Clone)]
pub struct WriterAgentRunRequest {
    pub task: WriterAgentTask,
    pub observation: WriterObservation,
    pub user_instruction: String,
    pub frontend_state: WriterAgentFrontendState,
    pub approval_mode: WriterAgentApprovalMode,
    pub stream_mode: WriterAgentStreamMode,
    pub manual_history: Vec<LlmMessage>,
}

#[derive(Debug, Clone, Default)]
pub struct WriterAgentFrontendState {
    pub truncated_context: String,
    pub paragraph: String,
    pub selected_text: String,
    pub memory_context: String,
    pub has_lore: bool,
    pub has_outline: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WriterAgentRunResult {
    pub answer: String,
    pub proposals: Vec<AgentProposal>,
    pub operations: Vec<WriterOperation>,
    pub task_packet: TaskPacket,
    pub task_receipt: Option<WriterTaskReceipt>,
    pub context_pack_summary: WriterAgentContextPackSummary,
    pub tool_inventory: EffectiveToolInventory,
    pub trace_refs: Vec<String>,
    pub source_refs: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WriterAgentContextPackSummary {
    pub task: AgentTask,
    pub source_count: usize,
    pub total_chars: usize,
    pub budget_limit: usize,
    pub source_refs: Vec<String>,
}

pub struct WriterAgentPreparedRun<P: Provider, H: ToolHandler> {
    pub(crate) request: WriterAgentRunRequest,
    pub(crate) agent: AgentLoop<P, H>,
    pub(crate) proposals: Vec<AgentProposal>,
    pub(crate) operations: Vec<WriterOperation>,
    pub(crate) task_packet: TaskPacket,
    pub(crate) task_receipt: Option<WriterTaskReceipt>,
    pub(crate) context_pack_summary: WriterAgentContextPackSummary,
    pub(crate) tool_inventory: EffectiveToolInventory,
    pub(crate) source_refs: Vec<String>,
    pub(crate) trace_refs: Vec<String>,
}

impl<P, H> WriterAgentPreparedRun<P, H>
where
    P: Provider,
    H: ToolHandler,
{
    const FIRST_ROUND_OUTPUT_TOKENS: u64 = 4_096;

    pub fn set_event_callback(&mut self, cb: EventCallback) {
        self.agent.set_event_callback(cb);
    }

    pub fn request(&self) -> &WriterAgentRunRequest {
        &self.request
    }

    pub fn proposals(&self) -> &[AgentProposal] {
        &self.proposals
    }

    pub fn context_pack_summary(&self) -> &WriterAgentContextPackSummary {
        &self.context_pack_summary
    }

    pub fn tool_inventory(&self) -> &EffectiveToolInventory {
        &self.tool_inventory
    }

    pub fn task_packet(&self) -> &TaskPacket {
        &self.task_packet
    }

    pub fn system_prompt(&self) -> &str {
        &self.agent.config.system_prompt
    }

    pub fn source_refs(&self) -> &[String] {
        &self.source_refs
    }

    pub fn first_round_provider_budget(
        &self,
        task: WriterProviderBudgetTask,
        model: impl Into<String>,
    ) -> WriterProviderBudgetReport {
        self.provider_budget_from_estimate(
            task,
            model,
            self.first_round_estimated_input_tokens(),
            Self::FIRST_ROUND_OUTPUT_TOKENS,
        )
    }

    pub fn first_round_estimated_input_tokens(&self) -> u64 {
        let mut messages = self.agent.messages.clone();
        messages.push(LlmMessage {
            role: "user".to_string(),
            content: Some(self.request.user_instruction.clone()),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        });

        self.agent.provider.estimate_tokens(&messages)
            + (self.agent.config.system_prompt.chars().count() as u64 / 3)
            + self.tool_inventory.allowed.len() as u64 * 256
    }

    pub fn provider_budget_from_estimate(
        &self,
        task: WriterProviderBudgetTask,
        model: impl Into<String>,
        estimated_input_tokens: u64,
        requested_output_tokens: u64,
    ) -> WriterProviderBudgetReport {
        evaluate_provider_budget(WriterProviderBudgetRequest::new(
            task,
            model,
            estimated_input_tokens,
            requested_output_tokens,
        ))
    }

    pub fn set_provider_call_guard(&mut self, guard: ProviderCallGuard) {
        self.agent.set_provider_call_guard(guard);
    }

    pub fn provider_budget_from_call_context(
        task: WriterProviderBudgetTask,
        context: &ProviderCallContext,
    ) -> WriterProviderBudgetReport {
        evaluate_provider_budget(WriterProviderBudgetRequest::new(
            task,
            context.model.clone(),
            context.estimated_input_tokens,
            context.requested_output_tokens,
        ))
    }

    pub async fn run(mut self) -> Result<WriterAgentRunResult, String> {
        self.agent
            .add_user_message(self.request.user_instruction.clone());
        let answer = self
            .agent
            .run(
                &self.request.user_instruction,
                self.request.frontend_state.has_lore,
                self.request.frontend_state.has_outline,
            )
            .await?;
        self.trace_refs.push(self.task_packet.id.clone());
        Ok(WriterAgentRunResult {
            answer,
            proposals: self.proposals,
            operations: self.operations,
            task_packet: self.task_packet,
            task_receipt: self.task_receipt,
            context_pack_summary: self.context_pack_summary,
            tool_inventory: self.tool_inventory,
            trace_refs: self.trace_refs,
            source_refs: self.source_refs,
        })
    }
}
