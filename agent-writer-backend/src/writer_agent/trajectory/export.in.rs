pub fn export_trace_snapshot(
    project_id: &str,
    session_id: &str,
    snapshot: &WriterAgentTraceSnapshot,
) -> WriterTrajectoryExport {
    let mut lines = Vec::new();
    let mut seq = 0usize;
    let trace_id = format!("{}:{}", project_id, session_id);

    for observation in &snapshot.recent_observations {
        seq += 1;
        push_event(
            &mut lines,
            TrajectoryEvent {
                trace_schema: TRAJECTORY_SCHEMA,
                schema_version: SCHEMA_VERSION,
                trace_id: &trace_id,
                project_id,
                session_id,
                seq,
                event_type: "writer.observation",
                ts_ms: observation.created_at,
                data: observation,
            },
        );
    }
    for task_packet in &snapshot.task_packets {
        seq += 1;
        push_event(
            &mut lines,
            TrajectoryEvent {
                trace_schema: TRAJECTORY_SCHEMA,
                schema_version: SCHEMA_VERSION,
                trace_id: &trace_id,
                project_id,
                session_id,
                seq,
                event_type: "writer.task_packet",
                ts_ms: task_packet.packet.created_at_ms,
                data: task_packet,
            },
        );
    }
    for proposal in &snapshot.recent_proposals {
        seq += 1;
        push_event(
            &mut lines,
            TrajectoryEvent {
                trace_schema: TRAJECTORY_SCHEMA,
                schema_version: SCHEMA_VERSION,
                trace_id: &trace_id,
                project_id,
                session_id,
                seq,
                event_type: "writer.proposal",
                ts_ms: 0,
                data: proposal,
            },
        );
    }
    for feedback in &snapshot.recent_feedback {
        seq += 1;
        push_event(
            &mut lines,
            TrajectoryEvent {
                trace_schema: TRAJECTORY_SCHEMA,
                schema_version: SCHEMA_VERSION,
                trace_id: &trace_id,
                project_id,
                session_id,
                seq,
                event_type: "writer.feedback",
                ts_ms: feedback.created_at,
                data: feedback,
            },
        );
    }
    for lifecycle in &snapshot.operation_lifecycle {
        seq += 1;
        push_event(
            &mut lines,
            TrajectoryEvent {
                trace_schema: TRAJECTORY_SCHEMA,
                schema_version: SCHEMA_VERSION,
                trace_id: &trace_id,
                project_id,
                session_id,
                seq,
                event_type: "writer.operation_lifecycle",
                ts_ms: lifecycle.created_at,
                data: lifecycle,
            },
        );
    }
    for run_event in &snapshot.run_events {
        seq += 1;
        push_event(
            &mut lines,
            TrajectoryEvent {
                trace_schema: TRAJECTORY_SCHEMA,
                schema_version: SCHEMA_VERSION,
                trace_id: &trace_id,
                project_id,
                session_id,
                seq,
                event_type: "writer.run_event",
                ts_ms: run_event.ts_ms,
                data: run_event,
            },
        );
    }
    for report in &snapshot.post_write_diagnostics {
        seq += 1;
        push_event(
            &mut lines,
            TrajectoryEvent {
                trace_schema: TRAJECTORY_SCHEMA,
                schema_version: SCHEMA_VERSION,
                trace_id: &trace_id,
                project_id,
                session_id,
                seq,
                event_type: "writer.post_write_diagnostics",
                ts_ms: report.created_at_ms,
                data: report,
            },
        );
    }
    for recall in &snapshot.context_recalls {
        seq += 1;
        push_event(
            &mut lines,
            TrajectoryEvent {
                trace_schema: TRAJECTORY_SCHEMA,
                schema_version: SCHEMA_VERSION,
                trace_id: &trace_id,
                project_id,
                session_id,
                seq,
                event_type: "writer.context_recall",
                ts_ms: recall.last_recalled_at,
                data: recall,
            },
        );
    }
    seq += 1;
    push_event(
        &mut lines,
        TrajectoryEvent {
            trace_schema: TRAJECTORY_SCHEMA,
            schema_version: SCHEMA_VERSION,
            trace_id: &trace_id,
            project_id,
            session_id,
            seq,
            event_type: "writer.metacognition",
            ts_ms: snapshot
                .run_events
                .iter()
                .map(|event| event.ts_ms)
                .max()
                .or_else(|| {
                    snapshot
                        .recent_feedback
                        .iter()
                        .map(|feedback| feedback.created_at)
                        .max()
                })
                .unwrap_or(0),
            data: &snapshot.metacognitive_snapshot,
        },
    );
    seq += 1;
    push_event(
        &mut lines,
        TrajectoryEvent {
            trace_schema: TRAJECTORY_SCHEMA,
            schema_version: SCHEMA_VERSION,
            trace_id: &trace_id,
            project_id,
            session_id,
            seq,
            event_type: "writer.product_metrics_trend",
            ts_ms: snapshot
                .product_metrics_trend
                .recent_sessions
                .iter()
                .map(|session| session.last_event_at)
                .max()
                .unwrap_or(0),
            data: &snapshot.product_metrics_trend,
        },
    );
    seq += 1;
    push_event(
        &mut lines,
        TrajectoryEvent {
            trace_schema: TRAJECTORY_SCHEMA,
            schema_version: SCHEMA_VERSION,
            trace_id: &trace_id,
            project_id,
            session_id,
            seq,
            event_type: "writer.product_metrics",
            ts_ms: snapshot
                .recent_feedback
                .iter()
                .map(|feedback| feedback.created_at)
                .max()
                .unwrap_or(0),
            data: &snapshot.product_metrics,
        },
    );

    let redaction_warning = redaction_warning();
    let trace_viewer_lines = trace_viewer_jsonl_lines(
        project_id,
        session_id,
        &trace_id,
        &lines,
        &redaction_warning,
    );
    WriterTrajectoryExport {
        schema: TRAJECTORY_SCHEMA.to_string(),
        schema_version: SCHEMA_VERSION,
        project_id: project_id.to_string(),
        session_id: session_id.to_string(),
        redaction_warning,
        local_only: true,
        event_count: lines.len(),
        jsonl: if lines.is_empty() {
            String::new()
        } else {
            format!("{}\n", lines.join("\n"))
        },
        trace_viewer_schema: "claude-code-jsonl-for-hf-agent-trace-viewer".to_string(),
        trace_viewer_event_count: trace_viewer_lines.len(),
        trace_viewer_jsonl: if trace_viewer_lines.is_empty() {
            String::new()
        } else {
            format!("{}\n", trace_viewer_lines.join("\n"))
        },
    }
}
