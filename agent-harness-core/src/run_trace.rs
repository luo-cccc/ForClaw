use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentRunStatus {
    Running,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentRunEventKind {
    Started,
    Observation,
    ContextBuilt,
    ToolSelected,
    ToolFinished,
    LlmDelta,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRunEvent {
    pub sequence: u32,
    pub elapsed_ms: u64,
    pub kind: AgentRunEventKind,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(default)]
    pub metadata: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRunTrace {
    pub run_id: String,
    pub goal: String,
    pub started_at_ms: u64,
    pub status: AgentRunStatus,
    pub events: Vec<AgentRunEvent>,
    pub tool_call_count: u32,
    pub context_chars: usize,
}

impl AgentRunTrace {
    pub fn new(run_id: impl Into<String>, goal: impl Into<String>, started_at_ms: u64) -> Self {
        let mut trace = Self {
            run_id: run_id.into(),
            goal: goal.into(),
            started_at_ms,
            status: AgentRunStatus::Running,
            events: Vec::new(),
            tool_call_count: 0,
            context_chars: 0,
        };
        trace.push(
            AgentRunEventKind::Started,
            "run_started",
            None,
            serde_json::Value::Null,
            started_at_ms,
        );
        trace
    }

    pub fn push(
        &mut self,
        kind: AgentRunEventKind,
        label: impl Into<String>,
        detail: Option<String>,
        metadata: serde_json::Value,
        now_ms: u64,
    ) {
        if matches!(kind, AgentRunEventKind::ToolSelected) {
            self.tool_call_count += 1;
        }

        self.events.push(AgentRunEvent {
            sequence: self.events.len() as u32 + 1,
            elapsed_ms: now_ms.saturating_sub(self.started_at_ms),
            kind,
            label: label.into(),
            detail,
            metadata,
        });
    }

    pub fn record_context_built(&mut self, context_chars: usize, source_count: usize, now_ms: u64) {
        self.context_chars = context_chars;
        self.push(
            AgentRunEventKind::ContextBuilt,
            "context_built",
            Some(format!(
                "{} chars from {} sources",
                context_chars, source_count
            )),
            serde_json::json!({
                "contextChars": context_chars,
                "sourceCount": source_count
            }),
            now_ms,
        );
    }

    pub fn complete(&mut self, now_ms: u64) {
        self.status = AgentRunStatus::Completed;
        self.push(
            AgentRunEventKind::Completed,
            "run_completed",
            None,
            serde_json::Value::Null,
            now_ms,
        );
    }

    pub fn fail(&mut self, error: impl Into<String>, now_ms: u64) {
        let error = error.into();
        self.status = AgentRunStatus::Failed;
        self.push(
            AgentRunEventKind::Failed,
            "run_failed",
            Some(error.clone()),
            serde_json::json!({ "error": error }),
            now_ms,
        );
    }

    pub fn cancel(&mut self, reason: impl Into<String>, now_ms: u64) {
        let reason = reason.into();
        self.status = AgentRunStatus::Cancelled;
        self.push(
            AgentRunEventKind::Cancelled,
            "run_cancelled",
            Some(reason.clone()),
            serde_json::json!({ "reason": reason }),
            now_ms,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trace_records_context_and_completion() {
        let mut trace = AgentRunTrace::new("run-1", "draft a scene", 1_000);
        trace.record_context_built(1200, 3, 1_050);
        trace.complete(1_100);

        assert_eq!(trace.status, AgentRunStatus::Completed);
        assert_eq!(trace.context_chars, 1200);
        assert_eq!(trace.events.len(), 3);
        assert_eq!(trace.events[1].elapsed_ms, 50);
    }
}
