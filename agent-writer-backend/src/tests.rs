use super::*;

fn test_context_pack() -> writer_agent::context::WritingContextPack {
    writer_agent::context::WritingContextPack {
        task: writer_agent::context::AgentTask::InlineRewrite,
        total_chars: 18,
        budget_limit: 32,
        budget_report: writer_agent::context::ContextBudgetReport {
            total_budget: 32,
            used: 18,
            wasted: 14,
            source_reports: vec![
                writer_agent::context::SourceReport {
                    source: "SelectedText".to_string(),
                    requested: 20,
                    provided: 12,
                    truncated: false,
                    reason:
                        "InlineRewrite required source reserved 400 chars before priority fill."
                            .to_string(),
                    truncation_reason: None,
                },
                writer_agent::context::SourceReport {
                    source: "CursorPrefix".to_string(),
                    requested: 20,
                    provided: 6,
                    truncated: true,
                    reason:
                        "InlineRewrite required source reserved 160 chars before priority fill."
                            .to_string(),
                    truncation_reason: Some(
                        "Source content was limited by its per-source budget of 20 chars."
                            .to_string(),
                    ),
                },
            ],
        },
        sources: vec![
            writer_agent::context::ContextExcerpt {
                source: writer_agent::context::ContextSource::SelectedText,
                content: "林墨握紧寒影刀。".to_string(),
                char_count: 8,
                truncated: false,
                priority: 10,
                evidence_ref: Some("selection".to_string()),
            },
            writer_agent::context::ContextExcerpt {
                source: writer_agent::context::ContextSource::CursorPrefix,
                content: "张三退后".to_string(),
                char_count: 4,
                truncated: true,
                priority: 9,
                evidence_ref: None,
            },
        ],
    }
}

#[test]
fn render_writer_context_pack_includes_budget_report() {
    let rendered = render_writer_context_pack(&test_context_pack());

    assert!(rendered.contains("# ContextPack Budget"));
    assert!(rendered.contains("task: InlineRewrite"));
    assert!(rendered.contains("used/budget: 18/32"));
    assert!(rendered.contains("wasted: 14"));
    assert!(rendered.contains("- CursorPrefix: requested 20, provided 6, truncated true"));
    assert!(rendered.contains("reason: InlineRewrite required source"));
    assert!(rendered.contains("truncation: Source content was limited"));
    assert!(rendered.contains("# ContextPack Sources"));
    assert!(rendered.contains("## SelectedText"));
    assert!(rendered.contains("林墨握紧寒影刀。"));
}

#[test]
fn inline_operation_messages_include_context_budget_report() {
    let observation = build_manual_writer_observation(
        "改写得更紧张",
        "林墨握紧寒影刀。张三退后。",
        "林墨握紧寒影刀。",
        "寒影刀",
        None,
        "novel-a",
    );
    let messages = commands::manual_agent::writer_agent_inline_operation_messages(
        "改写得更紧张",
        &observation,
        &test_context_pack(),
    );
    let user_content = messages[1]["content"].as_str().unwrap();

    assert!(user_content.contains("ContextPack:"));
    assert!(user_content.contains("# ContextPack Budget"));
    assert!(user_content.contains("used/budget: 18/32"));
    assert!(user_content.contains("truncated true"));
}

#[test]
fn manual_observation_uses_char_cursor_and_selection_range() {
    let context = "林墨拔出寒影刀。\n张三后退一步。";
    let observation =
        build_manual_writer_observation("这段有冲突吗", context, "", "寒影刀", None, "test");

    assert_eq!(
        observation.source,
        writer_agent::observation::ObservationSource::ManualRequest
    );
    assert_eq!(
        observation.reason,
        writer_agent::observation::ObservationReason::Explicit
    );
    assert_eq!(
        observation.cursor.unwrap().to,
        context.chars().count(),
        "cursor must use character index, not UTF-8 bytes"
    );
    let selection = observation.selection.unwrap();
    assert_eq!(selection.from, 4);
    assert_eq!(selection.to, 7);
    assert_eq!(selection.text, "寒影刀");
    assert_eq!(observation.paragraph, "寒影刀");
}

