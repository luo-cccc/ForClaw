fn redaction_warning() -> String {
    "Trajectory export may contain manuscript text, project memory, author feedback, prompts, tool results, and internal reasoning metadata. Review or redact before sharing; exports are local-only by default.".to_string()
}

fn push_event<T: Serialize>(lines: &mut Vec<String>, event: TrajectoryEvent<'_, T>) {
    let Ok(line) = serde_json::to_string(&event) else {
        return;
    };
    if line.len() <= MAX_EVENT_BYTES {
        lines.push(line);
        return;
    }

    let truncated = TrajectoryEvent {
        trace_schema: TRAJECTORY_SCHEMA,
        schema_version: SCHEMA_VERSION,
        trace_id: event.trace_id,
        project_id: event.project_id,
        session_id: event.session_id,
        seq: event.seq,
        event_type: event.event_type,
        ts_ms: event.ts_ms,
        data: TruncatedEvent {
            truncated: true,
            original_bytes: line.len(),
            limit_bytes: MAX_EVENT_BYTES,
            reason: "trajectory-event-size-limit",
        },
    };
    if let Ok(line) = serde_json::to_string(&truncated) {
        lines.push(line);
    }
}

fn trace_viewer_jsonl_lines(
    project_id: &str,
    session_id: &str,
    trace_id: &str,
    forge_lines: &[String],
    redaction_warning: &str,
) -> Vec<String> {
    let mut lines = Vec::new();
    let metadata_uuid = stable_uuid(session_id, "metadata", 0);
    push_trace_viewer_event(
        &mut lines,
        json!({
            "type": "assistant",
            "message": {
                "role": "assistant",
                "model": "forge-writer-agent",
                "content": [{
                    "type": "text",
                    "text": format!(
                        "Forge writer trajectory export for project {}. {}",
                        project_id,
                        redaction_warning
                    ),
                }],
            },
            "uuid": metadata_uuid,
            "parentUuid": Value::Null,
            "sessionId": session_id,
            "timestamp": iso_timestamp(0),
            "forgeEventType": "forge.export_metadata",
            "forgeTraceId": trace_id,
            "projectId": project_id,
            "localOnly": true,
            "redactionWarning": redaction_warning,
        }),
    );
    let mut parent_uuid = Some(metadata_uuid);

    for (index, line) in forge_lines.iter().enumerate() {
        let Ok(event) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let event_type = string_field(&event, &["eventType"]).unwrap_or("writer.unknown");
        let seq = event
            .get("seq")
            .and_then(Value::as_u64)
            .unwrap_or(index as u64 + 1);
        let ts_ms = event.get("tsMs").and_then(Value::as_u64).unwrap_or(0);
        let source_refs = event
            .get("sourceRefs")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let data = event.get("data").cloned().unwrap_or(Value::Null);
        let message_type = trace_viewer_message_type(event_type);
        let role = if message_type == "user" {
            "user"
        } else {
            "assistant"
        };
        let uuid = stable_uuid(session_id, event_type, seq);
        let parent = parent_uuid
            .as_ref()
            .map(|uuid| Value::String(uuid.clone()))
            .unwrap_or(Value::Null);
        let content_text = trace_viewer_summary(event_type, &data);
        let message = if role == "user" {
            json!({
                "role": "user",
                "content": content_text,
            })
        } else {
            json!({
                "role": "assistant",
                "model": "forge-writer-agent",
                "content": [{
                    "type": "text",
                    "text": content_text,
                }],
            })
        };

        push_trace_viewer_event(
            &mut lines,
            json!({
                "type": message_type,
                "message": message,
                "uuid": uuid,
                "parentUuid": parent,
                "sessionId": session_id,
                "timestamp": iso_timestamp(ts_ms),
                "forgeEventType": event_type,
                "forgeSeq": seq,
                "forgeTraceId": trace_id,
                "projectId": project_id,
                "sourceRefs": source_refs,
                "localOnly": true,
                "forgeEvent": event,
            }),
        );
        parent_uuid = Some(uuid);
    }

    lines
}

fn push_trace_viewer_event(lines: &mut Vec<String>, event: Value) {
    if let Ok(line) = serde_json::to_string(&event) {
        lines.push(line);
    }
}

fn trace_viewer_message_type(event_type: &str) -> &'static str {
    match event_type {
        "writer.observation" | "writer.feedback" | "writer.context_recall" => "user",
        "writer.run_event" => "assistant",
        _ => "assistant",
    }
}

