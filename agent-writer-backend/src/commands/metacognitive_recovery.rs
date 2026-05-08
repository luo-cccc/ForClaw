//! Metacognitive recovery Tauri command.
//!
//! This command intentionally exposes only read-only recovery tasks. It does
//! not reuse the open-ended manual ask entrypoint, so metacognitive blocks get
//! a narrow path into Planning Review or Continuity Diagnostic runs.

use agent_harness_core::provider::openai_compat::OpenAiCompatProvider;
use agent_harness_core::AgentLoopEvent;
use tauri::{Emitter, Manager};

use crate::{
    agent_runtime, events, llm_runtime, storage, tool_bridge, writer_agent, AppState,
    AskAgentContext, HarnessState,
};
use writer_agent::kernel::{
    ModelStartedEventContext, WriterAgentApprovalMode, WriterAgentFrontendState,
    WriterAgentRunRequest, WriterAgentRunResult, WriterAgentStreamMode, WriterAgentTask,
};
use writer_agent::provider_budget::{
    apply_provider_budget_approval, WriterProviderBudgetDecision, WriterProviderBudgetReport,
    WriterProviderBudgetTask,
};

const METACOGNITIVE_RECOVERY_PROVIDER_BUDGET_ERROR: &str =
    "METACOGNITIVE_RECOVERY_PROVIDER_BUDGET_APPROVAL_REQUIRED";

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetacognitiveRecoveryPayload {
    pub action: MetacognitiveRecoveryAction,
    pub instruction: Option<String>,
    pub context: String,
    pub paragraph: String,
    pub selected_text: String,
    pub context_payload: Option<AskAgentContext>,
}

#[derive(Debug, Clone, serde::Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum MetacognitiveRecoveryAction {
    PlanningReview,
    ContinuityDiagnostic,
}

impl MetacognitiveRecoveryAction {
    fn task(&self) -> WriterAgentTask {
        match self {
            MetacognitiveRecoveryAction::PlanningReview => WriterAgentTask::PlanningReview,
            MetacognitiveRecoveryAction::ContinuityDiagnostic => {
                WriterAgentTask::ContinuityDiagnostic
            }
        }
    }