#[test]
fn manual_agent_history_keeps_only_matching_project_recent_first() {
    let mut history = manual_agent::ManualAgentHistory::default();
    history.append(ManualAgentTurn {
        project_id: "novel-a".to_string(),
        created_at: 1,
        observation_id: "obs-a-1".to_string(),
        chapter_title: Some("第一章".to_string()),
        user: "第一问".to_string(),
        assistant: "第一答".to_string(),
        source_refs: vec!["Lorebook".to_string()],
    });
    history.append(ManualAgentTurn {
        project_id: "novel-b".to_string(),
        created_at: 2,
        observation_id: "obs-b-1".to_string(),
        chapter_title: Some("第一章".to_string()),
        user: "另一项目".to_string(),
        assistant: "不应出现".to_string(),
        source_refs: Vec::new(),
    });
    history.append(ManualAgentTurn {
        project_id: "novel-a".to_string(),
        created_at: 3,
        observation_id: "obs-a-2".to_string(),
        chapter_title: Some("第二章".to_string()),
        user: "第二问".to_string(),
        assistant: "第二答".to_string(),
        source_refs: vec!["Outline".to_string()],
    });

    let recent = history.recent_for_project("novel-a", 8, 12_000);

    assert_eq!(recent.len(), 2);
    assert_eq!(recent[0].user, "第一问");
    assert_eq!(recent[1].user, "第二问");
    assert_eq!(recent[1].source_refs, vec!["Outline".to_string()]);
}

#[test]
fn manual_agent_history_applies_turn_and_char_budgets() {
    let mut history = manual_agent::ManualAgentHistory::default();
    for idx in 0..5 {
        history.append(ManualAgentTurn {
            project_id: "novel-a".to_string(),
            created_at: idx,
            observation_id: format!("obs-a-{}", idx),
            chapter_title: None,
            user: format!("问题{}", idx),
            assistant: "答复".repeat(80),
            source_refs: Vec::new(),
        });
    }

    let recent = history.recent_for_project("novel-a", 2, 12_000);
    assert_eq!(recent.len(), 2);
    assert_eq!(recent[0].user, "问题3");
    assert_eq!(recent[1].user, "问题4");

    let budgeted = history.recent_for_project("novel-a", 8, 180);
    assert_eq!(budgeted.len(), 1);
    assert_eq!(budgeted[0].user, "问题4");
}

#[test]
fn manual_agent_history_messages_restore_dialog_roles() {
    let messages = manual_agent::manual_agent_history_messages(&[ManualAgentTurn {
        project_id: "novel-a".to_string(),
        created_at: 42,
        observation_id: "obs-a-42".to_string(),
        chapter_title: Some("第三章".to_string()),
        user: "上一轮怎么处理张三？".to_string(),
        assistant: "先让张三隐瞒玉佩。".to_string(),
        source_refs: vec!["PromiseLedger".to_string()],
    }]);

    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0].role, "user");
    assert!(messages[0]
        .content
        .as_ref()
        .unwrap()
        .contains("上一轮怎么处理张三"));
    assert_eq!(messages[1].role, "assistant");
    assert!(messages[1]
        .content
        .as_ref()
        .unwrap()
        .contains("PromiseLedger"));
}

