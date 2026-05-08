//! Generation, analysis, and creative drafting Tauri commands.

use crate::chapter_generation::{
    ChapterGenerationEvent, FrontendChapterStateSnapshot, GenerateChapterAutonomousPayload,
    PipelineTerminal, SaveMode,
};
use crate::llm_runtime;
use crate::writer_agent::kernel::ModelStartedEventContext;
use crate::writer_agent::provider_budget::{
    evaluate_provider_budget, WriterProviderBudgetApproval,
};
use crate::writer_agent::supervised_sprint::{
    advance_sprint, budget_ceiling_reached, cancel_sprint, checkpoint_sprint,
    create_sprint_plan_with_limits, pause_sprint, record_budget_usage, resume_sprint,
    sprint_progress, update_current_chapter_state, SprintCheckpoint, SprintProgress,
    SupervisedSprintPlan,
};
use serde::{Deserialize, Serialize};
use tauri::{Emitter, Manager};

// ── Types ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ReviewItem {
    quote: String,
    #[serde(rename = "type")]
    review_type: String,
    issue: String,
    suggestion: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ReviewReport {
    reviews: Vec<ReviewItem>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ParallelDraft {
    id: String,
    label: String,
    text: String,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ParallelDraftPayload {
    prefix: String,
    suffix: String,
    paragraph: String,
    selected_text: String,
    chapter_title: Option<String>,
    #[serde(default)]
    mission_context: String,
    #[serde(default)]
    promise_context: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AskProjectBrainPayload {
    query: String,
    provider_budget_approval: Option<WriterProviderBudgetApproval>,
}

#[derive(Serialize, Clone)]
struct BatchStatus {
    chapter_title: String,
    status: String,
    error: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ChapterGenerationStart {
    request_id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartSupervisedSprintPayload {
    pub chapter_titles: Vec<String>,
    #[serde(default)]
    pub require_approval_per_chapter: bool,
    #[serde(default)]
    pub max_chapters_per_session: Option<usize>,
    #[serde(default)]
    pub budget_ceiling_micros: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepairChapterStateResult {
    pub chapter_title: String,
    pub revision: String,
    pub settlement_delta: crate::chapter_generation::ChapterSettlementDelta,
    pub settlement_apply: crate::chapter_generation::ChapterSettlementApplyResult,
    pub artifact_refs: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub settlement_replay_matches: Option<bool>,
    #[serde(default)]
    pub already_repaired: bool,
}

// ── Helpers ─────────────────────────────────────────────────────────────────

fn trim_parallel_draft(text: &str) -> String {
    text.trim_matches(|c: char| c == '`' || c.is_whitespace())
        .chars()
        .take(1200)
        .collect::<String>()
}

fn parse_parallel_drafts(raw: &str) -> Vec<ParallelDraft> {
    let labels = ["A 顺势推进", "B 冲突加压", "C 情绪转折"];
    let ids = ["a", "b", "c"];
    let mut drafts = Vec::new();
    let mut current_idx: Option<usize> = None;
    let mut current_text = String::new();

    let flush = |drafts: &mut Vec<ParallelDraft>,
                 current_idx: &mut Option<usize>,
                 current_text: &mut String| {
        let Some(idx) = current_idx.take() else {
            current_text.clear();
            return;
        };
        let text = trim_parallel_draft(current_text);
        current_text.clear();
        if text.is_empty() {
            return;
        }
        drafts.push(ParallelDraft {
            id: ids[idx].to_string(),
            label: labels[idx].to_string(),
            text,
        });
    };

    for line in raw.lines() {
        let trimmed = line.trim_start();
        let marker = trimmed
            .split_once(':')
            .or_else(|| trimmed.split_once('：'))
            .and_then(|(head, body)| {
                let idx = match head.trim().chars().next().map(|c| c.to_ascii_uppercase()) {
                    Some('A') => 0,
                    Some('B') => 1,
                    Some('C') => 2,
                    _ => return None,
                };
                Some((idx, body.trim_start()))
            });

        if let Some((idx, body)) = marker {
            flush(&mut drafts, &mut current_idx, &mut current_text);
            current_idx = Some(idx);
            current_text.push_str(body);
        } else if current_idx.is_some() {
            if !current_text.is_empty() {
                current_text.push('\n');
            }
            current_text.push_str(line);
        }
    }
    flush(&mut drafts, &mut current_idx, &mut current_text);
    drafts.truncate(3);
    drafts
}

fn record_writer_failure_bundle(
    app: &tauri::AppHandle,
    bundle: &crate::writer_agent::task_receipt::WriterFailureEvidenceBundle,
) {
    let state = app.state::<crate::AppState>();
    let Ok(mut kernel) = state.writer_kernel.lock() else {
        return;
    };
    kernel.record_failure_evidence_bundle(bundle);
}

fn record_chapter_provider_budget_report(
    app: &tauri::AppHandle,
    context: &crate::chapter_generation::BuiltChapterContext,
    report: &crate::writer_agent::provider_budget::WriterProviderBudgetReport,
) {
    let state = app.state::<crate::AppState>();
    let Ok(mut kernel) = state.writer_kernel.lock() else {
        return;
    };
    let mut source_refs = vec![
        format!("receipt:{}", context.receipt.task_id),
        format!("chapter:{}", context.target.title),
    ];
    source_refs.extend(
        context
            .sources
            .iter()
            .filter(|source| source.included_chars > 0)
            .map(|source| format!("{}:{}", source.source_type, source.id)),
    );
    kernel.record_provider_budget_report(
        context.request_id.clone(),
        report,
        source_refs,
        crate::agent_runtime::now_ms(),
    );
}

fn sprint_current_chapter_title(plan: &SupervisedSprintPlan) -> Option<&str> {
    plan.chapters
        .get(plan.current_index)
        .map(|chapter| chapter.chapter_title.as_str())
}

fn sprint_matches_target(plan: &SupervisedSprintPlan, target_title: &str) -> bool {
    sprint_current_chapter_title(plan).is_some_and(|title| title == target_title)
}

fn sprint_block_message(plan: &SupervisedSprintPlan, target_title: &str) -> Option<String> {
    if plan.status == "paused" {
        return Some(format!(
            "supervised sprint {} is paused before {}",
            plan.sprint_id, target_title
        ));
    }
    if plan.status == "cancelled" || plan.status == "completed" {
        return Some(format!(
            "supervised sprint {} is {}",
            plan.sprint_id, plan.status
        ));
    }
    if budget_ceiling_reached(plan) {
        return Some(format!(
            "supervised sprint {} reached budget ceiling before {}",
            plan.sprint_id, target_title
        ));
    }
    None
}

fn persist_sprint_plan(app: &tauri::AppHandle, plan: &SupervisedSprintPlan) -> Result<(), String> {
    let state = app.state::<crate::AppState>();
    let kernel = state
        .writer_kernel
        .lock()
        .map_err(|_| "Writer kernel lock poisoned".to_string())?;
    kernel
        .memory
        .upsert_supervised_sprint(&kernel.project_id, plan)
        .map_err(|e| e.to_string())
}

fn persist_sprint_checkpoint(
    app: &tauri::AppHandle,
    checkpoint: &SprintCheckpoint,
) -> Result<(), String> {
    let state = app.state::<crate::AppState>();
    let kernel = state
        .writer_kernel
        .lock()
        .map_err(|_| "Writer kernel lock poisoned".to_string())?;
    kernel
        .memory
        .insert_supervised_sprint_checkpoint(&kernel.project_id, checkpoint)
        .map_err(|e| e.to_string())
}

fn ensure_sprint_allows_generation(
    app: &tauri::AppHandle,
    target_title: Option<&str>,
) -> Result<(), String> {
    let Some(target_title) = target_title else {
        return Ok(());
    };
    let state = app.state::<crate::AppState>();
    let sprint = state
        .current_sprint
        .lock()
        .map_err(|_| "Supervised sprint lock poisoned".to_string())?;
    let Some(plan) = sprint
        .as_ref()
        .filter(|plan| sprint_matches_target(plan, target_title))
    else {
        return Ok(());
    };
    if let Some(message) = sprint_block_message(plan, target_title) {
        return Err(message);
    }
    Ok(())
}

fn sprint_blocked_error(message: String) -> crate::chapter_generation::ChapterGenerationError {
    crate::chapter_generation::ChapterGenerationError::new(
        "SUPERVISED_SPRINT_BLOCKED",
        message,
        true,
    )
}

fn ensure_sprint_allows_provider_budget(
    app: &tauri::AppHandle,
    target_title: &str,
    report: &crate::writer_agent::provider_budget::WriterProviderBudgetReport,
) -> Result<(), String> {
    let state = app.state::<crate::AppState>();
    let sprint = state
        .current_sprint
        .lock()
        .map_err(|_| "Supervised sprint lock poisoned".to_string())?;
    let Some(plan) = sprint
        .as_ref()
        .filter(|plan| sprint_matches_target(plan, target_title))
    else {
        return Ok(());
    };
    if let Some(message) = sprint_block_message(plan, target_title) {
        return Err(message);
    }
    if let Some(ceiling) = plan.budget_ceiling_micros {
        let projected = plan
            .spent_budget_micros
            .saturating_add(report.estimated_cost_micros);
        if projected > ceiling {
            return Err(format!(
                "supervised sprint {} would exceed budget ceiling before {} (spent {} + estimated {} > ceiling {})",
                plan.sprint_id,
                target_title,
                plan.spent_budget_micros,
                report.estimated_cost_micros,
                ceiling
            ));
        }
    }
    Ok(())
}

fn record_sprint_context_built(
    app: &tauri::AppHandle,
    context: &crate::chapter_generation::BuiltChapterContext,
) {
    let plan_to_persist = {
        let state = app.state::<crate::AppState>();
        let Ok(mut sprint) = state.current_sprint.lock() else {
            return;
        };
        let Some(plan) = sprint
            .as_mut()
            .filter(|plan| sprint_matches_target(plan, &context.target.title))
        else {
            return;
        };
        if plan.status == "planned" {
            plan.status = "running".to_string();
        }
        let readiness = if context.warnings.is_empty() {
            "ready"
        } else {
            "warning"
        };
        update_current_chapter_state(
            plan,
            Some("drafting"),
            Some(&context.receipt.task_id),
            Some(readiness),
            None,
        );
        plan.clone()
    };
    if let Err(error) = persist_sprint_plan(app, &plan_to_persist) {
        tracing::warn!(
            "Failed to persist supervised sprint context state: {}",
            error
        );
    }
}

fn record_sprint_provider_budget(
    app: &tauri::AppHandle,
    context: &crate::chapter_generation::BuiltChapterContext,
    report: &crate::writer_agent::provider_budget::WriterProviderBudgetReport,
) {
    let plan_to_persist = {
        let state = app.state::<crate::AppState>();
        let Ok(mut sprint) = state.current_sprint.lock() else {
            return;
        };
        let Some(plan) = sprint
            .as_mut()
            .filter(|plan| sprint_matches_target(plan, &context.target.title))
        else {
            return;
        };
        record_budget_usage(plan, report.estimated_cost_micros);
        if budget_ceiling_reached(plan) {
            plan.status = "paused".to_string();
            update_current_chapter_state(
                plan,
                None,
                None,
                None,
                Some("budget ceiling reached after provider estimate"),
            );
        }
        plan.clone()
    };
    if let Err(error) = persist_sprint_plan(app, &plan_to_persist) {
        tracing::warn!(
            "Failed to persist supervised sprint budget state: {}",
            error
        );
    }
}

fn record_sprint_generation_completed(
    app: &tauri::AppHandle,
    saved: &crate::chapter_generation::SaveGeneratedChapterOutput,
) {
    let plan_to_persist = {
        let state = app.state::<crate::AppState>();
        let Ok(mut sprint) = state.current_sprint.lock() else {
            return;
        };
        let Some(plan) = sprint
            .as_mut()
            .filter(|plan| sprint_matches_target(plan, &saved.chapter_title))
        else {
            return;
        };
        update_current_chapter_state(plan, Some("saved"), None, Some("ready"), None);
        if !plan.require_approval_per_chapter && !budget_ceiling_reached(plan) {
            let _ = advance_sprint(plan);
        }
        plan.clone()
    };
    if let Err(error) = persist_sprint_plan(app, &plan_to_persist) {
        tracing::warn!("Failed to persist supervised sprint completion: {}", error);
    }
}

fn record_sprint_generation_failed(
    app: &tauri::AppHandle,
    target_title: &str,
    error_message: &str,
) {
    let plan_to_persist = {
        let state = app.state::<crate::AppState>();
        let Ok(mut sprint) = state.current_sprint.lock() else {
            return;
        };
        let Some(plan) = sprint
            .as_mut()
            .filter(|plan| sprint_matches_target(plan, target_title))
        else {
            return;
        };
        update_current_chapter_state(
            plan,
            Some("blocked"),
            None,
            Some("blocked"),
            Some(error_message),
        );
        plan.clone()
    };
    if let Err(error) = persist_sprint_plan(app, &plan_to_persist) {
        tracing::warn!("Failed to persist supervised sprint failure: {}", error);
    }
}

fn chapter_model_source_refs(
    context: &crate::chapter_generation::BuiltChapterContext,
    report: &crate::writer_agent::provider_budget::WriterProviderBudgetReport,
) -> Vec<String> {
    let mut source_refs = vec![
        format!("receipt:{}", context.receipt.task_id),
        format!("chapter:{}", context.target.title),
        format!("model:{}", report.model),
        format!("estimated_tokens:{}", report.estimated_total_tokens),
        format!("estimated_cost_micros:{}", report.estimated_cost_micros),
    ];
    source_refs.extend(
        context
            .sources
            .iter()
            .filter(|source| source.included_chars > 0)
            .map(|source| format!("{}:{}", source.source_type, source.id)),
    );
    source_refs
}

fn record_chapter_model_started(
    app: &tauri::AppHandle,
    context: &crate::chapter_generation::BuiltChapterContext,
    report: &crate::writer_agent::provider_budget::WriterProviderBudgetReport,
) {
    let state = app.state::<crate::AppState>();
    let Ok(mut kernel) = state.writer_kernel.lock() else {
        return;
    };
    kernel.record_model_started_run_event(
        ModelStartedEventContext {
            task_id: context.request_id.clone(),
            task: report.task,
            model: report.model.clone(),
            provider: "openai-compatible".to_string(),
            stream: false,
        },
        chapter_model_source_refs(context, report),
        Some(report),
        crate::agent_runtime::now_ms(),
    );
}

fn record_chapter_context_pack_built(
    app: &tauri::AppHandle,
    context: &crate::chapter_generation::BuiltChapterContext,
    created_at_ms: u64,
) {
    let state = app.state::<crate::AppState>();
    let Ok(mut kernel) = state.writer_kernel.lock() else {
        return;
    };
    kernel.record_chapter_context_pack_built_run_event(context, created_at_ms);
}

fn build_chapter_generation_config(
    app: &tauri::AppHandle,
    settings: llm_runtime::LlmSettings,
    payload: GenerateChapterAutonomousPayload,
    user_profile_entries: Vec<String>,
) -> Result<
    crate::chapter_generation::ChapterGenerationConfig<
        crate::chapter_generation::TauriChapterGenerationProject,
    >,
    String,
> {
    let project = crate::chapter_generation::TauriChapterGenerationProject::new(app.clone())?;
    let project_id =
        crate::chapter_generation::ChapterGenerationProject::project_id(&project).to_string();
    let memory_path =
        crate::chapter_generation::ChapterGenerationProject::memory_path(&project).to_path_buf();
    Ok(crate::chapter_generation::ChapterGenerationConfig {
        project,
        settings,
        payload,
        user_profile_entries,
        project_id,
        memory_path,
    })
}

// ── Commands ────────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn batch_generate_chapter(
    app: tauri::AppHandle,
    chapter_title: String,
    summary: String,
    frontend_state: Option<FrontendChapterStateSnapshot>,
) -> Result<(), String> {
    ensure_sprint_allows_generation(&app, Some(&chapter_title))?;
    let api_key = crate::require_api_key()?;
    let settings = llm_runtime::settings(api_key);
    let user_profile_entries = crate::collect_user_profile_entries(&app).unwrap_or_default();
    let request_id = crate::chapter_generation::make_request_id("batch");

    let app_clone = app.clone();
    let title_clone = chapter_title.clone();

    tauri::async_runtime::spawn(async move {
        let _ = app_clone.emit(
            crate::events::BATCH_STATUS,
            BatchStatus {
                chapter_title: title_clone.clone(),
                status: "generating".to_string(),
                error: String::new(),
            },
        );

        let trace_request_id = request_id.clone();
        let payload = GenerateChapterAutonomousPayload {
            request_id: Some(request_id.clone()),
            target_chapter_title: Some(title_clone.clone()),
            target_chapter_number: None,
            user_instruction: format!("帮我写《{}》这一章的完整初稿。", title_clone),
            budget: None,
            frontend_state,
            save_mode: SaveMode::ReplaceIfClean,
            chapter_summary_override: Some(summary),
            chapter_contract: None,
            provider_budget_approval: None,
        };
        let user_instruction = payload.user_instruction.clone();
        let trace_app = app_clone.clone();
        let budget_app = app_clone.clone();
        let guard_app = app_clone.clone();
        let model_app = app_clone.clone();

        let config = match build_chapter_generation_config(
            &app_clone,
            settings,
            payload,
            user_profile_entries,
        ) {
            Ok(config) => config,
            Err(error) => {
                record_sprint_generation_failed(&app_clone, &title_clone, &error);
                let bundle = crate::writer_agent::task_receipt::WriterFailureEvidenceBundle::new(
                    crate::writer_agent::task_receipt::WriterFailureCategory::SaveFailed,
                    "CHAPTER_GENERATION_CONFIG_FAILED",
                    error.clone(),
                    true,
                    Some(trace_request_id.clone()),
                    vec![format!("chapter:{}", title_clone)],
                    serde_json::json!({ "error": error }),
                    vec!["Check project storage paths and retry.".to_string()],
                    crate::agent_runtime::now_ms(),
                );
                record_writer_failure_bundle(&app_clone, &bundle);
                let _ = app_clone.emit(
                    crate::events::BATCH_STATUS,
                    BatchStatus {
                        chapter_title: title_clone,
                        status: "error".to_string(),
                        error: "chapter generation config failed".to_string(),
                    },
                );
                return;
            }
        };

        let terminal = crate::chapter_generation::run_chapter_generation_pipeline(
            config,
            |event| {
                let _ = app_clone.emit(crate::events::CHAPTER_GENERATION, event);
            },
            move |context| {
                let created_at_ms = crate::agent_runtime::now_ms();
                record_sprint_context_built(&trace_app, context);
                record_chapter_context_pack_built(&trace_app, context, created_at_ms);
                let state = trace_app.state::<crate::AppState>();
                let Ok(mut kernel) = state.writer_kernel.lock() else {
                    return;
                };
                let packet = crate::chapter_generation::build_chapter_generation_task_packet(
                    &kernel.project_id,
                    &kernel.session_id,
                    context,
                    &user_instruction,
                    created_at_ms,
                );
                if let Err(error) = kernel.record_task_packet(
                    context.request_id.clone(),
                    "ChapterGeneration",
                    packet,
                ) {
                    tracing::warn!(
                        "WriterAgent chapter-generation task packet rejected: {}",
                        error
                    );
                }
            },
            move |context, report| {
                record_sprint_provider_budget(&budget_app, context, report);
                record_chapter_provider_budget_report(&budget_app, context, report);
            },
            move |context, report| {
                ensure_sprint_allows_provider_budget(&guard_app, &context.target.title, report)
                    .map_err(sprint_blocked_error)
            },
            move |context, report| {
                record_chapter_model_started(&model_app, context, report);
            },
        )
        .await;

        match terminal {
            PipelineTerminal::Completed {
                saved,
                generated_content,
                settlement_delta,
            } => {
                record_sprint_generation_completed(&app_clone, &saved);
                crate::observe_generated_chapter_result(
                    &app_clone,
                    &saved,
                    &generated_content,
                    Some(&settlement_delta),
                );
                let embed_app = app_clone.clone();
                let embed_title = title_clone.clone();
                tauri::async_runtime::spawn(async move {
                    crate::auto_embed_chapter(&embed_app, &embed_title, &generated_content).await;
                });
                let _ = app_clone.emit(
                    crate::events::BATCH_STATUS,
                    BatchStatus {
                        chapter_title: title_clone,
                        status: "complete".to_string(),
                        error: String::new(),
                    },
                );
            }
            PipelineTerminal::Conflict(conflict) => {
                record_sprint_generation_failed(
                    &app_clone,
                    &title_clone,
                    &format!("save conflict: {}", conflict.reason),
                );
                let bundle = crate::writer_agent::task_receipt::WriterFailureEvidenceBundle::new(
                    crate::writer_agent::task_receipt::WriterFailureCategory::SaveFailed,
                    "SAVE_CONFLICT",
                    format!("Save blocked by {}.", conflict.reason),
                    true,
                    Some(trace_request_id.clone()),
                    vec![
                        format!("base_revision:{}", conflict.base_revision),
                        format!("current_revision:{}", conflict.current_revision),
                        format!("save_conflict:{}", conflict.reason),
                    ],
                    serde_json::json!({ "conflict": conflict }),
                    vec![
                        "Resolve editor/storage revision mismatch or save as a draft copy."
                            .to_string(),
                    ],
                    crate::agent_runtime::now_ms(),
                );
                record_writer_failure_bundle(&app_clone, &bundle);
                let _ = app_clone.emit(
                    crate::events::BATCH_STATUS,
                    BatchStatus {
                        chapter_title: title_clone,
                        status: "error".to_string(),
                        error: format!("save conflict: {}", conflict.reason),
                    },
                );
            }
            PipelineTerminal::Failed(error) => {
                record_sprint_generation_failed(&app_clone, &title_clone, &error.message);
                let bundle = crate::chapter_generation::failure_bundle_from_chapter_error(
                    &trace_request_id,
                    &error,
                    crate::agent_runtime::now_ms(),
                );
                record_writer_failure_bundle(&app_clone, &bundle);
                let _ = app_clone.emit(
                    crate::events::BATCH_STATUS,
                    BatchStatus {
                        chapter_title: title_clone,
                        status: "error".to_string(),
                        error: error.message,
                    },
                );
            }
        }
    });

    Ok(())
}

#[tauri::command]
pub async fn generate_chapter_autonomous(
    app: tauri::AppHandle,
    payload: GenerateChapterAutonomousPayload,
) -> Result<ChapterGenerationStart, String> {
    ensure_sprint_allows_generation(&app, payload.target_chapter_title.as_deref())?;
    let api_key = crate::require_api_key()?;
    let settings = llm_runtime::settings(api_key);
    let user_profile_entries = crate::collect_user_profile_entries(&app).unwrap_or_default();
    let request_id = payload
        .request_id
        .clone()
        .unwrap_or_else(|| crate::chapter_generation::make_request_id("chapter"));
    let payload = GenerateChapterAutonomousPayload {
        request_id: Some(request_id.clone()),
        ..payload
    };
    let app_clone = app.clone();
    let trace_request_id = request_id.clone();
    let requested_target_title = payload.target_chapter_title.clone();

    tauri::async_runtime::spawn(async move {
        let user_instruction = payload.user_instruction.clone();
        let trace_app = app_clone.clone();
        let budget_app = app_clone.clone();
        let guard_app = app_clone.clone();
        let model_app = app_clone.clone();
        let config = match build_chapter_generation_config(
            &app_clone,
            settings,
            payload,
            user_profile_entries,
        ) {
            Ok(config) => config,
            Err(error) => {
                if let Some(target_title) = requested_target_title.as_deref() {
                    record_sprint_generation_failed(&app_clone, target_title, &error);
                }
                let bundle = crate::writer_agent::task_receipt::WriterFailureEvidenceBundle::new(
                    crate::writer_agent::task_receipt::WriterFailureCategory::SaveFailed,
                    "CHAPTER_GENERATION_CONFIG_FAILED",
                    error.clone(),
                    true,
                    Some(trace_request_id.clone()),
                    requested_target_title
                        .iter()
                        .map(|title| format!("chapter:{}", title))
                        .collect(),
                    serde_json::json!({ "error": error }),
                    vec!["Check project storage paths and retry.".to_string()],
                    crate::agent_runtime::now_ms(),
                );
                record_writer_failure_bundle(&app_clone, &bundle);
                return;
            }
        };
        let terminal = crate::chapter_generation::run_chapter_generation_pipeline(
            config,
            |event: ChapterGenerationEvent| {
                let _ = app_clone.emit(crate::events::CHAPTER_GENERATION, event);
            },
            move |context| {
                let created_at_ms = crate::agent_runtime::now_ms();
                record_sprint_context_built(&trace_app, context);
                record_chapter_context_pack_built(&trace_app, context, created_at_ms);
                let state = trace_app.state::<crate::AppState>();
                let Ok(mut kernel) = state.writer_kernel.lock() else {
                    return;
                };
                let packet = crate::chapter_generation::build_chapter_generation_task_packet(
                    &kernel.project_id,
                    &kernel.session_id,
                    context,
                    &user_instruction,
                    created_at_ms,
                );
                if let Err(error) = kernel.record_task_packet(
                    context.request_id.clone(),
                    "ChapterGeneration",
                    packet,
                ) {
                    tracing::warn!(
                        "WriterAgent chapter-generation task packet rejected: {}",
                        error
                    );
                }
            },
            move |context, report| {
                record_sprint_provider_budget(&budget_app, context, report);
                record_chapter_provider_budget_report(&budget_app, context, report);
            },
            move |context, report| {
                ensure_sprint_allows_provider_budget(&guard_app, &context.target.title, report)
                    .map_err(sprint_blocked_error)
            },
            move |context, report| {
                record_chapter_model_started(&model_app, context, report);
            },
        )
        .await;

        match terminal {
            PipelineTerminal::Completed {
                saved,
                generated_content,
                settlement_delta,
            } => {
                record_sprint_generation_completed(&app_clone, &saved);
                crate::observe_generated_chapter_result(
                    &app_clone,
                    &saved,
                    &generated_content,
                    Some(&settlement_delta),
                );
                let embed_app = app_clone.clone();
                tauri::async_runtime::spawn(async move {
                    crate::auto_embed_chapter(&embed_app, &saved.chapter_title, &generated_content)
                        .await;
                });
            }
            PipelineTerminal::Conflict(conflict) => {
                if let Some(target_title) = requested_target_title.as_deref() {
                    record_sprint_generation_failed(
                        &app_clone,
                        target_title,
                        &format!("save conflict: {}", conflict.reason),
                    );
                }
                let bundle = crate::writer_agent::task_receipt::WriterFailureEvidenceBundle::new(
                    crate::writer_agent::task_receipt::WriterFailureCategory::SaveFailed,
                    "SAVE_CONFLICT",
                    format!("Save blocked by {}.", conflict.reason),
                    true,
                    Some(trace_request_id.clone()),
                    vec![
                        format!("base_revision:{}", conflict.base_revision),
                        format!("current_revision:{}", conflict.current_revision),
                        format!("save_conflict:{}", conflict.reason),
                    ],
                    serde_json::json!({ "conflict": conflict }),
                    vec![
                        "Resolve editor/storage revision mismatch or save as a draft copy."
                            .to_string(),
                    ],
                    crate::agent_runtime::now_ms(),
                );
                record_writer_failure_bundle(&app_clone, &bundle);
            }
            PipelineTerminal::Failed(error) => {
                if let Some(target_title) = requested_target_title.as_deref() {
                    record_sprint_generation_failed(&app_clone, target_title, &error.message);
                }
                let bundle = crate::chapter_generation::failure_bundle_from_chapter_error(
                    &trace_request_id,
                    &error,
                    crate::agent_runtime::now_ms(),
                );
                record_writer_failure_bundle(&app_clone, &bundle);
            }
        }
    });

    Ok(ChapterGenerationStart { request_id })
}

#[tauri::command]
pub async fn analyze_chapter(
    _app: tauri::AppHandle,
    content: String,
) -> Result<Vec<ReviewItem>, String> {
    let api_key = crate::require_api_key()?;
    let settings = llm_runtime::settings(api_key);

    // Provider budget preflight
    let _budget_report = evaluate_provider_budget(
        crate::writer_agent::provider_budget::WriterProviderBudgetRequest::new(
            crate::writer_agent::provider_budget::WriterProviderBudgetTask::ManualRequest,
            &settings.model,
            2000,
            1024,
        ),
    );
    tracing::info!(
        task = "analyze_chapter",
        decision = ?_budget_report.decision,
        tokens = _budget_report.estimated_total_tokens,
        "Provider budget preflight"
    );

    let system_prompt = r#"You are a professional novel editor. Analyze the chapter and output a JSON object with a "reviews" array.

Each review must have:
- "quote": exact text from the chapter (copy verbatim, at least 10 characters)
- "type": one of "logic" | "ooc" | "pacing" | "prose"
- "issue": what the problem is
- "suggestion": how to fix it (in Chinese, specific rewrite suggestion)

Output ONLY the JSON object, no explanation outside. Example:
{"reviews":[{"quote":"他走出了房间","type":"prose","issue":"缺乏画面感","suggestion":"他推开吱呀作响的木门，幽暗的走廊里只有自己的脚步声在回荡。"}]}"#;

    let truncated = agent_harness_core::truncate_context(&content, 8000);
    let body = llm_runtime::chat_json(
        &settings,
        vec![
            serde_json::json!({"role": "system", "content": system_prompt}),
            serde_json::json!({"role": "user", "content": format!("Analyze this chapter:\n\n{}", truncated)}),
        ],
        60,
    )
    .await?;

    let report: ReviewReport =
        serde_json::from_value(body).map_err(|e| format!("Failed to parse review JSON: {}", e))?;

    Ok(report.reviews)
}

#[tauri::command]
pub async fn ask_project_brain(
    app: tauri::AppHandle,
    query: String,
    payload: Option<AskProjectBrainPayload>,
) -> Result<(), String> {
    let query = payload
        .as_ref()
        .map(|payload| payload.query.as_str())
        .unwrap_or(&query);
    let api_key = crate::require_api_key()?;
    let settings = llm_runtime::settings(api_key);
    let focus = {
        let state = app.state::<crate::AppState>();
        let kernel = state.writer_kernel.lock().map_err(|e| e.to_string())?;
        crate::brain_service::ProjectBrainFocus::from_kernel(query, &kernel)
    };

    crate::brain_service::answer_query_with_focus(
        &app,
        &settings,
        query,
        &focus,
        payload
            .as_ref()
            .and_then(|payload| payload.provider_budget_approval.as_ref()),
        |content| {
            let _ = app.emit(
                crate::events::AGENT_STREAM_CHUNK,
                crate::StreamChunk { content },
            );
            Ok(llm_runtime::StreamControl::Continue)
        },
    )
    .await?;

    let _ = app.emit(
        crate::events::AGENT_STREAM_END,
        crate::StreamEnd {
            reason: "complete".to_string(),
        },
    );
    Ok(())
}

#[tauri::command]
pub async fn generate_parallel_drafts(
    payload: ParallelDraftPayload,
) -> Result<Vec<ParallelDraft>, String> {
    let api_key = crate::require_api_key()?;
    let settings = llm_runtime::settings(api_key);

    let _budget_report = evaluate_provider_budget(
        crate::writer_agent::provider_budget::WriterProviderBudgetRequest::new(
            crate::writer_agent::provider_budget::WriterProviderBudgetTask::GhostPreview,
            &settings.model,
            2000,
            2000,
        ),
    );
    tracing::info!(
        task = "generate_parallel_drafts",
        decision = ?_budget_report.decision,
        tokens = _budget_report.estimated_total_tokens,
        "Provider budget preflight"
    );

    let chapter = payload
        .chapter_title
        .as_deref()
        .filter(|title| !title.trim().is_empty())
        .unwrap_or("当前章节");
    let focus = if payload.selected_text.trim().is_empty() {
        payload.paragraph.trim()
    } else {
        payload.selected_text.trim()
    };

    let mission_block = if payload.mission_context.trim().is_empty() {
        String::new()
    } else {
        format!("\n## 本章任务约束\n{}\n", payload.mission_context)
    };
    let promise_block = if payload.promise_context.trim().is_empty() {
        String::new()
    } else {
        format!("\n## 未兑现伏笔参考\n{}\n", payload.promise_context)
    };

    let prompt = format!(
        "你是中文小说共创写手。请顺着用户已有文本，生成三个不同方向的平行草稿。\n\
         输出格式必须严格为：\n\
         A: ...\nB: ...\nC: ...\n\
         每个版本 2-5 句，可以分段；每个版本末尾用括号标注关联的创作依据。不要解释，不要 Markdown。\n\
         A 偏顺势推进，B 偏冲突加压，C 偏情绪转折。\n\
         ## 章节\n{}{}{}\n## 光标前文\n{}\n## 光标后文\n{}\n## 当前焦点\n{}",
        chapter,
        mission_block,
        promise_block,
        agent_harness_core::truncate_context(&payload.prefix, 3000),
        agent_harness_core::truncate_context(&payload.suffix, 1000),
        focus,
    );

    let text = llm_runtime::chat_text_profile(
        &settings,
        vec![serde_json::json!({"role": "user", "content": prompt})],
        llm_runtime::LlmRequestProfile::ParallelDraft,
        45,
    )
    .await?;
    let drafts = parse_parallel_drafts(&text);
    if drafts.is_empty() {
        let fallback = trim_parallel_draft(&text);
        if fallback.is_empty() {
            return Ok(Vec::new());
        }
        return Ok(vec![ParallelDraft {
            id: "a".to_string(),
            label: "A 顺势推进".to_string(),
            text: fallback,
        }]);
    }
    Ok(drafts)
}

#[tauri::command]
pub async fn analyze_pacing(summaries: String) -> Result<String, String> {
    let api_key = crate::require_api_key()?;
    let settings = llm_runtime::settings(api_key);

    let _budget_report = evaluate_provider_budget(
        crate::writer_agent::provider_budget::WriterProviderBudgetRequest::new(
            crate::writer_agent::provider_budget::WriterProviderBudgetTask::ManualRequest,
            &settings.model,
            3000,
            1024,
        ),
    );
    tracing::info!(
        task = "analyze_pacing",
        decision = ?_budget_report.decision,
        tokens = _budget_report.estimated_total_tokens,
        "Provider budget preflight"
    );

    let text = llm_runtime::chat_text_profile(
        &settings,
        vec![
            serde_json::json!({"role": "system", "content": "You are a structural editor. Analyze the chapter sequence for pacing issues, slow sections, abrupt transitions, and unresolved arcs. Be specific and concise."}),
            serde_json::json!({"role": "user", "content": format!("Chapter summaries:\n{}", summaries)}),
        ],
        llm_runtime::LlmRequestProfile::Analysis,
        60,
    )
    .await?;

    Ok(if text.is_empty() {
        "No analysis generated".to_string()
    } else {
        text
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_parallel_drafts_keeps_multiline_branches() {
        let drafts = parse_parallel_drafts(
            "A: 林墨没有立刻回答。\n他只是把刀压低。\nB：门外忽然传来脚步声。\nC: 她看见他眼里的犹豫。",
        );

        assert_eq!(drafts.len(), 3);
        assert_eq!(drafts[0].id, "a");
        assert!(drafts[0].text.contains("把刀压低"));
        assert_eq!(drafts[1].label, "B 冲突加压");
        assert_eq!(drafts[2].id, "c");
    }

    #[test]
    fn trim_parallel_draft_removes_markdown_fence_noise() {
        assert_eq!(
            trim_parallel_draft("```\n林墨停下脚步。\n```"),
            "林墨停下脚步。"
        );
    }
}

#[tauri::command]
pub fn start_supervised_sprint(
    app: tauri::AppHandle,
    payload: StartSupervisedSprintPayload,
) -> Result<SupervisedSprintPlan, String> {
    if payload.chapter_titles.is_empty() {
        return Err("supervised sprint requires at least one chapter".to_string());
    }
    let sprint_id = crate::chapter_generation::make_request_id("sprint");
    let plan = create_sprint_plan_with_limits(
        &sprint_id,
        &payload.chapter_titles,
        payload.require_approval_per_chapter,
        payload
            .max_chapters_per_session
            .unwrap_or(payload.chapter_titles.len())
            .max(1),
        payload.budget_ceiling_micros,
    );
    persist_sprint_plan(&app, &plan)?;
    let state = app.state::<crate::AppState>();
    let mut sprint = state
        .current_sprint
        .lock()
        .map_err(|_| "Supervised sprint lock poisoned".to_string())?;
    *sprint = Some(plan.clone());
    Ok(plan)
}

#[tauri::command]
pub fn get_supervised_sprint_plan(
    app: tauri::AppHandle,
) -> Result<Option<SupervisedSprintPlan>, String> {
    let state = app.state::<crate::AppState>();
    let sprint = state
        .current_sprint
        .lock()
        .map_err(|_| "Supervised sprint lock poisoned".to_string())?;
    Ok(sprint.clone())
}

#[tauri::command]
pub fn get_supervised_sprint_progress(
    app: tauri::AppHandle,
) -> Result<Option<SprintProgress>, String> {
    let state = app.state::<crate::AppState>();
    let sprint = state
        .current_sprint
        .lock()
        .map_err(|_| "Supervised sprint lock poisoned".to_string())?;
    Ok(sprint.as_ref().map(sprint_progress))
}

#[tauri::command]
pub fn pause_supervised_sprint(app: tauri::AppHandle) -> Result<Option<SprintProgress>, String> {
    let output = {
        let state = app.state::<crate::AppState>();
        let mut sprint = state
            .current_sprint
            .lock()
            .map_err(|_| "Supervised sprint lock poisoned".to_string())?;
        let Some(plan) = sprint.as_mut() else {
            return Ok(None);
        };
        pause_sprint(plan);
        Some((sprint_progress(plan), plan.clone()))
    };
    let Some((progress, plan)) = output else {
        return Ok(None);
    };
    persist_sprint_plan(&app, &plan)?;
    Ok(Some(progress))
}

#[tauri::command]
pub fn resume_supervised_sprint(app: tauri::AppHandle) -> Result<Option<SprintProgress>, String> {
    let output = {
        let state = app.state::<crate::AppState>();
        let mut sprint = state
            .current_sprint
            .lock()
            .map_err(|_| "Supervised sprint lock poisoned".to_string())?;
        let Some(plan) = sprint.as_mut() else {
            return Ok(None);
        };
        let _ = resume_sprint(plan);
        Some((sprint_progress(plan), plan.clone()))
    };
    let Some((progress, plan)) = output else {
        return Ok(None);
    };
    persist_sprint_plan(&app, &plan)?;
    Ok(Some(progress))
}

#[tauri::command]
pub fn cancel_supervised_sprint(app: tauri::AppHandle) -> Result<Option<SprintProgress>, String> {
    let output = {
        let state = app.state::<crate::AppState>();
        let mut sprint = state
            .current_sprint
            .lock()
            .map_err(|_| "Supervised sprint lock poisoned".to_string())?;
        let Some(plan) = sprint.as_mut() else {
            return Ok(None);
        };
        cancel_sprint(plan);
        Some((sprint_progress(plan), plan.clone()))
    };
    let Some((progress, plan)) = output else {
        return Ok(None);
    };
    persist_sprint_plan(&app, &plan)?;
    Ok(Some(progress))
}

#[tauri::command]
pub fn checkpoint_supervised_sprint(
    app: tauri::AppHandle,
    source: String,
) -> Result<Option<SprintCheckpoint>, String> {
    let output = {
        let state = app.state::<crate::AppState>();
        let mut sprint = state
            .current_sprint
            .lock()
            .map_err(|_| "Supervised sprint lock poisoned".to_string())?;
        let Some(plan) = sprint.as_mut() else {
            return Ok(None);
        };
        let checkpoint = checkpoint_sprint(plan, &source);
        Some((checkpoint, plan.clone()))
    };
    let Some((checkpoint, plan)) = output else {
        return Ok(None);
    };
    persist_sprint_checkpoint(&app, &checkpoint)?;
    persist_sprint_plan(&app, &plan)?;
    Ok(Some(checkpoint))
}

#[tauri::command]
pub fn record_supervised_sprint_budget_usage(
    app: tauri::AppHandle,
    spent_micros: u64,
) -> Result<Option<SprintProgress>, String> {
    let output = {
        let state = app.state::<crate::AppState>();
        let mut sprint = state
            .current_sprint
            .lock()
            .map_err(|_| "Supervised sprint lock poisoned".to_string())?;
        let Some(plan) = sprint.as_mut() else {
            return Ok(None);
        };
        record_budget_usage(plan, spent_micros);
        if budget_ceiling_reached(plan) {
            plan.status = "paused".to_string();
            update_current_chapter_state(plan, None, None, None, Some("budget ceiling reached"));
        }
        Some((sprint_progress(plan), plan.clone()))
    };
    let Some((progress, plan)) = output else {
        return Ok(None);
    };
    persist_sprint_plan(&app, &plan)?;
    Ok(Some(progress))
}

#[tauri::command]
pub fn repair_chapter_state(
    app: tauri::AppHandle,
    chapter_title: String,
) -> Result<RepairChapterStateResult, String> {
    let project_id = crate::storage::active_project_id(&app)?;
    let content = crate::storage::load_chapter(&app, chapter_title.clone())?;
    let revision = crate::storage::chapter_revision(&app, &chapter_title)?;

    // Idempotency guard: skip repair if settlement already exists for this chapter+revision.
    // Compare by deserialized fields (not filename substring) to avoid cross-chapter false matches.
    let project_dir = crate::storage::active_project_data_dir(&app)?;
    let runtime_dir = project_dir.join("chapter_runtime");
    if let Ok(entries) = std::fs::read_dir(&runtime_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if !name.ends_with(".settlement.json") {
                continue;
            }
            let raw = match std::fs::read_to_string(entry.path()) {
                Ok(r) => r,
                Err(_) => continue,
            };
            let existing: crate::chapter_generation::ChapterSettlementDelta =
                match serde_json::from_str(&raw) {
                    Ok(d) => d,
                    Err(_) => continue,
                };
            if existing.chapter_title == chapter_title && existing.chapter_revision == revision {
                return Ok(RepairChapterStateResult {
                    chapter_title: chapter_title.clone(),
                    revision: revision.clone(),
                    settlement_delta: existing,
                    settlement_apply: Default::default(),
                    artifact_refs: Vec::new(),
                    settlement_replay_matches: None,
                    already_repaired: true,
                });
            }
        }
    }

    let memory_path = crate::storage::active_project_data_dir(&app)?
        .join(crate::storage::WRITER_MEMORY_DB_FILENAME);
    let memory =
        crate::writer_agent::memory::WriterMemory::open(&memory_path).map_err(|e| e.to_string())?;
    let project = crate::chapter_generation::TauriChapterGenerationProject::new(app.clone())?;
    let context = crate::chapter_generation::build_chapter_context(
        &project,
        crate::chapter_generation::BuildChapterContextInput {
            request_id: crate::chapter_generation::make_request_id("repair-state"),
            target_chapter_title: Some(chapter_title.clone()),
            target_chapter_number: None,
            user_instruction: format!(
                "Repair chapter settlement state for '{}' without rewriting the chapter.",
                chapter_title
            ),
            budget: crate::chapter_generation::ChapterContextBudget::default(),
            chapter_contract: crate::chapter_generation::ChapterContract::default(),
            chapter_summary_override: None,
            user_profile_entries: crate::collect_user_profile_entries(&app).unwrap_or_default(),
            compiled_input: None,
            open_promise_count: memory
                .get_open_promises()
                .ok()
                .map(|p| p.len())
                .unwrap_or(0),
        },
    )
    .map_err(|error| error.message.clone())?;
    let saved = crate::chapter_generation::SaveGeneratedChapterOutput {
        chapter_title: chapter_title.clone(),
        new_revision: revision.clone(),
        saved_mode: "repaired_state".to_string(),
        output_chars: content.chars().count(),
    };
    let delta = crate::chapter_generation::build_basic_chapter_settlement_delta(
        &project_id,
        &chapter_title,
        &revision,
        &content,
        crate::agent_runtime::now_ms(),
        &memory,
        Vec::new(),
    );
    // Snapshot chronology before repair to verify it is preserved
    let chronology_before: Vec<String> = memory
        .list_recent_chapter_results(&project_id, 20)
        .unwrap_or_default()
        .iter()
        .map(|r| r.chapter_title.clone())
        .collect();

    let settlement_apply = crate::writer_agent::settlement_apply::apply_chapter_settlement_delta(
        &memory,
        &project_id,
        &delta,
    )?;

    // Verify settlement is replayable
    let replay = crate::chapter_generation::replay_settlement_extraction(&delta, &content, &memory);
    if !replay.matches_original {
        tracing::warn!(
            chapter = chapter_title,
            mismatches = replay.mismatches.join("; "),
            "Settlement replay produced different results"
        );
    }

    // Rebuild character state versions from settlement deltas
    if !delta.character_state_deltas.is_empty() {
        for cs_delta in &delta.character_state_deltas {
            if let Ok(Some(character)) = memory.get_character_by_name(&cs_delta.character_name) {
                let _ =
                    memory.close_active_states_for_character(character.id, &cs_delta.chapter_title);
                let _ = memory.upsert_character_state(
                    character.id,
                    &cs_delta.chapter_title,
                    &serde_json::json!(cs_delta.core_commitments),
                    &cs_delta.goal_state,
                    &serde_json::json!({}),
                    &[],
                    &cs_delta.source_ref,
                );
            }
        }
    }

    // Rebuild character relationships from settlement deltas
    if !delta.relationship_deltas.is_empty() {
        for rel_delta in &delta.relationship_deltas {
            if !rel_delta.character_a_name.is_empty() && !rel_delta.character_b_name.is_empty() {
                if let (Ok(Some(a)), Ok(Some(b))) = (
                    memory.get_character_by_name(&rel_delta.character_a_name),
                    memory.get_character_by_name(&rel_delta.character_b_name),
                ) {
                    let _ = memory.upsert_relationship(
                        a.id,
                        b.id,
                        &rel_delta.relation_type,
                        &rel_delta.visibility,
                        &rel_delta.chapter_title,
                        &rel_delta.source_ref,
                    );
                }
            }
        }
    }

    // Rebuild knowledge state from settlement deltas
    if !delta.knowledge_deltas.is_empty() {
        for kd in &delta.knowledge_deltas {
            if let Ok(knowledge_id) =
                memory.upsert_knowledge_item(&kd.topic, &kd.truth_state, &kd.source_ref)
            {
                let _ = memory.upsert_knowledge_ownership(
                    knowledge_id,
                    &kd.holder_type,
                    kd.holder_id,
                    &kd.knowledge_mode,
                    &kd.chapter_title,
                    &kd.source_ref,
                );
                if kd.knowledge_mode == "aware" {
                    let _ = memory.record_reveal_event(
                        knowledge_id,
                        "knowledge",
                        "public",
                        &kd.chapter_title,
                        &kd.source_ref,
                    );
                }
            }
        }
    }

    // Rebuild identity state from settlement deltas
    if !delta.identity_deltas.is_empty() {
        for id_delta in &delta.identity_deltas {
            if let Ok(Some(character)) = memory.get_character_by_name(&id_delta.character_name) {
                if let Ok(Some(existing)) =
                    memory.get_active_identity(character.id, &id_delta.chapter_title)
                {
                    let _ = memory.close_identity_layer(existing.id, &id_delta.chapter_title);
                }
                let _ = memory.upsert_identity_layer(
                    character.id,
                    &id_delta.public_identity,
                    &id_delta.private_identity,
                    &id_delta.revealed_to,
                    &id_delta.chapter_title,
                );
            }
        }
    }

    let telemetry = crate::chapter_generation::ChapterLengthTelemetry {
        target_chars: context.chapter_contract.target_chars,
        min_chars: context.chapter_contract.min_chars,
        max_chars: context.chapter_contract.max_chars,
        save_hard_floor_chars: context.chapter_contract.save_hard_floor_chars,
        save_hard_ceiling_chars: context.chapter_contract.save_hard_ceiling_chars,
        draft_chars: None,
        final_chars: Some(saved.output_chars),
        continuation_applied: false,
        compress_applied: false,
        hard_compress_applied: false,
        phase_telemetry: Default::default(),
        warning: None,
    };
    let artifacts = crate::chapter_generation::persist_chapter_runtime_artifacts(
        &project,
        &context.request_id,
        &context,
        &delta,
        &telemetry,
        &content,
    )?;
    crate::audit_project_file_write(
        &app,
        &chapter_title,
        &format!("Repair chapter state: {}", chapter_title),
        "repair_chapter_state",
        &format!(
            "Rebuilt settlement/runtime artifacts for '{}' at revision {} without rewriting chapter text.",
            chapter_title, revision
        ),
        &artifacts.artifact_refs,
    );
    crate::observe_chapter_save(&app, &chapter_title, &content, &revision)?;

    // Verify chronology was preserved by the repair
    let chronology_after: Vec<String> = memory
        .list_recent_chapter_results(&project_id, 20)
        .unwrap_or_default()
        .iter()
        .map(|r| r.chapter_title.clone())
        .collect();
    if chronology_before != chronology_after {
        tracing::error!(
            chapter = chapter_title,
            before = ?chronology_before,
            after = ?chronology_after,
            "repair_chapter_state altered chapter chronology"
        );
    }

    Ok(RepairChapterStateResult {
        chapter_title,
        revision,
        settlement_delta: delta,
        settlement_apply,
        artifact_refs: artifacts.artifact_refs,
        settlement_replay_matches: Some(replay.matches_original),
        already_repaired: false,
    })
}
