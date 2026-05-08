#[cfg(test)]
mod tests {
    use super::*;

    fn memory() -> WriterMemory {
        WriterMemory::open(std::path::Path::new(":memory:")).unwrap()
    }

    #[test]
    fn test_canon_entity_upsert() {
        let m = memory();
        let id = m
            .upsert_canon_entity(
                "location",
                "主角",
                &["林墨".into()],
                "主角",
                &serde_json::json!({"weapon": "剑"}),
                0.9,
            )
            .unwrap();
        assert!(id > 0);
        let facts = m.get_canon_facts_for_entity("主角").unwrap();
        assert_eq!(facts, vec![("weapon".to_string(), "剑".to_string())]);
        let entities = m.list_canon_entities().unwrap();
        assert_eq!(entities[0].name, "主角");
    }

    #[test]
    fn test_canon_rule_upsert() {
        let m = memory();
        let id = m
            .upsert_canon_rule("林墨绝不主动弃刀。", "character_rule", 7, "test")
            .unwrap();
        assert!(id > 0);
        m.upsert_canon_rule("林墨绝不主动弃刀。", "combat_rule", 9, "test2")
            .unwrap();
        let rules = m.list_canon_rules(10).unwrap();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].category, "combat_rule");
        assert_eq!(rules[0].priority, 9);
    }

    #[test]
    fn test_promise_lifecycle() {
        let m = memory();
        let id = m
            .add_promise("clue", "密道", "第2章破庙有密道", "ch2", "ch8", 5)
            .unwrap();
        assert!(id > 0);
        let open = m.get_open_promises().unwrap();
        assert_eq!(open.len(), 1);
        let summaries = m.get_open_promise_summaries().unwrap();
        assert_eq!(summaries[0].title, "密道");
        assert!(m.resolve_promise(id, "ch8").unwrap());
        let open2 = m.get_open_promises().unwrap();
        assert_eq!(open2.len(), 0);
    }

    #[test]
    fn test_promise_defer_and_abandon() {
        let m = memory();
        let deferred_id = m
            .add_promise("clue", "密道", "第2章破庙有密道", "ch2", "ch8", 5)
            .unwrap();
        assert!(m.defer_promise(deferred_id, "ch10").unwrap());
        let summaries = m.get_open_promise_summaries().unwrap();
        assert_eq!(summaries[0].expected_payoff, "ch10");

        let abandoned_id = m
            .add_promise("clue", "铜铃", "铜铃声需要解释", "ch2", "ch6", 5)
            .unwrap();
        assert!(m.abandon_promise(abandoned_id).unwrap());
        let open_titles = m
            .get_open_promise_summaries()
            .unwrap()
            .into_iter()
            .map(|promise| promise.title)
            .collect::<Vec<_>>();
        assert!(!open_titles.contains(&"铜铃".to_string()));
    }

    #[test]
    fn test_style_preference_update() {
        let m = memory();
        m.upsert_style_preference("dialog_style", "prefers_subtext", true)
            .unwrap();
        m.upsert_style_preference("exposition", "rejects_info_dump", false)
            .unwrap();
        let prefs = m.list_style_preferences(5).unwrap();
        assert_eq!(prefs.len(), 2);
        assert!(prefs.iter().any(|p| p.key == "dialog_style"));
    }

    #[test]
    fn test_feedback_record() {
        let m = memory();
        m.record_feedback("prop_1", "accepted", "", "").unwrap();
        m.record_feedback("prop_2", "rejected", "not my style", "")
            .unwrap();
        let (accepted, rejected) = m.feedback_stats("prop_1").unwrap();
        assert_eq!(accepted, 1);
        assert_eq!(rejected, 0);
    }

    #[test]
    fn test_memory_audit_record() {
        let m = memory();
        m.record_memory_audit(&MemoryAuditSummary {
            proposal_id: "prop_1".to_string(),
            kind: "CanonUpdate".to_string(),
            action: "Accepted".to_string(),
            title: "沈照 [character]".to_string(),
            evidence: "那个少年名叫沈照".to_string(),
            rationale: "durable character".to_string(),
            reason: Some("approved".to_string()),
            created_at: 42,
        })
        .unwrap();
        let audit = m.list_memory_audit(5).unwrap();
        assert_eq!(audit.len(), 1);
        assert_eq!(audit[0].proposal_id, "prop_1");
        assert_eq!(audit[0].reason.as_deref(), Some("approved"));
    }

    #[test]
    fn test_memory_feedback_record_and_filter_by_slot() {
        let m = memory();
        m.record_memory_feedback(&MemoryFeedbackSummary {
            slot: "memory|canon|character|沈照".to_string(),
            category: "canon".to_string(),
            action: "reinforcement".to_string(),
            confidence_delta: 0.08,
            source_error: None,
            proposal_id: "prop_1".to_string(),
            reason: Some("accepted after save".to_string()),
            created_at: 42,
        })
        .unwrap();
        m.record_memory_feedback(&MemoryFeedbackSummary {
            slot: "memory|promise|mystery_clue|玉佩".to_string(),
            category: "promise".to_string(),
            action: "correction".to_string(),
            confidence_delta: -0.2,
            source_error: Some("作者指出不是伏笔".to_string()),
            proposal_id: "prop_2".to_string(),
            reason: Some("not durable".to_string()),
            created_at: 43,
        })
        .unwrap();

        let feedback = m.list_memory_feedback(10).unwrap();
        assert_eq!(feedback.len(), 2);
        assert_eq!(feedback[0].slot, "memory|promise|mystery_clue|玉佩");
        assert_eq!(feedback[0].action, "correction");
        assert_eq!(
            feedback[0].source_error.as_deref(),
            Some("作者指出不是伏笔")
        );

        let slot_feedback = m
            .list_memory_feedback_for_slot("memory|canon|character|沈照", 10)
            .unwrap();
        assert_eq!(slot_feedback.len(), 1);
        assert_eq!(slot_feedback[0].category, "canon");
        assert_eq!(slot_feedback[0].confidence_delta, 0.08);
    }

    #[test]
    fn test_manual_agent_turns_persist_and_filter_by_project() {
        let m = memory();
        m.record_manual_agent_turn(&ManualAgentTurnSummary {
            project_id: "novel-a".to_string(),
            observation_id: "obs-a-1".to_string(),
            chapter_title: Some("第一章".to_string()),
            user: "上一轮怎么处理玉佩？".to_string(),
            assistant: "让张三暂时隐瞒玉佩。".to_string(),
            source_refs: vec!["PromiseLedger".to_string()],
            created_at: 10,
        })
        .unwrap();
        m.record_manual_agent_turn(&ManualAgentTurnSummary {
            project_id: "novel-b".to_string(),
            observation_id: "obs-b-1".to_string(),
            chapter_title: None,
            user: "另一个项目".to_string(),
            assistant: "不应混入".to_string(),
            source_refs: Vec::new(),
            created_at: 11,
        })
        .unwrap();
        m.record_manual_agent_turn(&ManualAgentTurnSummary {
            project_id: "novel-a".to_string(),
            observation_id: "obs-a-2".to_string(),
            chapter_title: Some("第二章".to_string()),
            user: "继续上一轮".to_string(),
            assistant: "把玉佩变成下一章冲突。".to_string(),
            source_refs: vec!["CreativeDecision".to_string()],
            created_at: 12,
        })
        .unwrap();

        let turns = m.list_manual_agent_turns("novel-a", 10).unwrap();

        assert_eq!(turns.len(), 2);
        assert_eq!(turns[0].observation_id, "obs-a-1");
        assert_eq!(turns[0].chapter_title.as_deref(), Some("第一章"));
        assert_eq!(turns[0].source_refs, vec!["PromiseLedger".to_string()]);
        assert_eq!(turns[1].user, "继续上一轮");
    }

    #[test]
    fn test_writer_trace_record() {
        let m = memory();
        m.record_observation_trace("obs_1", 10, "Idle", Some("Chapter-1"), "林墨停下脚步")
            .unwrap();
        m.record_proposal_trace(
            &ProposalTraceSummary {
                id: "prop_1".to_string(),
                observation_id: "obs_1".to_string(),
                kind: "Ghost".to_string(),
                priority: "Ambient".to_string(),
                state: "pending".to_string(),
                confidence: 0.7,
                preview_snippet: "他没有立刻回答".to_string(),
                evidence: vec![crate::writer_agent::proposal::EvidenceRef {
                    source: crate::writer_agent::proposal::EvidenceSource::ChapterMission,
                    reference: "Chapter-1:mission".to_string(),
                    snippet: "本章任务".to_string(),
                }],
                context_budget: Some(ContextBudgetTrace {
                    task: "GhostWriting".to_string(),
                    used: 40,
                    total_budget: 100,
                    wasted: 60,
                    source_reports: vec![ContextSourceBudgetTrace {
                        source: "CursorPrefix".to_string(),
                        requested: 80,
                        provided: 40,
                        truncated: true,
                        reason:
                            "GhostWriting required source reserved 240 chars before priority fill."
                                .to_string(),
                        truncation_reason: Some(
                            "Source content was limited by its per-source budget of 80 chars."
                                .to_string(),
                        ),
                    }],
                }),
                expires_at: Some(1000),
            },
            11,
        )
        .unwrap();
        m.record_feedback_trace(&FeedbackTraceSummary {
            proposal_id: "prop_1".to_string(),
            action: "Accepted".to_string(),
            reason: Some("fits".to_string()),
            created_at: 12,
        })
        .unwrap();
        m.update_proposal_trace_state("prop_1", "feedback:Accepted")
            .unwrap();

        assert_eq!(m.list_observation_traces(5).unwrap()[0].id, "obs_1");
        let proposal = m.list_proposal_traces(5).unwrap().remove(0);
        assert_eq!(proposal.state, "feedback:Accepted");
        assert_eq!(proposal.evidence.len(), 1);
        assert_eq!(
            proposal.evidence[0].source,
            crate::writer_agent::proposal::EvidenceSource::ChapterMission
        );
        let budget = proposal.context_budget.unwrap();
        assert_eq!(budget.task, "GhostWriting");
        assert_eq!(budget.used, 40);
        assert!(budget.source_reports[0].truncated);
        assert_eq!(
            m.list_feedback_traces(5).unwrap()[0].reason.as_deref(),
            Some("fits")
        );
    }

    #[test]
    fn test_run_events_persist_and_replay_in_order() {
        let m = memory();
        m.record_run_event(&RunEventSummary {
            seq: 1,
            project_id: "novel-a".to_string(),
            session_id: "session-a".to_string(),
            task_id: Some("obs-1".to_string()),
            event_type: "writer.observation".to_string(),
            source_refs: vec!["chapter:Chapter-1".to_string()],
            data: serde_json::json!({"reason": "Idle"}),
            ts_ms: 10,
        })
        .unwrap();
        m.record_run_event(&RunEventSummary {
            seq: 2,
            project_id: "novel-a".to_string(),
            session_id: "session-a".to_string(),
            task_id: Some("prop-1".to_string()),
            event_type: "writer.proposal_created".to_string(),
            source_refs: vec!["obs-1".to_string()],
            data: serde_json::json!({"kind": "Ghost"}),
            ts_ms: 11,
        })
        .unwrap();
        m.record_run_event(&RunEventSummary {
            seq: 1,
            project_id: "novel-b".to_string(),
            session_id: "session-b".to_string(),
            task_id: None,
            event_type: "writer.observation".to_string(),
            source_refs: Vec::new(),
            data: serde_json::json!({"reason": "Other"}),
            ts_ms: 12,
        })
        .unwrap();

        let events = m.list_run_events("novel-a", "session-a", 10).unwrap();

        assert_eq!(events.len(), 2);
        assert_eq!(
            events.iter().map(|event| event.seq).collect::<Vec<_>>(),
            vec![1, 2]
        );
        assert_eq!(events[0].event_type, "writer.observation");
        assert_eq!(events[1].task_id.as_deref(), Some("prop-1"));
        assert_eq!(
            events[1].data.get("kind").and_then(|value| value.as_str()),
            Some("Ghost")
        );
    }

    #[test]
    fn test_context_recall_records_only_surfaced_evidence() {
        let m = memory();
        let evidence = vec![
            crate::writer_agent::proposal::EvidenceRef {
                source: crate::writer_agent::proposal::EvidenceSource::ChapterMission,
                reference: "Chapter-2:mission".to_string(),
                snippet: "本章必须推进玉佩线索。".to_string(),
            },
            crate::writer_agent::proposal::EvidenceRef {
                source: crate::writer_agent::proposal::EvidenceSource::Canon,
                reference: "林墨.weapon".to_string(),
                snippet: "林墨惯用寒影刀。".to_string(),
            },
        ];
        m.record_context_recalls("novel-a", "prop_1", "obs_1", &evidence, 10)
            .unwrap();
        m.record_context_recalls("novel-a", "prop_2", "obs_2", &evidence[..1], 20)
            .unwrap();

        let recalls = m.list_context_recalls("novel-a", 10).unwrap();

        assert_eq!(recalls.len(), 2);
        assert_eq!(recalls[0].source, "ChapterMission");
        assert_eq!(recalls[0].reference, "Chapter-2:mission");
        assert_eq!(recalls[0].recall_count, 2);
        assert_eq!(recalls[0].first_recalled_at, 10);
        assert_eq!(recalls[0].last_recalled_at, 20);
        assert_eq!(recalls[0].last_proposal_id, "prop_2");
        assert_eq!(recalls[1].reference, "林墨.weapon");
        assert_eq!(recalls[1].recall_count, 1);
    }

    #[test]
    fn test_decision_summary() {
        let m = memory();
        m.record_decision(
            "Chapter-1",
            "续写建议",
            "accepted",
            &[],
            "符合角色声音",
            &[],
        )
        .unwrap();
        let decisions = m.list_recent_decisions(5).unwrap();
        assert_eq!(decisions.len(), 1);
        assert_eq!(decisions[0].decision, "accepted");
    }

    #[test]
    fn test_story_contract_upsert_and_seed() {
        let m = memory();
        assert!(m
            .ensure_story_contract_seed(
                "novel-a",
                "寒影录",
                "玄幻",
                "刀客追查玉佩真相。",
                "复仇与守护的冲突。",
                "不得提前泄露玉佩来源。",
            )
            .unwrap());
        assert!(!m
            .ensure_story_contract_seed("novel-a", "不覆盖", "悬疑", "", "", "",)
            .unwrap());

        let mut contract = m.get_story_contract("novel-a").unwrap().unwrap();
        assert_eq!(contract.title, "寒影录");
        assert!(contract.render_for_context().contains("读者承诺"));

        contract.reader_promise = "新的读者承诺".to_string();
        m.upsert_story_contract(&contract).unwrap();
        let updated = m.get_story_contract("novel-a").unwrap().unwrap();
        assert_eq!(updated.reader_promise, "新的读者承诺");
        assert!(!updated.is_empty());
    }

    #[test]
    fn test_chapter_mission_upsert_list_and_seed() {
        let m = memory();
        assert!(m
            .ensure_chapter_mission_seed(
                "novel-a",
                "第一章",
                "林墨发现玉佩线索。",
                "推进玉佩线索",
                "不要提前揭开真相",
                "以新的疑问收束。",
                "test",
            )
            .unwrap());
        assert!(!m
            .ensure_chapter_mission_seed("novel-a", "第一章", "不覆盖", "", "", "", "test")
            .unwrap());

        let mut mission = m.get_chapter_mission("novel-a", "第一章").unwrap().unwrap();
        assert_eq!(mission.mission, "林墨发现玉佩线索。");
        assert!(mission.render_for_context().contains("本章任务"));

        mission.expected_ending = "以冲突升级收束。".to_string();
        m.upsert_chapter_mission(&mission).unwrap();

        let missions = m.list_chapter_missions("novel-a", 10).unwrap();
        assert_eq!(missions.len(), 1);
        assert_eq!(missions[0].expected_ending, "以冲突升级收束。");
        assert!(!missions[0].is_empty());
    }

    #[test]
    fn test_chapter_result_record_and_render() {
        let m = memory();
        let id = m
            .record_chapter_result(&ChapterResultSummary {
                id: 0,
                project_id: "novel-a".to_string(),
                chapter_title: "第一章".to_string(),
                chapter_revision: "rev-1".to_string(),
                summary: "林墨发现玉佩线索，张三隐瞒下落。".to_string(),
                state_changes: vec!["林墨得知玉佩存在风险".to_string()],
                character_progress: vec!["张三选择隐瞒".to_string()],
                new_conflicts: vec!["林墨与张三信任受损".to_string()],
                new_clues: vec!["玉佩".to_string()],
                promise_updates: vec!["玉佩仍需后续解释".to_string()],
                canon_updates: vec!["林墨惯用寒影刀".to_string()],
                source_ref: "chapter_save:第一章:rev-1".to_string(),
                created_at: 42,
            })
            .unwrap();
        assert!(id > 0);

        let recent = m.list_recent_chapter_results("novel-a", 5).unwrap();
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].chapter_title, "第一章");
        assert_eq!(recent[0].new_clues, vec!["玉佩".to_string()]);
        assert!(recent[0].render_for_context().contains("章节结果"));

        let latest = m
            .latest_chapter_result("novel-a", "第一章")
            .unwrap()
            .unwrap();
        assert_eq!(latest.summary, "林墨发现玉佩线索，张三隐瞒下落。");
        assert!(!latest.is_empty());
    }

    #[test]
    fn open_migrates_legacy_writer_memory_columns() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE canon_entities (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                kind TEXT NOT NULL,
                name TEXT NOT NULL UNIQUE
            );
            INSERT INTO canon_entities (kind, name) VALUES ('character', '林墨');
            CREATE TABLE plot_promises (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                kind TEXT NOT NULL,
                title TEXT NOT NULL
            );
            INSERT INTO plot_promises (kind, title) VALUES ('clue', '玉佩');
            CREATE TABLE writer_proposal_trace (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                proposal_id TEXT NOT NULL UNIQUE,
                observation_id TEXT NOT NULL,
                kind TEXT NOT NULL,
                priority TEXT NOT NULL,
                state TEXT NOT NULL,
                confidence REAL DEFAULT 0.0,
                preview_snippet TEXT DEFAULT '',
                created_at INTEGER NOT NULL
            );",
        )
        .unwrap();

        initialize_schema(&conn).unwrap();

        assert!(table_has_column(&conn, "canon_entities", "attributes_json").unwrap());
        assert!(table_has_column(&conn, "plot_promises", "expected_payoff").unwrap());
        assert!(table_has_column(&conn, "plot_promises", "last_seen_chapter").unwrap());
        assert!(table_has_column(&conn, "plot_promises", "blocked_reason").unwrap());
        assert!(table_has_column(&conn, "plot_promises", "promoted").unwrap());
        assert!(table_has_column(&conn, "plot_promises", "core").unwrap());
        assert!(table_has_column(&conn, "writer_proposal_trace", "evidence_json").unwrap());
        assert!(table_has_column(&conn, "writer_proposal_trace", "expires_at").unwrap());
        assert!(table_exists(&conn, "manual_agent_turns").unwrap());
        assert!(table_has_column(&conn, "manual_agent_turns", "source_refs_json").unwrap());
        assert!(table_exists(&conn, "story_contracts").unwrap());
        assert!(table_has_column(&conn, "story_contracts", "reader_promise").unwrap());
        assert!(table_exists(&conn, "chapter_missions").unwrap());
        assert!(table_has_column(&conn, "chapter_missions", "expected_ending").unwrap());
        assert!(table_exists(&conn, "chapter_result_snapshots").unwrap());
        assert!(table_has_column(&conn, "chapter_result_snapshots", "new_clues_json").unwrap());
        assert!(table_exists(&conn, "memory_feedback_events").unwrap());
        assert!(table_has_column(&conn, "memory_feedback_events", "source_error").unwrap());
        assert!(table_has_column(&conn, "memory_feedback_events", "confidence_delta").unwrap());
        let version: i64 = conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);

        let m = WriterMemory { conn };
        let entities = m.list_canon_entities().unwrap();
        assert_eq!(entities[0].name, "林墨");
        let promises = m.get_open_promise_summaries().unwrap();
        assert_eq!(promises[0].title, "玉佩");
        assert_eq!(promises[0].last_seen_chapter, "");
    }
}
