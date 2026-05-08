#[test]
fn save_observation_calibrates_completed_chapter_mission() {
    let memory = WriterMemory::open(std::path::Path::new(":memory:")).unwrap();
    memory
        .ensure_chapter_mission_seed(
            "default",
            "Chapter-1",
            "林墨追查玉佩下落。",
            "玉佩",
            "提前揭开真相",
            "下落",
            "test",
        )
        .unwrap();
    let mut kernel = WriterAgentKernel::new("default", memory);
    let mut obs = observation("林墨发现玉佩的下落，但张三仍没有说出真相。");
    obs.reason = ObservationReason::Save;
    obs.source = ObservationSource::ChapterSave;

    let proposals = kernel.observe(obs).unwrap();

    let calibration = proposals
        .iter()
        .find(|p| p.rationale.contains("mission calibration"))
        .expect("should produce a mission calibration proposal");
    assert_eq!(calibration.kind, ProposalKind::ChapterMission);
    assert!(calibration.preview.contains("completed"));
    // Mission in DB should NOT be auto-calibrated — proposals require author approval
    let mission = kernel
        .ledger_snapshot()
        .active_chapter_mission
        .expect("mission should exist");
    assert_eq!(mission.status, "draft");
}

#[test]
fn save_observation_marks_chapter_mission_drifted_on_must_not_hit() {
    let memory = WriterMemory::open(std::path::Path::new(":memory:")).unwrap();
    memory
        .ensure_chapter_mission_seed(
            "default",
            "Chapter-1",
            "林墨追查玉佩下落。",
            "玉佩",
            "真相",
            "下落",
            "test",
        )
        .unwrap();
    let mut kernel = WriterAgentKernel::new("default", memory);
    let mut obs = observation("林墨发现玉佩的下落，并当场揭开真相。");
    obs.reason = ObservationReason::Save;
    obs.source = ObservationSource::ChapterSave;

    let proposals = kernel.observe(obs).unwrap();

    let calibration = proposals
        .iter()
        .find(|p| p.rationale.contains("mission calibration"))
        .expect("should produce a mission calibration proposal");
    assert_eq!(calibration.kind, ProposalKind::ChapterMission);
    assert_eq!(calibration.priority, ProposalPriority::Urgent);
    assert!(calibration.preview.contains("drifted"));
    assert!(calibration.risks.iter().any(|r| r.contains("drifted")));
    // Mission in DB should NOT be auto-calibrated
    let mission = kernel
        .ledger_snapshot()
        .active_chapter_mission
        .expect("mission should exist");
    assert_eq!(mission.status, "draft");
}

#[test]
fn ledger_snapshot_derives_next_beat_from_latest_result_and_promises() {
    let memory = WriterMemory::open(std::path::Path::new(":memory:")).unwrap();
    memory
        .add_promise(
            "object_in_motion",
            "玉佩",
            "张三拿走玉佩",
            "Chapter-1",
            "Chapter-3",
            4,
        )
        .unwrap();
    let mut kernel = WriterAgentKernel::new("default", memory);
    let mut obs = observation("林墨发现玉佩的下落，却开始怀疑张三。新的冲突就此埋下。");
    obs.reason = ObservationReason::Save;
    obs.source = ObservationSource::ChapterSave;
    obs.chapter_title = Some("Chapter-2".to_string());

    kernel.observe(obs).unwrap();

    let next_beat = kernel
        .ledger_snapshot()
        .next_beat
        .expect("next beat should be derived from saved result");
    assert!(next_beat.goal.contains("冲突"));
    assert!(next_beat
        .carryovers
        .iter()
        .any(|line| line.contains("玉佩")));
    assert!(next_beat
        .source_refs
        .iter()
        .any(|source| source.contains("chapter_save:Chapter-2")));
}

#[test]
fn accepted_memory_candidate_writes_ledger() {
    let memory = WriterMemory::open(std::path::Path::new(":memory:")).unwrap();
    let mut kernel = WriterAgentKernel::new("default", memory);
    let mut obs = observation("那个少年名叫沈照，袖中藏着一枚玉佩。");
    obs.reason = ObservationReason::Save;
    obs.source = ObservationSource::ChapterSave;

    let proposal = kernel
        .observe(obs)
        .unwrap()
        .into_iter()
        .find(|proposal| proposal.kind == ProposalKind::CanonUpdate)
        .unwrap();
    let result = kernel
        .approve_editor_operation_with_approval(
            proposal.operations[0].clone(),
            "",
            Some(&test_approval("memory_candidate")),
        )
        .unwrap();

    assert!(result.success);
    assert!(kernel
        .memory
        .get_character_by_name("沈照")
        .unwrap()
        .is_some());
}

