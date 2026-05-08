//! Append-only run events for replayable Writer Agent timelines.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WriterRunEvent {
    pub seq: u64,
    pub ts_ms: u64,
    pub project_id: String,
    pub session_id: String,
    pub task_id: Option<String>,
    pub event_type: String,
    pub source_refs: Vec<String>,
    pub data: serde_json::Value,
}

#[derive(Debug, Clone, Default)]
pub struct WriterRunEventStore {
    events: Vec<WriterRunEvent>,
    next_seq: u64,
}

impl WriterRunEventStore {
    pub fn append(
        &mut self,
        project_id: &str,
        session_id: &str,
        event_type: impl Into<String>,
        ts_ms: u64,
        task_id: Option<String>,
        source_refs: Vec<String>,
        data: serde_json::Value,
    ) -> WriterRunEvent {
        self.next_seq = self.next_seq.saturating_add(1);
        let event = WriterRunEvent {
            seq: self.next_seq,
            ts_ms,
            project_id: project_id.to_string(),
            session_id: session_id.to_string(),
            task_id,
            event_type: event_type.into(),
            source_refs: normalize_source_refs(source_refs),
            data,
        };
        self.events.push(event.clone());
        event
    }

    pub fn recent(&self, limit: usize) -> Vec<WriterRunEvent> {
        let limit = limit.min(self.events.len());
        self.events[self.events.len().saturating_sub(limit)..].to_vec()
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.events.len()
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}

pub fn normalize_source_refs(source_refs: Vec<String>) -> Vec<String> {
    let mut normalized = Vec::new();
    for source_ref in source_refs {
        let source_ref = source_ref.trim();
        if source_ref.is_empty() || normalized.iter().any(|existing| existing == source_ref) {
            continue;
        }
        normalized.push(source_ref.to_string());
    }
    normalized
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appends_events_with_monotonic_seq() {
        let mut store = WriterRunEventStore::default();
        store.append(
            "project",
            "session",
            "observation",
            10,
            Some("obs-1".to_string()),
            vec!["Chapter-1".to_string()],
            serde_json::json!({"reason": "Idle"}),
        );
        store.append(
            "project",
            "session",
            "proposal_created",
            11,
            Some("prop-1".to_string()),
            vec!["Chapter-1".to_string(), "Chapter-1".to_string()],
            serde_json::json!({"kind": "Ghost"}),
        );

        let events = store.recent(10);
        assert_eq!(store.len(), 2);
        assert_eq!(events[0].seq, 1);
        assert_eq!(events[1].seq, 2);
        assert_eq!(events[1].source_refs, vec!["Chapter-1"]);
    }

    #[test]
    fn recent_preserves_timeline_order() {
        let mut store = WriterRunEventStore::default();
        for index in 0..5 {
            store.append(
                "project",
                "session",
                "event",
                index,
                None,
                Vec::new(),
                serde_json::json!({ "index": index }),
            );
        }

        let events = store.recent(3);
        assert_eq!(
            events.iter().map(|event| event.seq).collect::<Vec<_>>(),
            vec![3, 4, 5]
        );
    }
}
