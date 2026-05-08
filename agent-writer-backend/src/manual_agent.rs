//! Manual agent history and turn management. Extracted from lib.rs.

use crate::{writer_agent::memory::ManualAgentTurnSummary, AppState};

const MANUAL_AGENT_HISTORY_MAX_STORED_TURNS: usize = 64;
const MANUAL_AGENT_TURN_USER_CHARS: usize = 1_200;
const MANUAL_AGENT_TURN_ASSISTANT_CHARS: usize = 2_400;
pub(crate) const MANUAL_AGENT_HISTORY_MAX_TURNS: usize = 8;
pub(crate) const MANUAL_AGENT_HISTORY_MAX_CHARS: usize = 12_000;
pub(crate) const MANUAL_AGENT_PERSISTED_HISTORY_LOOKBACK: usize = 32;

#[derive(Debug, Clone)]
pub(crate) struct ManualAgentTurn {
    pub project_id: String,
    pub created_at: u64,
    pub observation_id: String,
    pub chapter_title: Option<String>,
    pub user: String,
    pub assistant: String,
    pub source_refs: Vec<String>,
}

#[derive(Debug, Default)]
pub(crate) struct ManualAgentHistory {
    turns: Vec<ManualAgentTurn>,
}

impl ManualAgentHistory {
    pub fn append(&mut self, turn: ManualAgentTurn) {
        if turn.user.trim().is_empty() && turn.assistant.trim().is_empty() {
            return;
        }

        self.turns.push(turn);
        if self.turns.len() > MANUAL_AGENT_HISTORY_MAX_STORED_TURNS {
            let excess = self.turns.len() - MANUAL_AGENT_HISTORY_MAX_STORED_TURNS;
            self.turns.drain(0..excess);
        }
    }

    pub fn recent_for_project(
        &self,
        project_id: &str,
        max_turns: usize,
        max_chars: usize,
    ) -> Vec<ManualAgentTurn> {
        if max_turns == 0 || max_chars == 0 {
            return Vec::new();
        }

        let mut selected = Vec::new();
        let mut consumed = 0usize;
        for turn in self
            .turns
            .iter()
            .rev()
            .filter(|turn| turn.project_id == project_id)
        {
            if selected.len() >= max_turns {
                break;
            }

            let clipped = ManualAgentTurn {
                project_id: turn.project_id.clone(),
                created_at: turn.created_at,
                observation_id: turn.observation_id.clone(),
                chapter_title: turn.chapter_title.clone(),
                user: crate::char_tail(&turn.user, MANUAL_AGENT_TURN_USER_CHARS),
                assistant: crate::char_tail(&turn.assistant, MANUAL_AGENT_TURN_ASSISTANT_CHARS),
                source_refs: turn.source_refs.iter().take(12).cloned().collect(),
            };
            let cost = clipped.user.chars().count() + clipped.assistant.chars().count();
            if !selected.is_empty() && consumed + cost > max_chars {
                break;
            }

            consumed += cost;
            selected.push(clipped);
        }

        selected.reverse();
        selected
    }
}

impl From<ManualAgentTurnSummary> for ManualAgentTurn {
    fn from(turn: ManualAgentTurnSummary) -> Self {
        Self {
            project_id: turn.project_id,
            created_at: turn.created_at,
            observation_id: turn.observation_id,
            chapter_title: turn.chapter_title,
            user: turn.user,
            assistant: turn.assistant,
            source_refs: turn.source_refs,
        }
    }
}

pub(crate) fn merge_manual_agent_history(
    project_id: &str,
    persisted: Vec<ManualAgentTurn>,
    runtime: Vec<ManualAgentTurn>,
    max_turns: usize,
    max_chars: usize,
) -> Vec<ManualAgentTurn> {
    let mut merged = persisted;
    for turn in runtime {
        let duplicate = merged.iter().any(|existing| {
            !turn.observation_id.is_empty() && existing.observation_id == turn.observation_id
        });
        if !duplicate {
            merged.push(turn);
        }
    }
    merged.sort_by_key(|turn| turn.created_at);

    let history = ManualAgentHistory { turns: merged };
    history.recent_for_project(project_id, max_turns, max_chars)
}

pub(crate) fn lock_manual_agent_history(
    state: &AppState,
) -> Result<std::sync::MutexGuard<'_, ManualAgentHistory>, String> {
    state
        .manual_agent_history
        .lock()
        .map_err(|_| "Manual agent history lock poisoned".to_string())
}

pub(crate) fn manual_agent_history_messages(
    history: &[ManualAgentTurn],
) -> Vec<agent_harness_core::provider::LlmMessage> {
    use agent_harness_core::provider::LlmMessage;
    let mut messages = Vec::with_capacity(history.len() * 2);
    for turn in history {
        messages.push(LlmMessage {
            role: "user".to_string(),
            content: Some(format!(
                "[Earlier manual request, project={}, at={}]\n{}",
                turn.project_id, turn.created_at, turn.user
            )),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        });

        let source_note = if turn.source_refs.is_empty() {
            String::new()
        } else {
            format!(
                "\n\n[Context sources used: {}]",
                turn.source_refs.join(", ")
            )
        };
        messages.push(LlmMessage {
            role: "assistant".to_string(),
            content: Some(format!("{}{}", turn.assistant, source_note)),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        });
    }
    messages
}