#[test]
fn promise_resolve_operation_closes_open_promise() {
    let memory = WriterMemory::open(std::path::Path::new(":memory:")).unwrap();
    let promise_id = memory
        .add_promise(
            "object_in_motion",
            "玉佩",
            "张三拿走玉佩",
            "Chapter-1",
            "Chapter-4",
            4,
        )
        .unwrap();
    let mut kernel = WriterAgentKernel::new("default", memory);
    let result = kernel
        .approve_editor_operation_with_approval(
            WriterOperation::PromiseResolve {
                promise_id: promise_id.to_string(),
                chapter: "Chapter-4".to_string(),
            },
            "",
            Some(&test_approval("promise_test")),
        )
        .unwrap();

    assert!(result.success);
    assert!(kernel.ledger_snapshot().open_promises.is_empty());
}

#[test]
fn story_contract_operation_updates_ledger_snapshot() {
    let memory = WriterMemory::open(std::path::Path::new(":memory:")).unwrap();
    let mut kernel = WriterAgentKernel::new("novel-a", memory);
    let result = kernel
        .approve_editor_operation_with_approval(
            WriterOperation::StoryContractUpsert {
                contract: crate::writer_agent::operation::StoryContractOp {
                    project_id: "novel-a".to_string(),
                    title: "寒影录".to_string(),
                    genre: "玄幻".to_string(),
                    target_reader: "长篇玄幻读者".to_string(),
                    reader_promise: "刀客追查玉佩真相。".to_string(),
                    first_30_chapter_promise: "建立宗门危机与玉佩谜团。".to_string(),
                    main_conflict: "复仇与守护的冲突。".to_string(),
                    structural_boundary: "不得提前泄露玉佩来源。".to_string(),
                    tone_contract: "克制、冷峻、少解释。".to_string(),
                },
            },
            "",
            Some(&test_approval("contract_test")),
        )
        .unwrap();

    assert!(result.success);
    let ledger = kernel.ledger_snapshot();
    let contract = ledger.story_contract.unwrap();
    assert_eq!(contract.title, "寒影录");
    assert!(contract.render_for_context().contains("前30章承诺"));
}

#[test]
fn story_contract_operation_rejects_incomplete_foundation() {
    let memory = WriterMemory::open(std::path::Path::new(":memory:")).unwrap();
    let mut kernel = WriterAgentKernel::new("novel-a", memory);
    let result = kernel
        .approve_editor_operation_with_approval(
            WriterOperation::StoryContractUpsert {
                contract: crate::writer_agent::operation::StoryContractOp {
                    project_id: "novel-a".to_string(),
                    title: "寒影录".to_string(),
                    genre: "玄幻".to_string(),
                    target_reader: "".to_string(),
                    reader_promise: "爽文".to_string(),
                    first_30_chapter_promise: "".to_string(),
                    main_conflict: "复仇".to_string(),
                    structural_boundary: "".to_string(),
                    tone_contract: "".to_string(),
                },
            },
            "",
            Some(&test_approval("contract_test")),
        )
        .unwrap();

    assert!(!result.success);
    assert_eq!(result.error.unwrap().code, "invalid");
    assert!(kernel.ledger_snapshot().story_contract.is_none());
}

#[test]
fn chapter_mission_operation_updates_ledger_snapshot() {
    let memory = WriterMemory::open(std::path::Path::new(":memory:")).unwrap();
    let mut kernel = WriterAgentKernel::new("novel-a", memory);
    kernel.active_chapter = Some("第一章".to_string());
    let result = kernel
        .approve_editor_operation_with_approval(
            WriterOperation::ChapterMissionUpsert {
                mission: crate::writer_agent::operation::ChapterMissionOp {
                    project_id: "novel-a".to_string(),
                    chapter_title: "第一章".to_string(),
                    mission: "林墨发现玉佩线索。".to_string(),
                    must_include: "推进玉佩线索".to_string(),
                    must_not: "不要提前揭开真相".to_string(),
                    expected_ending: "以新的疑问收束。".to_string(),
                    status: "active".to_string(),
                    source_ref: "test".to_string(),
                    blocked_reason: String::new(),
                    retired_history: String::new(),
                },
            },
            "",
            Some(&test_approval("mission_test")),
        )
        .unwrap();

    assert!(result.success);
    let ledger = kernel.ledger_snapshot();
    assert_eq!(ledger.chapter_missions.len(), 1);
    assert_eq!(
        ledger.active_chapter_mission.unwrap().mission,
        "林墨发现玉佩线索。"
    );
    assert_eq!(
        kernel
            .ledger_snapshot()
            .active_chapter_mission
            .unwrap()
            .status,
        "active"
    );
}