fn trace_viewer_summary(event_type: &str, data: &Value) -> String {
    match event_type {
        "writer.observation" => format!(
            "Observation: {}{}{}",
            string_field(data, &["reason"]).unwrap_or("unknown reason"),
            optional_labeled(
                " chapter ",
                string_field(data, &["chapterTitle", "chapter_title"])
            ),
            optional_labeled(
                " - ",
                string_field(data, &["paragraphSnippet", "paragraph_snippet"])
            ),
        ),
        "writer.task_packet" => format!(
            "Task packet: {} | objective: {} | scope: {} | required context: {}",
            string_field(data, &["task"]).unwrap_or("unknown task"),
            string_field(data, &["objective"]).unwrap_or("unknown objective"),
            string_field(data, &["scope"]).unwrap_or("unknown scope"),
            number_field(data, &["requiredContextCount", "required_context_count"]).unwrap_or(0),
        ),
        "writer.proposal" => format!(
            "Proposal: {} priority={} confidence={}{}",
            string_field(data, &["kind"]).unwrap_or("unknown"),
            string_field(data, &["priority"]).unwrap_or("unknown"),
            number_or_float_field(data, &["confidence"]).unwrap_or_else(|| "unknown".to_string()),
            optional_labeled(
                " - ",
                string_field(data, &["previewSnippet", "preview_snippet"])
            ),
        ),
        "writer.feedback" => format!(
            "Feedback: proposal={} action={}{}",
            string_field(data, &["proposalId", "proposal_id"]).unwrap_or("unknown"),
            string_field(data, &["action"]).unwrap_or("unknown"),
            optional_labeled(" reason=", string_field(data, &["reason"])),
        ),
        "writer.operation_lifecycle" => format!(
            "Operation lifecycle: {} {}{}",
            string_field(data, &["operationKind", "operation_kind"]).unwrap_or("operation"),
            string_field(data, &["state"]).unwrap_or("unknown"),
            optional_labeled(
                " proposal=",
                string_field(data, &["proposalId", "proposal_id"])
            ),
        ),
        "writer.run_event" => summarize_run_event(data),
        "writer.post_write_diagnostics" => format!(
            "Post-write diagnostics: total={} errors={} warnings={}{}",
            number_field(data, &["totalCount", "total_count"]).unwrap_or(0),
            number_field(data, &["errorCount", "error_count"]).unwrap_or(0),
            number_field(data, &["warningCount", "warning_count"]).unwrap_or(0),
            optional_labeled(
                " chapter=",
                string_field(data, &["chapterTitle", "chapter_title"])
            ),
        ),
        "writer.context_recall" => format!(
            "Context recall: {}:{}{}",
            string_field(data, &["source"]).unwrap_or("source"),
            string_field(data, &["reference"]).unwrap_or("reference"),
            optional_labeled(" - ", string_field(data, &["snippet"])),
        ),
        "writer.product_metrics" => format!(
            "Product metrics: proposals={} feedback={} acceptance={} durable_save={}",
            number_field(data, &["proposalCount", "proposal_count"]).unwrap_or(0),
            number_field(data, &["feedbackCount", "feedback_count"]).unwrap_or(0),
            number_or_float_field(
                data,
                &["proposalAcceptanceRate", "proposal_acceptance_rate"]
            )
            .unwrap_or_else(|| "unknown".to_string()),
            number_or_float_field(
                data,
                &["durableSaveSuccessRate", "durable_save_success_rate"]
            )
            .unwrap_or_else(|| "unknown".to_string()),
        ),
        "writer.product_metrics_trend" => format!(
            "Product metrics trend: sessions={} recent_save_feedback={} previous_save_feedback={} delta={}",
            number_field(data, &["sessionCount", "session_count"]).unwrap_or(0),
            number_field(
                data,
                &[
                    "recentAverageSaveToFeedbackMs",
                    "recent_average_save_to_feedback_ms"
                ]
            )
            .map(|value| value.to_string())
            .unwrap_or_else(|| "n/a".to_string()),
            number_field(
                data,
                &[
                    "previousAverageSaveToFeedbackMs",
                    "previous_average_save_to_feedback_ms"
                ]
            )
            .map(|value| value.to_string())
            .unwrap_or_else(|| "n/a".to_string()),
            number_or_float_field(
                data,
                &["saveToFeedbackDeltaMs", "save_to_feedback_delta_ms"]
            )
            .unwrap_or_else(|| "n/a".to_string()),
        ),
        "writer.metacognition" => format!(
            "Metacognition: risk={} action={} confidence={}{}",
            string_field(data, &["riskLevel", "risk_level"]).unwrap_or("unknown"),
            string_field(data, &["recommendedAction", "recommended_action"]).unwrap_or("unknown"),
            number_or_float_field(data, &["confidence"]).unwrap_or_else(|| "unknown".to_string()),
            optional_labeled(" - ", string_field(data, &["summary"])),
        ),
        _ => format!("{}: {}", event_type, compact_json_snippet(data, 600)),
    }
}