#[test]
fn merge_manual_agent_history_dedupes_persisted_and_runtime_turns() {
    let persisted = vec![ManualAgentTurn {
        project_id: "novel-a".to_string(),
        created_at: 1,
        observation_id: "obs-a-1".to_string(),
        chapter_title: None,
        user: "已持久化问题".to_string(),
        assistant: "已持久化答复".to_string(),
        source_refs: Vec::new(),
    }];
    let runtime = vec![
        ManualAgentTurn {
            project_id: "novel-a".to_string(),
            created_at: 1,
            observation_id: "obs-a-1".to_string(),
            chapter_title: None,
            user: "重复问题".to_string(),
            assistant: "重复答复".to_string(),
            source_refs: Vec::new(),
        },
        ManualAgentTurn {
            project_id: "novel-a".to_string(),
            created_at: 2,
            observation_id: "obs-a-2".to_string(),
            chapter_title: None,
            user: "新问题".to_string(),
            assistant: "新答复".to_string(),
            source_refs: Vec::new(),
        },
    ];

    let merged = manual_agent::merge_manual_agent_history("novel-a", persisted, runtime, 8, 12_000);

    assert_eq!(merged.len(), 2);
    assert_eq!(merged[0].user, "已持久化问题");
    assert_eq!(merged[1].user, "新问题");
}

#[test]
fn split_context_for_cursor_keeps_suffix_after_cursor() {
    let (prefix, suffix) = split_context_for_cursor("甲乙丙丁戊", 3, 2, 2);
    assert_eq!(prefix, "乙丙");
    assert_eq!(suffix, "丁戊");
}

#[test]
fn html_to_plain_text_keeps_editor_text_without_tags() {
    let text = html_to_plain_text(
        "<p>林墨&nbsp;拔出寒影刀。</p><p>张三说：&quot;别动&quot;&amp;后退。</p>",
    );

    assert_eq!(text, "林墨 拔出寒影刀。\n张三说：\"别动\"&后退。");
}

#[test]
fn last_meaningful_paragraph_ignores_short_trailing_lines() {
    let paragraph =
        writer_observer::last_meaningful_paragraph("第一段很长。\n短\n最后一段足够长。").unwrap();
    assert_eq!(paragraph, "最后一段足够长。");
}

#[test]
fn editor_state_observation_uses_text_cursor_for_kernel() {
    let payload = test_editor_state_payload("林墨拔出", "寒影刀", "林墨拔出", 99, Some(4));
    let observation = build_writer_observation_from_editor_state(&payload, "test");

    assert_eq!(
        observation.cursor.unwrap().to,
        4,
        "WriterAgent coordinates must be plain-text character indexes"
    );
    assert_eq!(observation.prefix, "林墨拔出");
    assert_eq!(observation.suffix, "寒影刀");
    assert_eq!(
        observation.reason,
        writer_agent::observation::ObservationReason::Idle
    );
}

#[test]
fn editor_state_observation_falls_back_to_prefix_char_count() {
    let payload = test_editor_state_payload("林墨拔出", "", "", 99, None);
    let observation = build_writer_observation_from_editor_state(&payload, "test");

    assert_eq!(observation.cursor.unwrap().to, "林墨拔出".chars().count());
    assert!(observation.paragraph.contains("Current paragraph"));
}

#[test]
fn migrate_legacy_db_copies_when_target_is_missing() {
    let root = std::env::temp_dir().join(format!(
        "forge-db-migrate-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let legacy_path = root.join("legacy.db");
    let target_path = root.join("app-data").join("writer_memory.db");

    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(&legacy_path, b"legacy-memory").unwrap();

    app_state::migrate_legacy_db_if_needed(&target_path, Some(legacy_path.clone())).unwrap();

    assert_eq!(std::fs::read(&target_path).unwrap(), b"legacy-memory");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn migrate_legacy_db_does_not_overwrite_existing_target() {
    let root = std::env::temp_dir().join(format!(
        "forge-db-migrate-existing-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let legacy_path = root.join("legacy.db");
    let target_dir = root.join("app-data");
    let target_path = target_dir.join("writer_memory.db");

    std::fs::create_dir_all(&target_dir).unwrap();
    std::fs::write(&legacy_path, b"legacy-memory").unwrap();
    std::fs::write(&target_path, b"current-memory").unwrap();

    app_state::migrate_legacy_db_if_needed(&target_path, Some(legacy_path)).unwrap();

    assert_eq!(std::fs::read(&target_path).unwrap(), b"current-memory");
    let _ = std::fs::remove_dir_all(&root);
}