#[test]
fn chapter_mission_status_machine_accepts_new_statuses_and_legacy_alias() {
    let memory = WriterMemory::open(std::path::Path::new(":memory:")).unwrap();
    let mut kernel = WriterAgentKernel::new("novel-a", memory);
    kernel.active_chapter = Some("第一章".to_string());

    for (raw, normalized) in [
        ("in_progress", "active"),
        ("active", "active"),
        ("draft", "draft"),
        ("blocked", "blocked"),
        ("retired", "retired"),
        ("needs_review", "needs_review"),
    ] {
        let result = kernel
            .approve_editor_operation_with_approval(
                WriterOperation::ChapterMissionUpsert {
                    mission: crate::writer_agent::operation::ChapterMissionOp {
                        project_id: "novel-a".to_string(),
                        chapter_title: "第一章".to_string(),
                        mission: "林墨发现玉佩线索。".to_string(),
                        must_include: "推进玉佩线索".to_string(),
                        must_not: "不要提前揭开真相".to_string(),
                        expected_ending: "以新的疑问收束。".to_string(),
                        status: raw.to_string(),
                        source_ref: "test".to_string(),
                        blocked_reason: if raw == "blocked" {
                            "等待作者确认张三是否已经离场。".to_string()
                        } else {
                            String::new()
                        },
                        retired_history: if raw == "retired" {
                            "作者已改用第二章任务承接这条线。".to_string()
                        } else {
                            String::new()
                        },
                    },
                },
                "",
                Some(&test_approval("mission_status_machine")),
            )
            .unwrap();
        assert!(result.success, "{raw} should be accepted");
        assert_eq!(
            kernel
                .ledger_snapshot()
                .active_chapter_mission
                .unwrap()
                .status,
            normalized
        );
    }
}

#[test]
fn chapter_mission_blocked_and_retired_require_explanations() {
    let memory = WriterMemory::open(std::path::Path::new(":memory:")).unwrap();
    let mut kernel = WriterAgentKernel::new("novel-a", memory);
    kernel.active_chapter = Some("第一章".to_string());

    for (status, expected) in [
        ("blocked", "blocked_reason"),
        ("retired", "retired_history"),
    ] {
        let result = kernel
            .approve_editor_operation_with_approval(
                WriterOperation::ChapterMissionUpsert {
                    mission: crate::writer_agent::operation::ChapterMissionOp {
                        project_id: "novel-a".to_string(),
                        chapter_title: "第一章".to_string(),
                        mission: "林墨发现玉佩线索。".to_string(),
                        must_include: "推进玉佩线索".to_string(),
                        must_not: "不要提前揭开真相".to_string(),
                        expected_ending: "以新的疑问收束。".to_string(),
                        status: status.to_string(),
                        source_ref: "test".to_string(),
                        blocked_reason: String::new(),
                        retired_history: String::new(),
                    },
                },
                "",
                Some(&test_approval("mission_status_explanation")),
            )
            .unwrap();

        assert!(!result.success, "{status} should require explanation");
        let error = result.error.expect("invalid mission should return error");
        assert_eq!(error.code, "invalid");
        assert!(
            error.message.contains(expected),
            "{status} error should mention {expected}: {}",
            error.message
        );
    }
}

