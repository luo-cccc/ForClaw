/// Microcompact: clear old tool results to save tokens without an LLM call.
/// Ported from OpenHarness microcompact_messages() in services/compact/__init__.py.
///
/// Keeps the most recent `keep_recent` tool results; replaces older ones with
/// a compact placeholder. Returns the modified messages and tokens saved.
pub fn microcompact_tool_results(messages: &mut [LlmMessage], keep_recent: usize) -> u64 {
    let keep_recent = keep_recent.max(1);
    let mut tool_result_indices: Vec<usize> = Vec::new();
    for (i, msg) in messages.iter().enumerate() {
        if msg.role == "tool" && msg.tool_call_id.is_some() {
            tool_result_indices.push(i);
        }
    }

    if tool_result_indices.len() <= keep_recent {
        return 0;
    }

    let clear_count = tool_result_indices.len().saturating_sub(keep_recent);
    let mut tokens_saved: u64 = 0;

    for &idx in &tool_result_indices[..clear_count] {
        let msg = &mut messages[idx];
        let old_len = msg.content.as_ref().map(|c| c.len()).unwrap_or(0) as u64;
        if old_len > 20 {
            tokens_saved += old_len / 3; // rough token estimate
        }
        msg.content = Some("[Tool result cleared by microcompact]".to_string());
    }

    tokens_saved
}

#[cfg(test)]
mod microcompact_tests {
    use super::*;

    fn tool_msg(id: &str, content: &str) -> LlmMessage {
        LlmMessage {
            role: "tool".into(),
            content: Some(content.into()),
            tool_calls: None,
            tool_call_id: Some(id.into()),
            name: Some("test_tool".into()),
        }
    }

    #[test]
    fn microcompact_preserves_recent_tool_pairs() {
        let mut messages = vec![
            LlmMessage {
                role: "user".into(),
                content: Some("q".into()),
                tool_calls: None,
                tool_call_id: None,
                name: None,
            },
            tool_msg("t1", &"old result ".repeat(50)),
            LlmMessage {
                role: "assistant".into(),
                content: Some("a1".into()),
                tool_calls: None,
                tool_call_id: None,
                name: None,
            },
            tool_msg("t2", &"old result ".repeat(50)),
            LlmMessage {
                role: "assistant".into(),
                content: Some("a2".into()),
                tool_calls: None,
                tool_call_id: None,
                name: None,
            },
            tool_msg("t3", &"recent result ".repeat(50)),
        ];

        let saved = microcompact_tool_results(&mut messages, 1);
        assert!(saved > 0, "should have saved tokens");

        // t3 (most recent) should be preserved
        let t3 = &messages[5];
        assert!(
            t3.content.as_ref().unwrap().contains("recent"),
            "most recent tool result should be preserved"
        );

        // t1 (oldest) should be cleared
        let t1 = &messages[1];
        assert!(
            t1.content.as_ref().unwrap().contains("microcompact"),
            "old tool result should be cleared"
        );
    }

    #[test]
    fn microcompact_noop_when_few_results() {
        let mut messages = vec![tool_msg("t1", "short")];
        let saved = microcompact_tool_results(&mut messages, 3);
        assert_eq!(saved, 0);
        assert!(messages[0].content.as_ref().unwrap().contains("short"));
    }

    #[test]
    fn compaction_result_records_checkpoints() {
        let report = CompactionResult {
            summary: "test summary".to_string(),
            compacted_count: 10,
            preserved_count: 3,
            tokens_before: 5000,
            tokens_after: 3000,
            kind: CompactionKind::Full,
            checkpoints: vec![CompactionCheckpoint {
                name: "compacted".to_string(),
                message_count: 10,
                token_count: 2000,
                metadata: Some("cut at message 7".to_string()),
            }],
            tokens_saved_by_tool_truncation: 500,
            boundary_summary: "compacted 10 messages, preserved 3 messages".to_string(),
            recovery_level: None,
            trigger: CompactionTrigger::default(),
            spine_report: None,
        };
        assert!(
            !report.checkpoints.is_empty(),
            "report should have checkpoints"
        );
        assert!(
            report.tokens_saved_by_tool_truncation > 0,
            "should record token savings"
        );
        assert!(
            !report.boundary_summary.is_empty(),
            "should have boundary summary"
        );
        assert!(matches!(report.kind, CompactionKind::Full));
        assert_eq!(report.checkpoints[0].name, "compacted");
        assert_eq!(report.checkpoints[0].token_count, 2000);
    }
}
