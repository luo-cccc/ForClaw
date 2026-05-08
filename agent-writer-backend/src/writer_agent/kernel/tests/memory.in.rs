#[test]
fn llm_memory_candidates_parse_filter_and_dedupe() {
    let obs = observation("沈照把玉佩藏进袖中。");
    let value = serde_json::json!({
        "canon": [
            {
                "kind": "location",
                "name": "沈照",
                "aliases": ["少年"],
                "summary": "沈照把玉佩藏进袖中。",
                "attributes": { "object": "玉佩" },
                "confidence": 0.82
            },
            {
                "kind": "location",
                "name": "沈照",
                "summary": "重复条目",
                "confidence": 0.92
            },
            {
                "kind": "object",
                "name": "低",
                "summary": "置信太低",
                "confidence": 0.3
            }
        ],
        "promises": [
            {
                "kind": "object_in_motion",
                "title": "玉佩",
                "description": "玉佩的下落需要后续交代。",
                "introducedChapter": "Chapter-1",
                "expectedPayoff": "说明玉佩来源",
                "priority": 4,
                "confidence": 0.81
            }
        ]
    });

    let candidates = llm_memory_candidates_from_value(value, &obs, "test-model");

    assert_eq!(candidates.len(), 2);
    assert!(matches!(
        &candidates[0],
        MemoryCandidate::Canon(entity) if entity.name == "沈照"
    ));
    assert!(matches!(
        &candidates[1],
        MemoryCandidate::Promise(promise) if promise.title == "玉佩"
    ));
}

#[test]
fn llm_memory_proposal_replaces_local_candidate_for_same_slot() {
    let memory = WriterMemory::open(std::path::Path::new(":memory:")).unwrap();
    let mut kernel = WriterAgentKernel::new("default", memory);
    let mut obs = observation("那个少年名叫沈照，袖中藏着一枚玉佩。");
    obs.reason = ObservationReason::Save;
    obs.source = ObservationSource::ChapterSave;

    let local = kernel.observe(obs.clone()).unwrap();
    let local_canon_id = local
        .iter()
        .find(|proposal| proposal.kind == ProposalKind::CanonUpdate)
        .unwrap()
        .id
        .clone();
    let llm = kernel.create_llm_memory_proposals(
        obs,
        serde_json::json!({
            "canon": [{
                "kind": "character",
                "name": "沈照",
                "summary": "沈照是本章出现的少年，袖中藏着玉佩。",
                "attributes": { "object": "玉佩" },
                "confidence": 0.86
            }],
            "promises": []
        }),
        "test-model",
    );

    assert_eq!(llm.len(), 1);
    assert!(llm[0].rationale.contains("LLM增强记忆抽取"));
    assert!(kernel.superseded_proposals.contains(&local_canon_id));
    assert!(kernel
        .pending_proposals()
        .iter()
        .any(|proposal| proposal.id == llm[0].id));
}

#[test]
fn rejected_memory_candidate_suppresses_future_same_slot() {
    let memory = WriterMemory::open(std::path::Path::new(":memory:")).unwrap();
    let mut kernel = WriterAgentKernel::new("default", memory);
    let mut obs = observation("那个少年名叫沈照，袖中藏着一枚玉佩。");
    obs.reason = ObservationReason::Save;
    obs.source = ObservationSource::ChapterSave;

    let first = kernel.observe(obs.clone()).unwrap();
    let canon = first
        .iter()
        .find(|proposal| proposal.kind == ProposalKind::CanonUpdate)
        .unwrap();
    kernel
        .apply_feedback(ProposalFeedback {
            proposal_id: canon.id.clone(),
            action: FeedbackAction::Rejected,
            final_text: None,
            reason: Some("not a durable canon item".to_string()),
            created_at: now_ms(),
        })
        .unwrap();

    let mut next = obs;
    next.id = "obs-save-2".to_string();
    let second = kernel.observe(next).unwrap();

    assert!(!second.iter().any(|proposal| {
        matches!(
            proposal.operations.first(),
            Some(WriterOperation::CanonUpsertEntity { entity }) if entity.name == "沈照"
        )
    }));
}