#[test]
fn chapter_mission_operation_rejects_vague_foundation() {
    let memory = WriterMemory::open(std::path::Path::new(":memory:")).unwrap();
    let mut kernel = WriterAgentKernel::new("novel-a", memory);
    kernel.active_chapter = Some("第一章".to_string());
    let result = kernel
        .approve_editor_operation_with_approval(
            WriterOperation::ChapterMissionUpsert {
                mission: crate::writer_agent::operation::ChapterMissionOp {
                    project_id: "novel-a".to_string(),
                    chapter_title: "第一章".to_string(),
                    mission: "打架".to_string(),
                    must_include: "".to_string(),
                    must_not: "剧透".to_string(),
                    expected_ending: "".to_string(),
                    status: "in_progress".to_string(),
                    source_ref: "test".to_string(),
                    blocked_reason: String::new(),
                    retired_history: String::new(),
                },
            },
            "",
            Some(&test_approval("mission_test")),
        )
        .unwrap();

    assert!(!result.success);
    assert_eq!(result.error.unwrap().code, "invalid");
    assert!(kernel.ledger_snapshot().active_chapter_mission.is_none());
}

#[test]
fn settlement_delta_apply_updates_memory_ledgers() {
    let memory = WriterMemory::open(std::path::Path::new(":memory:")).unwrap();
    memory.ensure_default_book_state("novel-a", "寒影录").unwrap();
    let delta = crate::chapter_generation::ChapterSettlementDelta {
        chapter_title: "Chapter-12".to_string(),
        chapter_revision: "rev-1".to_string(),
        summary: "林墨归还玉佩，并确认旧门后的血契未断。".to_string(),
        extraction: crate::chapter_generation::ChapterSettlementExtraction::default(),
        chapter_result: crate::chapter_generation::ChapterResultDelta {
            summary: "林墨归还玉佩，并确认旧门后的血契未断。".to_string(),
            state_changes: vec!["林墨归还玉佩。".to_string()],
            character_progress: vec!["林墨决定继续追查旧门。".to_string()],
            new_conflicts: vec!["旧门后的血契仍在逼近。".to_string()],
            new_clues: vec!["血契未断".to_string()],
            promise_updates: vec!["玉佩: 已归还".to_string()],
            canon_updates: vec![],
        },
        promise_updates: vec![crate::chapter_generation::ChapterPromiseDeltaEntry {
            action: crate::chapter_generation::ChapterPromiseDeltaAction::Introduced,
            promise_id: None,
            kind: "mystery_clue".to_string(),
            title: "旧门血契".to_string(),
            description: "旧门后的血契仍未解释。".to_string(),
            chapter: "Chapter-12".to_string(),
            source_ref: "chapter_settlement:Chapter-12:rev-1".to_string(),
            expected_payoff: "Chapter-14".to_string(),
            priority: 7,
            related_entities: vec!["林墨".to_string(), "旧门".to_string()],
            core: true,
            promoted: true,
            blocked_reason: String::new(),
            evidence: "旧门后的血契仍未解释。".to_string(),
        }],
        arc_updates: vec![],
        book_state_updates: vec![crate::chapter_generation::ChapterBookStateDeltaEntry {
            bucket: crate::chapter_generation::ChapterBookStateDeltaBucket::MegaPromise,
            value: "旧门血契 -> Chapter-14".to_string(),
            source_ref: "chapter_settlement:Chapter-12:rev-1".to_string(),
            reason: "core promise".to_string(),
        }],
        chapter_fact_delta: vec!["林墨归还玉佩。".to_string()],
        promise_delta: vec!["introduced: 旧门血契 -> Chapter-14".to_string()],
        arc_delta: vec![],
        book_state_delta: vec!["mega_promise: 旧门血契 -> Chapter-14".to_string()],
        continuity_issues: vec![],
        repairable: true,
        ..Default::default()
    };

    let applied = crate::writer_agent::settlement_apply::apply_chapter_settlement_delta(
        &memory,
        "novel-a",
        &delta,
    )
    .unwrap();

    assert!(applied.applied);
    assert_eq!(applied.promise_created, 1);
    let results = memory.list_recent_chapter_results("novel-a", 5).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].chapter_title, "Chapter-12");
    let promises = memory.get_open_promise_summaries().unwrap();
    assert_eq!(promises.len(), 1);
    assert_eq!(promises[0].title, "旧门血契");
    assert!(promises[0].core);
    assert!(promises[0].promoted);
    let book_state = memory.get_book_state("novel-a").unwrap().unwrap();
    assert!(book_state
        .mega_promises
        .iter()
        .any(|item| item.contains("旧门血契")));
}

