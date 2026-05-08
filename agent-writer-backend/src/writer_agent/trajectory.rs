use chrono::{DateTime, SecondsFormat, Utc};
use serde::Serialize;
use serde_json::{json, Value};

use super::kernel::{
    WriterAgentTraceSnapshot, WriterFeedbackTrace, WriterObservationTrace,
    WriterOperationLifecycleTrace, WriterProposalTrace, WriterTaskPacketTrace,
};
use super::memory::ContextRecallSummary;
use super::metacognition::WriterMetacognitiveSnapshot;
use super::run_events::WriterRunEvent;

const TRAJECTORY_SCHEMA: &str = "forge-writer-agent-trajectory";
const SCHEMA_VERSION: u8 = 1;
const MAX_EVENT_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WriterTrajectoryExport {
    pub schema: String,
    pub schema_version: u8,
    pub project_id: String,
    pub session_id: String,
    pub redaction_warning: String,
    pub local_only: bool,
    pub event_count: usize,
    pub jsonl: String,
    pub trace_viewer_schema: String,
    pub trace_viewer_event_count: usize,
    pub trace_viewer_jsonl: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct TrajectoryEvent<'a, T: Serialize> {
    trace_schema: &'static str,
    schema_version: u8,
    trace_id: &'a str,
    project_id: &'a str,
    session_id: &'a str,
    seq: usize,
    event_type: &'static str,
    ts_ms: u64,
    data: T,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct TruncatedEvent {
    truncated: bool,
    original_bytes: usize,
    limit_bytes: usize,
    reason: &'static str,
}

include!("trajectory/export.in.rs");
include!("trajectory/trace_viewer.in.rs");
include!("trajectory/helpers.in.rs");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exports_snapshot_as_jsonl_events() {
        let snapshot = WriterAgentTraceSnapshot {
            recent_observations: vec![WriterObservationTrace {
                id: "obs_1".to_string(),
                created_at: 10,
                reason: "Idle".to_string(),
                chapter_title: Some("Chapter-1".to_string()),
                paragraph_snippet: "林墨停下。".to_string(),
            }],
            task_packets: Vec::new(),
            recent_proposals: Vec::new(),
            recent_feedback: vec![WriterFeedbackTrace {
                proposal_id: "prop_1".to_string(),
                action: "Accepted".to_string(),
                reason: Some("fits".to_string()),
                created_at: 20,
            }],
            operation_lifecycle: Vec::new(),
            run_events: Vec::new(),
            post_write_diagnostics: Vec::new(),
            context_source_trends: Vec::new(),
            context_recalls: Vec::new(),
            product_metrics: Default::default(),
            product_metrics_trend: Default::default(),
            metacognitive_snapshot: Default::default(),
        };

        let export = export_trace_snapshot("novel-a", "session-a", &snapshot);
        let lines = export.jsonl.lines().collect::<Vec<_>>();

        assert_eq!(export.schema, TRAJECTORY_SCHEMA);
        assert_eq!(export.event_count, 5);
        assert_eq!(lines.len(), 5);
        assert!(lines[0].contains("\"eventType\":\"writer.observation\""));
        assert!(lines[1].contains("\"eventType\":\"writer.feedback\""));
        assert!(lines[2].contains("\"eventType\":\"writer.metacognition\""));
        assert!(lines[3].contains("\"eventType\":\"writer.product_metrics_trend\""));
        assert!(lines[4].contains("\"eventType\":\"writer.product_metrics\""));
    }
}