#[test]
fn same_entity_memory_candidate_uses_attribute_merge_operation() {
    let memory = WriterMemory::open(std::path::Path::new(":memory:")).unwrap();
    memory
        .upsert_canon_entity(
            "location",
            "林墨",
            &[],
            "林墨惯用寒影刀的刀客。",
            &serde_json::json!({"weapon": "寒影刀"}),
            0.9,
        )
        .unwrap();
    let mut kernel = WriterAgentKernel::new("default", memory);
    let mut obs = observation("林墨的师门是北境寒山宗，他仍握着寒影刀。");
    obs.reason = ObservationReason::Save;
    obs.source = ObservationSource::ChapterSave;

    let proposals = kernel.create_llm_memory_proposals(
        obs,
        serde_json::json!({
            "canon": [{
                "kind": "location",
                "name": "林墨",
                "summary": "林墨出身北境寒山宗，惯用寒影刀。",
                "attributes": { "origin": "北境寒山宗" },
                "confidence": 0.88
            }],
            "promises": []
        }),
        "test-model",
    );

    let merge = proposals
        .iter()
        .find_map(|proposal| proposal.operations.first())
        .expect("expected canon attribute merge operation");
    assert!(matches!(
        merge,
        WriterOperation::CanonUpdateAttribute {
            entity,
            attribute,
            value,
            ..
        } if entity == "林墨" && attribute == "origin" && value == "北境寒山宗"
    ));
    assert!(!proposals.iter().any(|proposal| {
        matches!(
            proposal.operations.first(),
            Some(WriterOperation::CanonUpsertEntity { .. })
        )
    }));
}

#[test]
fn accepted_memory_candidate_records_positive_extraction_preference() {
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
    let proposal_id = proposal.id.clone();
    kernel
        .apply_feedback(ProposalFeedback {
            proposal_id: proposal_id.clone(),
            action: FeedbackAction::Accepted,
            final_text: None,
            reason: None,
            created_at: now_ms(),
        })
        .unwrap();

    let preferences = kernel.memory.list_style_preferences(20).unwrap();
    assert!(!preferences.iter().any(|preference| {
        preference
            .key
            .contains("memory_extract:memory|canon|character|沈照")
    }));

    let approval = test_approval_for_proposal("memory_candidate", &proposal_id);
    kernel
        .approve_editor_operation_with_approval(proposal.operations[0].clone(), "", Some(&approval))
        .unwrap();
    kernel
        .apply_feedback(proposal_feedback(
            proposal_id,
            FeedbackAction::Accepted,
            None,
        ))
        .unwrap();

    let preferences = kernel.memory.list_style_preferences(20).unwrap();
    assert!(preferences.iter().any(|preference| {
        preference
            .key
            .contains("memory_extract:memory|canon|character|沈照")
            && preference.accepted_count == 1
    }));
}

#[test]
fn ledger_snapshot_includes_memory_audit_for_candidate_feedback() {
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
    kernel
        .approve_editor_operation_with_approval(
            proposal.operations[0].clone(),
            "",
            Some(&test_approval_for_proposal("memory_audit", &proposal.id)),
        )
        .unwrap();
    kernel
        .apply_feedback(ProposalFeedback {
            proposal_id: proposal.id.clone(),
            action: FeedbackAction::Accepted,
            final_text: None,
            reason: Some("durable character".to_string()),
            created_at: 42,
        })
        .unwrap();

    let audit = kernel.ledger_snapshot().memory_audit;
    assert_eq!(audit.len(), 1);
    assert_eq!(audit[0].proposal_id, proposal.id);
    assert_eq!(audit[0].action, "Accepted");
    assert!(audit[0].title.contains("沈照"));
    assert!(audit[0].evidence.contains("沈照"));
    assert_eq!(audit[0].reason.as_deref(), Some("durable character"));
}