#[test]
fn story_debt_snapshot_uses_five_state_promise_ordering() {
    let memory = WriterMemory::open(std::path::Path::new(":memory:")).unwrap();
    let open_id = memory
        .add_promise("plot_promise", "普通伏笔", "普通未回收伏笔。", "Chapter-1", "Chapter-3", 3)
        .unwrap();
    let core_id = memory
        .add_promise_with_status_flags(
            "plot_promise",
            "核心伏笔",
            "核心线必须尽快处理。",
            "Chapter-1",
            "seed",
            "Chapter-5",
            8,
            &[],
            "",
            true,
            true,
        )
        .unwrap();
    let promoted_id = memory
        .add_promise_with_status_flags(
            "plot_promise",
            "升级伏笔",
            "已经升级到近程计划。",
            "Chapter-2",
            "seed",
            "Chapter-6",
            5,
            &[],
            "",
            true,
            false,
        )
        .unwrap();
    let blocked_id = memory
        .add_promise_with_status_flags(
            "plot_promise",
            "阻塞伏笔",
            "需要先解除阻塞。",
            "Chapter-2",
            "seed",
            "Chapter-7",
            6,
            &[],
            "等待作者确认角色去向。",
            false,
            false,
        )
        .unwrap();
    memory.touch_promise_last_seen(open_id, "Chapter-2", "seed").unwrap();
    memory.touch_promise_last_seen(core_id, "Chapter-4", "seed").unwrap();
    memory.touch_promise_last_seen(promoted_id, "Chapter-4", "seed").unwrap();
    memory.touch_promise_last_seen(blocked_id, "Chapter-4", "seed").unwrap();
    let mut kernel = WriterAgentKernel::new("novel-a", memory);
    kernel.active_chapter = Some("Chapter-10".to_string());

    let story_debt = kernel.story_debt_snapshot();
    let promise_entries = story_debt
        .entries
        .iter()
        .filter(|entry| entry.category == crate::writer_agent::kernel::StoryDebtCategory::Promise)
        .collect::<Vec<_>>();

    assert!(promise_entries.len() >= 4);
    assert_eq!(promise_entries[0].status, crate::writer_agent::kernel::StoryDebtStatus::Core);
    assert_eq!(promise_entries[1].status, crate::writer_agent::kernel::StoryDebtStatus::Blocked);
    assert_eq!(
        promise_entries[2].status,
        crate::writer_agent::kernel::StoryDebtStatus::Promoted
    );
    assert!(story_debt.open_count >= 3);
}

#[test]
fn settlement_upsert_preserves_original_result_created_at() {
    let memory = WriterMemory::open(std::path::Path::new(":memory:")).unwrap();
    let original = crate::writer_agent::memory::ChapterResultSummary {
        id: 0,
        project_id: "novel-a".to_string(),
        chapter_title: "Chapter-9".to_string(),
        chapter_revision: "rev-keep-time".to_string(),
        summary: "初版结果".to_string(),
        state_changes: vec!["林墨失去玉佩。".to_string()],
        character_progress: vec![],
        new_conflicts: vec![],
        new_clues: vec![],
        promise_updates: vec![],
        canon_updates: vec![],
        source_ref: "chapter_save:Chapter-9:rev-keep-time".to_string(),
        created_at: 111,
    };
    memory.record_chapter_result(&original).unwrap();

    let delta = crate::chapter_generation::ChapterSettlementDelta {
        chapter_title: "Chapter-9".to_string(),
        chapter_revision: "rev-keep-time".to_string(),
        summary: "修复后的结果".to_string(),
        extraction: crate::chapter_generation::ChapterSettlementExtraction::default(),
        chapter_result: crate::chapter_generation::ChapterResultDelta {
            summary: "修复后的结果".to_string(),
            state_changes: vec!["林墨失去玉佩。".to_string()],
            character_progress: vec![],
            new_conflicts: vec![],
            new_clues: vec![],
            promise_updates: vec![],
            canon_updates: vec![],
        },
        promise_updates: vec![],
        arc_updates: vec![],
        book_state_updates: vec![],
        chapter_fact_delta: vec![],
        promise_delta: vec![],
        arc_delta: vec![],
        book_state_delta: vec![],
        continuity_issues: vec![],
        repairable: true,
        ..Default::default()
    };

    crate::writer_agent::settlement_apply::apply_chapter_settlement_delta(&memory, "novel-a", &delta)
        .unwrap();

    let latest = memory
        .latest_chapter_result("novel-a", "Chapter-9")
        .unwrap()
        .unwrap();
    assert_eq!(latest.created_at, 111);
    assert_eq!(latest.summary, "修复后的结果");
}
