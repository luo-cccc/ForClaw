use serde::Serialize;
use tauri::{Emitter, Manager};

use crate::{
    agent_runtime, events, llm_runtime, lock_editor_prediction, resolve_api_key, writer_agent,
    AppState,
};

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct EditorGhostChunk {
    request_id: String,
    proposal_id: Option<String>,
    operation: Option<writer_agent::operation::WriterOperation>,
    cursor_position: usize,
    content: String,
    intent: Option<String>,
    candidates: Vec<EditorGhostCandidate>,
    replace: bool,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct EditorGhostCandidate {
    id: String,
    label: String,
    text: String,
    #[serde(default)]
    evidence: Vec<EditorGhostEvidence>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct EditorGhostEvidence {
    source: String,
    snippet: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct EditorGhostEnd {
    request_id: String,
    cursor_position: usize,
    reason: String,
}

#[derive(Debug, Clone)]
pub(crate) struct EditorGhostRenderTarget {
    pub(crate) request_id: String,
    pub(crate) cursor_position: usize,
}

pub(crate) fn realtime_cowrite_enabled() -> bool {
    std::env::var("AGENT_WRITER_REALTIME_COWRITE")
        .map(|value| {
            let normalized = value.trim().to_lowercase();
            !matches!(normalized.as_str(), "0" | "false" | "off" | "disabled")
        })
        .unwrap_or(true)
}

pub(crate) fn emit_editor_ghost_end(
    app: &tauri::AppHandle,
    target: &EditorGhostRenderTarget,
    reason: &str,
) -> Result<(), String> {
    app.emit(
        events::EDITOR_GHOST_END,
        EditorGhostEnd {
            request_id: target.request_id.clone(),
            cursor_position: target.cursor_position,
            reason: reason.to_string(),
        },
    )
    .map_err(|e| format!("Failed to emit editor ghost end: {}", e))?;
    clear_editor_prediction_for_output(app, Some(&target.request_id));
    Ok(())
}

pub(crate) fn emit_writer_ghost_proposal(
    app: &tauri::AppHandle,
    target: &EditorGhostRenderTarget,
    proposal: &writer_agent::proposal::AgentProposal,
    replace: bool,
    complete: bool,
) -> Result<(), String> {
    let candidates = ghost_candidates_from_proposal(
        proposal,
        if replace { "llm" } else { "a" },
        if replace {
            "AI 增强"
        } else {
            "A 内核接力"
        },
    );
    let Some(first_candidate) = candidates.first() else {
        return Ok(());
    };
    let content = first_candidate.text.clone();
    app.emit(
        events::EDITOR_GHOST_CHUNK,
        EditorGhostChunk {
            request_id: target.request_id.clone(),
            proposal_id: Some(proposal.id.clone()),
            operation: proposal.operations.first().cloned(),
            cursor_position: target.cursor_position,
            content,
            intent: ghost_intent_label(proposal),
            candidates,
            replace,
        },
    )
    .map_err(|e| format!("Failed to emit editor ghost: {}", e))?;
    if complete {
        emit_editor_ghost_end(app, target, "complete")?;
    }
    Ok(())
}

pub(crate) fn emit_ambient_output(app: &tauri::AppHandle, output: agent_harness_core::AgentOutput) {
    match output {
        agent_harness_core::AgentOutput::GhostText {
            request_id,
            text,
            position,
        } => {
            let request_id_value =
                request_id.unwrap_or_else(|| format!("ambient-{}", agent_runtime::now_ms()));
            let content = trim_ghost_completion(&text);
            if content.is_empty() {
                return;
            }
            let _ = app.emit(
                events::EDITOR_GHOST_CHUNK,
                EditorGhostChunk {
                    request_id: request_id_value.clone(),
                    proposal_id: None,
                    operation: None,
                    cursor_position: position,
                    content,
                    intent: None,
                    candidates: Vec::new(),
                    replace: false,
                },
            );
            let _ = app.emit(
                events::EDITOR_GHOST_END,
                EditorGhostEnd {
                    request_id: request_id_value.clone(),
                    cursor_position: position,
                    reason: "complete".to_string(),
                },
            );
            clear_editor_prediction_for_output(app, Some(&request_id_value));
        }
        agent_harness_core::AgentOutput::MultiGhost {
            request_id,
            position,
            intent,
            candidates,
        } => {
            let request_id_value =
                request_id.unwrap_or_else(|| format!("ambient-{}", agent_runtime::now_ms()));
            let candidates = candidates
                .into_iter()
                .map(|candidate| EditorGhostCandidate {
                    id: candidate.id,
                    label: candidate.label,
                    text: trim_ghost_completion(&candidate.text),
                    evidence: vec![],
                })
                .filter(|candidate| !candidate.text.is_empty())
                .collect::<Vec<_>>();
            let content = candidates
                .first()
                .map(|candidate| candidate.text.clone())
                .unwrap_or_default();
            if content.is_empty() {
                return;
            }
            let _ = app.emit(
                events::EDITOR_GHOST_CHUNK,
                EditorGhostChunk {
                    request_id: request_id_value.clone(),
                    proposal_id: None,
                    operation: None,
                    cursor_position: position,
                    content,
                    intent: Some(intent),
                    candidates,
                    replace: false,
                },
            );
            let _ = app.emit(
                events::EDITOR_GHOST_END,
                EditorGhostEnd {
                    request_id: request_id_value.clone(),
                    cursor_position: position,
                    reason: "complete".to_string(),
                },
            );
            clear_editor_prediction_for_output(app, Some(&request_id_value));
        }
        agent_harness_core::AgentOutput::GhostEnd {
            request_id,
            position,
            reason,
        } => {
            let request_id_value =
                request_id.unwrap_or_else(|| format!("ambient-{}", agent_runtime::now_ms()));
            let _ = app.emit(
                events::EDITOR_GHOST_END,
                EditorGhostEnd {
                    request_id: request_id_value.clone(),
                    cursor_position: position,
                    reason,
                },
            );
            clear_editor_prediction_for_output(app, Some(&request_id_value));
        }
        agent_harness_core::AgentOutput::SemanticLint {
            message,
            from,
            to,
            severity,
        } => {
            let _ = app.emit(
                events::EDITOR_SEMANTIC_LINT,
                crate::EditorSemanticLint {
                    request_id: format!("ambient-lint-{}", agent_runtime::now_ms()),
                    cursor_position: to,
                    from,
                    to,
                    message,
                    severity,
                },
            );
        }
        agent_harness_core::AgentOutput::HoverHint { message, from, to } => {
            let _ = app.emit(
                events::EDITOR_HOVER_HINT,
                serde_json::json!({
                    "message": message,
                    "from": from,
                    "to": to,
                }),
            );
        }
        agent_harness_core::AgentOutput::EntityCard {
            keyword,
            content,
            chapter,
        } => {
            let _ = app.emit(
                events::EDITOR_ENTITY_CARD,
                serde_json::json!({
                    "keyword": keyword,
                    "content": content,
                    "chapter": chapter,
                }),
            );
        }
        agent_harness_core::AgentOutput::StoryboardMarker {
            chapter,
            message,
            level,
        } => {
            let _ = app.emit(
                events::STORYBOARD_MARKER,
                serde_json::json!({
                    "chapter": chapter,
                    "message": message,
                    "level": level,
                }),
            );
        }
        agent_harness_core::AgentOutput::Epiphany { skill, category } => {
            let _ = app.emit(
                events::AGENT_EPIPHANY,
                serde_json::json!({
                    "skill": skill,
                    "category": category,
                }),
            );
        }
        agent_harness_core::AgentOutput::None => {}
    }
}

pub(crate) fn abort_editor_prediction_task(
    state: &AppState,
    request_id: Option<&str>,
) -> Result<bool, String> {
    let mut task = lock_editor_prediction(state)?;
    let should_cancel = match (&*task, request_id) {
        (Some(active), Some(request_id)) => active.request_id == request_id,
        (Some(_), None) => true,
        (None, _) => false,
    };

    if should_cancel {
        if let Some(active) = task.take() {
            active.cancel.cancel();
        }
        return Ok(true);
    }

    Ok(false)
}

pub(crate) fn spawn_llm_ghost_proposal(
    app: tauri::AppHandle,
    observation: writer_agent::observation::WriterObservation,
    context_pack: writer_agent::context::WritingContextPack,
    render_target: Option<EditorGhostRenderTarget>,
) {
    let Some(api_key) = resolve_api_key() else {
        return;
    };
    let settings = llm_runtime::settings(api_key);
    let model = settings.model.clone();
    let messages = writer_agent_ghost_messages(&observation, &context_pack);

    tauri::async_runtime::spawn(async move {
        let target_for_error = render_target.clone();

        // Provider budget preflight for ghost text generation
        let _budget_report = writer_agent::provider_budget::evaluate_provider_budget(
            writer_agent::provider_budget::WriterProviderBudgetRequest::new(
                writer_agent::provider_budget::WriterProviderBudgetTask::GhostPreview,
                &model,
                2000,
                160,
            ),
        );
        tracing::info!(
            task = "ghost_preview",
            decision = ?_budget_report.decision,
            tokens = _budget_report.estimated_total_tokens,
            "Provider budget preflight"
        );

        let text = match llm_runtime::chat_text_profile(
            &settings,
            messages,
            llm_runtime::LlmRequestProfile::GhostPreview,
            12,
        )
        .await
        {
            Ok(text) => text,
            Err(e) => {
                tracing::warn!("Writer Agent LLM ghost proposal failed: {}", e);
                if let Some(target) = target_for_error {
                    let _ = emit_editor_ghost_end(&app, &target, "complete");
                }
                return;
            }
        };

        if text.trim().is_empty() {
            if let Some(target) = target_for_error {
                let _ = emit_editor_ghost_end(&app, &target, "complete");
            }
            return;
        }

        let state = app.state::<AppState>();
        let proposal = {
            let mut kernel = match state.writer_kernel.lock() {
                Ok(kernel) => kernel,
                Err(e) => {
                    tracing::error!("Writer kernel lock poisoned: {}", e);
                    return;
                }
            };
            match kernel.create_llm_ghost_proposal(observation, text, &model) {
                Ok(proposal) => proposal,
                Err(e) => {
                    tracing::warn!("Writer Agent rejected LLM proposal: {}", e);
                    if let Some(target) = target_for_error {
                        let _ = emit_editor_ghost_end(&app, &target, "complete");
                    }
                    return;
                }
            }
        };

        let _ = app.emit(events::AGENT_PROPOSAL, proposal.clone());
        if let Some(target) = render_target {
            let _ = emit_writer_ghost_proposal(&app, &target, &proposal, true, true);
        }
    });
}

fn clear_editor_prediction_task(state: &AppState, request_id: &str) -> Result<(), String> {
    let mut task = lock_editor_prediction(state)?;
    if task
        .as_ref()
        .is_some_and(|active| active.request_id == request_id)
    {
        *task = None;
    }
    Ok(())
}

fn clear_editor_prediction_for_output(app: &tauri::AppHandle, request_id: Option<&str>) {
    let Some(request_id) = request_id else {
        return;
    };
    let state = app.state::<AppState>();
    let _ = clear_editor_prediction_task(&state, request_id);
}

fn writer_agent_ghost_messages(
    observation: &writer_agent::observation::WriterObservation,
    pack: &writer_agent::context::WritingContextPack,
) -> Vec<serde_json::Value> {
    let context = crate::render_writer_context_pack(pack);
    vec![
        serde_json::json!({
            "role": "system",
            "content": "你是一个中文长篇小说写作 Agent，不是聊天助手。你只负责在光标处提供可直接插入正文的一小段续写。必须尊重已给出的设定、伏笔、风格偏好和光标后文。不要解释，不要 Markdown，不要重复光标前文。输出 1-2 句中文正文。"
        }),
        serde_json::json!({
            "role": "user",
            "content": format!(
                "章节: {}\n光标位置: {}\n当前段落:\n{}\n\nContextPack:\n{}\n\n请输出光标处续写正文:",
                observation.chapter_title.as_deref().unwrap_or("current chapter"),
                observation.cursor.as_ref().map(|c| c.to).unwrap_or(0),
                observation.paragraph,
                context
            )
        }),
    ]
}

fn trim_ghost_completion(text: &str) -> String {
    let without_markers = text
        .replace("<|fim_middle|>", "")
        .replace("<|fim_prefix|>", "")
        .replace("<|fim_suffix|>", "");
    let trimmed = without_markers.trim_matches(|c: char| c == '`' || c.is_whitespace());
    trimmed.chars().take(180).collect::<String>()
}

fn clean_ghost_candidate_text(text: &str) -> String {
    trim_ghost_completion(text)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn ghost_intent_label(proposal: &writer_agent::proposal::AgentProposal) -> Option<String> {
    proposal
        .rationale
        .split("意图识别:")
        .nth(1)
        .and_then(|tail| tail.split_whitespace().next())
        .map(|label| label.trim_matches(|c: char| c == '.' || c == ',' || c == '，'))
        .filter(|label| !label.is_empty())
        .map(str::to_string)
}

fn proposal_to_ghost_candidate(
    proposal: &writer_agent::proposal::AgentProposal,
    id: &str,
    label: &str,
) -> Option<EditorGhostCandidate> {
    let text = clean_ghost_candidate_text(&proposal.preview);
    if text.is_empty() {
        return None;
    }
    Some(EditorGhostCandidate {
        id: id.to_string(),
        label: label.to_string(),
        text,
        evidence: vec![],
    })
}

fn ghost_candidates_from_proposal(
    proposal: &writer_agent::proposal::AgentProposal,
    default_id: &str,
    default_label: &str,
) -> Vec<EditorGhostCandidate> {
    let mut candidates = proposal
        .alternatives
        .iter()
        .filter_map(|alternative| {
            let text = clean_ghost_candidate_text(&alternative.preview);
            if text.is_empty() {
                return None;
            }
            Some(EditorGhostCandidate {
                id: alternative.id.clone(),
                label: alternative.label.clone(),
                text,
                evidence: alternative
                    .evidence
                    .iter()
                    .map(|e| EditorGhostEvidence {
                        source: format!("{:?}", e.source),
                        snippet: e.snippet.clone(),
                    })
                    .collect(),
            })
        })
        .collect::<Vec<_>>();

    if candidates.is_empty() {
        if let Some(candidate) = proposal_to_ghost_candidate(proposal, default_id, default_label) {
            candidates.push(candidate);
        }
    }

    candidates
}