#[test]
fn ledger_snapshot_summarizes_memory_reliability_feedback() {
    let memory = WriterMemory::open(std::path::Path::new(":memory:")).unwrap();
    let mut kernel = WriterAgentKernel::new("default", memory);
    let mut accepted_obs = observation("那个少年名叫沈照，袖中藏着一枚玉佩。");
    accepted_obs.reason = ObservationReason::Save;
    accepted_obs.source = ObservationSource::ChapterSave;
    let accepted = kernel
        .observe(accepted_obs)
        .unwrap()
        .into_iter()
        .find(|proposal| proposal.kind == ProposalKind::CanonUpdate)
        .unwrap();
    kernel
        .approve_editor_operation_with_approval(
            accepted.operations[0].clone(),
            "",
            Some(&test_approval_for_proposal(
                "memory_reliability",
                &accepted.id,
            )),
        )
        .unwrap();
    kernel
        .apply_feedback(ProposalFeedback {
            proposal_id: accepted.id.clone(),
            action: FeedbackAction::Accepted,
            final_text: None,
            reason: Some("confirmed durable fact".to_string()),
            created_at: 42,
        })
        .unwrap();

    let mut rejected_obs = observation("青灯案上放着一封密信，林墨没有告诉任何人它的下落。");
    rejected_obs.id = "memory-reliability-rejected".to_string();
    rejected_obs.reason = ObservationReason::Save;
    rejected_obs.source = ObservationSource::ChapterSave;
    let rejected = kernel
        .observe(rejected_obs)
        .unwrap()
        .into_iter()
        .find(|proposal| proposal.kind == ProposalKind::PlotPromise)
        .unwrap();
    kernel
        .apply_feedback(ProposalFeedback {
            proposal_id: rejected.id.clone(),
            action: FeedbackAction::Rejected,
            final_text: None,
            reason: Some("作者纠错：这只是气氛，不是伏笔".to_string()),
            created_at: 43,
        })
        .unwrap();

    let reliability = kernel.ledger_snapshot().memory_reliability;
    let accepted_slot = reliability
        .iter()
        .find(|item| item.slot == "memory|canon|character|沈照")
        .unwrap();
    assert_eq!(accepted_slot.status, "trusted");
    assert_eq!(accepted_slot.reinforcement_count, 1);
    assert_eq!(accepted_slot.correction_count, 0);
    assert!(accepted_slot.reliability > 0.5);

    let rejected_slot = reliability
        .iter()
        .find(|item| item.slot == "memory|promise|object_whereabouts|密信")
        .unwrap();
    assert_eq!(rejected_slot.status, "needs_review");
    assert_eq!(rejected_slot.correction_count, 1);
    assert!(rejected_slot.reliability < 0.5);
    assert!(rejected_slot
        .last_source_error
        .as_deref()
        .is_some_and(|error| error.contains("不是伏笔")));
}

#[test]
fn memory_audit_survives_kernel_restart() {
    let db_path = std::env::temp_dir().join(format!(
        "forge-memory-audit-{}.sqlite",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));

    {
        let memory = WriterMemory::open(&db_path).unwrap();
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
        kernel
            .approve_editor_operation_with_approval(
                proposal.operations[0].clone(),
                "",
                Some(&test_approval_for_proposal("memory_audit", &proposal.id)),
            )
            .unwrap();
        kernel
            .apply_feedback(ProposalFeedback {
                proposal_id: proposal.id,
                action: FeedbackAction::Accepted,
                final_text: None,
                reason: Some("durable character".to_string()),
                created_at: 42,
            })
            .unwrap();
    }

    let memory = WriterMemory::open(&db_path).unwrap();
    let kernel = WriterAgentKernel::new("default", memory);
    let audit = kernel.ledger_snapshot().memory_audit;
    let _ = std::fs::remove_file(&db_path);

    assert_eq!(audit.len(), 1);
    assert_eq!(audit[0].action, "Accepted");
    assert!(audit[0].title.contains("沈照"));
    assert_eq!(audit[0].reason.as_deref(), Some("durable character"));
}