    fn default_instruction(&self) -> &'static str {
        match self {
            MetacognitiveRecoveryAction::PlanningReview => {
                "Metacognitive gate requested recovery. Run a read-only Planning Review: rebuild the current context picture, identify missing evidence, list risks, propose candidate next actions, and ask any author-confirmation questions. Do not draft manuscript prose or mutate project memory."
            }
            MetacognitiveRecoveryAction::ContinuityDiagnostic => {
                "Metacognitive gate requested recovery. Run a read-only Continuity Diagnostic: inspect canon, chapter mission, promise, save, and context-pressure risks; cite evidence; produce a diagnostic_report only. Do not draft manuscript prose or mutate project memory."
            }
        }
    }

    fn label(&self) -> &'static str {
        match self {
            MetacognitiveRecoveryAction::PlanningReview => "Planning Review",
            MetacognitiveRecoveryAction::ContinuityDiagnostic => "Continuity Diagnostic",
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MetacognitiveRecoveryRunResult {
    pub action: MetacognitiveRecoveryActionResult,
    pub answer: String,
    pub task_packet: agent_harness_core::TaskPacket,
    pub task_receipt: Option<writer_agent::task_receipt::WriterTaskReceipt>,
    pub context_pack_summary: writer_agent::kernel::WriterAgentContextPackSummary,
    pub trace_refs: Vec<String>,
    pub source_refs: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MetacognitiveRecoveryActionResult {
    PlanningReview,
    ContinuityDiagnostic,
}

impl From<&MetacognitiveRecoveryAction> for MetacognitiveRecoveryActionResult {
    fn from(value: &MetacognitiveRecoveryAction) -> Self {
        match value {
            MetacognitiveRecoveryAction::PlanningReview => Self::PlanningReview,
            MetacognitiveRecoveryAction::ContinuityDiagnostic => Self::ContinuityDiagnostic,
        }
    }
}

#[tauri::command]
pub async fn run_metacognitive_recovery(
    app: tauri::AppHandle,
    payload: MetacognitiveRecoveryPayload,
) -> Result<MetacognitiveRecoveryRunResult, String> {
    let api_key = crate::require_api_key()?;
    let settings = llm_runtime::settings(api_key);
    let model = settings.model.clone();
    let state = app.state::<AppState>();
    let project_id = storage::active_project_id(&app)?;
    let message = payload
        .instruction
        .clone()
        .filter(|instruction| !instruction.trim().is_empty())
        .unwrap_or_else(|| payload.action.default_instruction().to_string());
    let observation = crate::build_manual_writer_observation(
        &message,
        &payload.context,
        &payload.paragraph,
        &payload.selected_text,
        payload.context_payload.as_ref(),
        &project_id,
    );
    let request_id = recovery_request_id(payload.context_payload.as_ref());
    let recovery_task = payload.action.task();

    let mut prepared_run = {
        let mut kernel = state.writer_kernel.lock().map_err(|e| e.to_string())?;
        crate::refresh_kernel_canon_from_lorebook(&app, &mut kernel);
        let request = WriterAgentRunRequest {
            task: recovery_task,
            observation: observation.clone(),
            user_instruction: message.clone(),
            frontend_state: WriterAgentFrontendState {
                truncated_context: agent_harness_core::truncate_context(&payload.context, 2000)
                    .to_string(),
                paragraph: payload.paragraph.clone(),
                selected_text: payload.selected_text.clone(),
                memory_context: crate::build_context_injection(&app, &message),
                has_lore: storage::load_lorebook(&app)
                    .map(|lore| !lore.is_empty())
                    .unwrap_or(false),
                has_outline: storage::load_outline(&app)
                    .map(|outline| !outline.is_empty())
                    .unwrap_or(false),
            },
            approval_mode: WriterAgentApprovalMode::ReadOnly,
            stream_mode: WriterAgentStreamMode::Text,
            manual_history: Vec::new(),
        };
        let provider = std::sync::Arc::new(OpenAiCompatProvider::new(
            &settings.api_base,
            &settings.api_key,
            &settings.model,
        ));
        kernel.prepare_task_run(
            request,
            provider,
            tool_bridge::TauriToolBridge { app: app.clone() },
            &model,
        )?
    };

    let preflight_estimated_input_tokens = prepared_run.first_round_estimated_input_tokens();
    let provider_budget_approval = payload
        .context_payload
        .as_ref()
        .and_then(|payload| payload.provider_budget_approval.clone());
    let budget_report = apply_provider_budget_approval(
        prepared_run.provider_budget_from_estimate(
            WriterProviderBudgetTask::MetacognitiveRecovery,
            model.clone(),
            preflight_estimated_input_tokens,
            4_096,
        ),
        payload
            .context_payload
            .as_ref()
            .and_then(|payload| payload.provider_budget_approval.as_ref()),
    );
    let budget_task_id = format!("metacognitive-recovery-{}", request_id);
    let budget_source_refs =
        recovery_budget_source_refs(&request_id, &observation, &budget_report, &payload.action);
    let budget_created_at = agent_runtime::now_ms();
    record_recovery_provider_budget_report(
        &app,
        &budget_task_id,
        &budget_report,
        budget_source_refs.clone(),
        budget_created_at,
    );
    if budget_report.approval_required {
        record_recovery_provider_budget_failure(
            &app,
            budget_task_id,
            budget_source_refs,
            budget_report.clone(),
            budget_created_at,
            &payload.action,
        );
        emit_recovery_provider_budget_error(&app, &budget_report, &payload.action);
        set_harness_idle(&state)?;
        return Err(METACOGNITIVE_RECOVERY_PROVIDER_BUDGET_ERROR.to_string());
    }
    install_recovery_provider_budget_guard(
        &mut prepared_run,
        app.clone(),
        request_id.clone(),
        observation.clone(),
        payload.action.clone(),
        provider_budget_approval,
        preflight_estimated_input_tokens,
    );
    prepared_run
        .agent
        .executor
        .set_audit_sink(tool_bridge::writer_tool_audit_sink(
            app.clone(),
            budget_task_id,
            vec!["metacognitive_recovery".to_string()],
        ));

    let app_handle = app.clone();
    prepared_run.set_event_callback(std::sync::Arc::new(move |event| match event {
        AgentLoopEvent::TextChunk { content } => {
            let _ = app_handle.emit(
                events::METACOGNITIVE_RECOVERY,
                serde_json::json!({ "phase": "chunk", "content": content }),
            );
        }
        AgentLoopEvent::Error { message } => {
            let _ = app_handle.emit(
                events::METACOGNITIVE_RECOVERY,
                serde_json::json!({ "phase": "error", "message": message }),
            );
        }
        AgentLoopEvent::Complete { .. } => {
            let _ = app_handle.emit(
                events::METACOGNITIVE_RECOVERY,
                serde_json::json!({ "phase": "complete" }),
            );
        }
        _ => {}
    }));

    let run_request = prepared_run.request().clone();
    let result = match prepared_run.run().await {
        Ok(result) => result,
        Err(error) => {
            set_harness_idle(&state)?;
            return Err(error);
        }
    };
    {
        let mut kernel = state.writer_kernel.lock().map_err(|e| e.to_string())?;
        kernel.record_run_completion(&run_request, &result)?;
    }
    set_harness_idle(&state)?;
    Ok(MetacognitiveRecoveryRunResult::from_result(
        &payload.action,
        result,
    ))
}

impl MetacognitiveRecoveryRunResult {
    fn from_result(action: &MetacognitiveRecoveryAction, result: WriterAgentRunResult) -> Self {
        Self {
            action: action.into(),
            answer: result.answer,
            task_packet: result.task_packet,
            task_receipt: result.task_receipt,
            context_pack_summary: result.context_pack_summary,
            trace_refs: result.trace_refs,
            source_refs: result.source_refs,
        }
    }
}

fn recovery_request_id(context_payload: Option<&AskAgentContext>) -> String {
    context_payload
        .and_then(|payload| payload.request_id.clone())
        .filter(|id| !id.trim().is_empty())
        .unwrap_or_else(|| format!("meta-recovery-{}", agent_runtime::now_ms()))
}

fn set_harness_idle(state: &tauri::State<'_, AppState>) -> Result<(), String> {
    let mut s = crate::lock_harness_state(state)?;
    *s = HarnessState::Idle;
    Ok(())
}

fn recovery_budget_source_refs(
    request_id: &str,
    observation: &writer_agent::observation::WriterObservation,
    report: &WriterProviderBudgetReport,
    action: &MetacognitiveRecoveryAction,
) -> Vec<String> {
    let mut refs = vec![
        format!("metacognitive_recovery:{}", request_id),
        format!("recovery_action:{}", action.label()),
        format!("model:{}", report.model),
        format!("estimated_tokens:{}", report.estimated_total_tokens),
        format!("estimated_cost_micros:{}", report.estimated_cost_micros),
    ];
    if let Some(chapter) = observation.chapter_title.as_deref() {
        refs.push(format!("chapter:{}", chapter));
    }
    if let Some(revision) = observation.chapter_revision.as_deref() {
        refs.push(format!("revision:{}", revision));
    }
    refs
}

fn record_recovery_provider_budget_report(
    app: &tauri::AppHandle,
    task_id: &str,
    report: &WriterProviderBudgetReport,
    source_refs: Vec<String>,
    created_at_ms: u64,
) {
    let state = app.state::<AppState>();
    let Ok(mut kernel) = state.writer_kernel.lock() else {
        return;
    };
    kernel.record_provider_budget_report(task_id.to_string(), report, source_refs, created_at_ms);
}

fn record_recovery_model_started(
    app: &tauri::AppHandle,
    task_id: &str,
    report: &WriterProviderBudgetReport,
    source_refs: Vec<String>,
    created_at_ms: u64,
) {
    let state = app.state::<AppState>();
    let Ok(mut kernel) = state.writer_kernel.lock() else {
        return;
    };
    kernel.record_model_started_run_event(
        ModelStartedEventContext {
            task_id: task_id.to_string(),
            task: report.task,
            model: report.model.clone(),
            provider: "openai-compatible".to_string(),
            stream: true,
        },
        source_refs,
        Some(report),
        created_at_ms,
    );
}

fn record_recovery_provider_budget_failure(
    app: &tauri::AppHandle,
    task_id: String,
    source_refs: Vec<String>,
    report: WriterProviderBudgetReport,
    created_at_ms: u64,
    action: &MetacognitiveRecoveryAction,
) {
    let state = app.state::<AppState>();
    let Ok(mut kernel) = state.writer_kernel.lock() else {
        return;
    };
    let bundle = writer_agent::task_receipt::WriterFailureEvidenceBundle::new(
        writer_agent::task_receipt::WriterFailureCategory::ProviderFailed,
        METACOGNITIVE_RECOVERY_PROVIDER_BUDGET_ERROR,
        "Metacognitive recovery provider budget requires explicit approval before entering the read-only agent loop.",
        true,
        Some(task_id),
        source_refs,
        serde_json::json!({
            "providerBudget": report,
            "recoveryAction": action.label(),
        }),
        vec![
            "Surface the recovery run token/cost estimate to the author before retrying.".to_string(),
            "Narrow the current editor context or run a smaller recovery action if approval is not granted.".to_string(),
        ],
        created_at_ms,
    );
    kernel.record_failure_evidence_bundle(&bundle);
}

fn emit_recovery_provider_budget_error(
    app: &tauri::AppHandle,
    report: &WriterProviderBudgetReport,
    action: &MetacognitiveRecoveryAction,
) {
    let _ = app.emit(
        events::AGENT_ERROR,
        serde_json::json!({
            "message": "Metacognitive recovery provider budget requires explicit approval before calling the model.",
            "source": "metacognitive_recovery",
            "error": {
                "code": METACOGNITIVE_RECOVERY_PROVIDER_BUDGET_ERROR,
                "message": "Metacognitive recovery provider budget requires explicit approval before calling the model.",
                "recoverable": true,
                "details": {
                    "providerBudget": report,
                    "recoveryAction": action.label(),
                },
            },
        }),
    );
}

fn install_recovery_provider_budget_guard(
    prepared_run: &mut writer_agent::kernel::WriterAgentPreparedRun<
        OpenAiCompatProvider,
        tool_bridge::TauriToolBridge,
    >,
    app: tauri::AppHandle,
    request_id: String,
    observation: writer_agent::observation::WriterObservation,
    action: MetacognitiveRecoveryAction,
    approval: Option<writer_agent::provider_budget::WriterProviderBudgetApproval>,
    preflight_estimated_input_tokens: u64,
) {
    prepared_run.set_provider_call_guard(std::sync::Arc::new(move |context| {
        let mut report = writer_agent::kernel::WriterAgentPreparedRun::<
            OpenAiCompatProvider,
            tool_bridge::TauriToolBridge,
        >::provider_budget_from_call_context(
            WriterProviderBudgetTask::MetacognitiveRecovery,
            &context,
        );
        if context.round == 1
            && report.estimated_input_tokens <= preflight_estimated_input_tokens
            && report.decision == WriterProviderBudgetDecision::ApprovalRequired
        {
            report = apply_provider_budget_approval(report, approval.as_ref());
        }

        let task_id = format!(
            "metacognitive-recovery-{}-round-{}",
            request_id, context.round
        );
        let source_refs = recovery_budget_source_refs(&request_id, &observation, &report, &action);
        let created_at = agent_runtime::now_ms();
        record_recovery_provider_budget_report(
            &app,
            &task_id,
            &report,
            source_refs.clone(),
            created_at,
        );
        if report.approval_required {
            emit_recovery_provider_budget_error(&app, &report, &action);
            record_recovery_provider_budget_failure(
                &app,
                task_id,
                source_refs,
                report,
                created_at,
                &action,
            );
            return Err(METACOGNITIVE_RECOVERY_PROVIDER_BUDGET_ERROR.to_string());
        }

        record_recovery_model_started(
            &app,
            &task_id,
            &report,
            source_refs,
            agent_runtime::now_ms(),
        );
        Ok(())
    }));
}
