fn observation(paragraph: &str) -> WriterObservation {
    WriterObservation {
        id: "obs-1".to_string(),
        created_at: now_ms(),
        source: ObservationSource::Editor,
        reason: ObservationReason::Idle,
        project_id: "default".to_string(),
        chapter_title: Some("Chapter-1".to_string()),
        chapter_revision: Some("rev".to_string()),
        cursor: Some(TextRange { from: 10, to: 10 }),
        selection: None,
        prefix: paragraph.to_string(),
        suffix: String::new(),
        paragraph: paragraph.to_string(),
        full_text_digest: None,
        editor_dirty: true,
    }
}

fn test_approval(source: &str) -> crate::writer_agent::operation::OperationApproval {
    test_approval_for_proposal(source, "proposal-test")
}

fn test_approval_for_proposal(
    source: &str,
    proposal_id: &str,
) -> crate::writer_agent::operation::OperationApproval {
    crate::writer_agent::operation::OperationApproval {
        source: source.to_string(),
        actor: "author".to_string(),
        reason: "test approval".to_string(),
        proposal_id: Some(proposal_id.to_string()),
        surfaced_to_user: true,
        created_at: now_ms(),
    }
}

#[test]
fn observe_emits_intent_proposal_and_feedback_records_decision() {
    let memory = WriterMemory::open(std::path::Path::new(":memory:")).unwrap();
    let mut kernel = WriterAgentKernel::new("default", memory);
    let proposals = kernel
            .observe(observation("林墨停在旧门前，风从裂开的门缝里钻出来，带着潮湿的冷意。他没有立刻推门，只把手按在刀柄上，听见里面有人压低声音。"))
            .unwrap();

    assert!(proposals
        .iter()
        .any(|proposal| proposal.kind == ProposalKind::Ghost));
    assert!(proposals.iter().any(|proposal| matches!(
        proposal.operations.first(),
        Some(WriterOperation::TextInsert { .. })
    )));
    assert!(proposals
        .iter()
        .any(|proposal| proposal.rationale.contains("ContextPack")));
    let proposal = proposals
        .iter()
        .find(|proposal| proposal.kind == ProposalKind::Ghost)
        .unwrap();
    let operation = proposal.operations[0].clone();
    kernel
        .approve_editor_operation_with_approval(
            operation.clone(),
            "rev",
            Some(&test_approval_for_proposal("ghost_feedback", &proposal.id)),
        )
        .unwrap();
    kernel
        .record_operation_durable_save(
            Some(proposal.id.clone()),
            operation,
            "editor_save:rev-2".to_string(),
        )
        .unwrap();
    let proposal_id = proposal.id.clone();
    kernel
        .apply_feedback(ProposalFeedback {
            proposal_id,
            action: FeedbackAction::Accepted,
            final_text: None,
            reason: None,
            created_at: 2_000,
        })
        .unwrap();

    let status = kernel.status();
    assert_eq!(status.total_feedback_events, 1);
    assert_eq!(status.pending_proposals, 0);
    assert!(kernel
        .ledger_snapshot()
        .recent_decisions
        .iter()
        .any(|decision| decision.decision == "accepted"));
}

#[test]
fn approve_editor_operation_checks_revision_without_mutating_text() {
    let memory = WriterMemory::open(std::path::Path::new(":memory:")).unwrap();
    let mut kernel = WriterAgentKernel::new("default", memory);
    let ok = kernel
        .approve_editor_operation_with_approval(
            WriterOperation::TextInsert {
                chapter: "Chapter-1".to_string(),
                at: 3,
                text: "续写".to_string(),
                revision: "rev-1".to_string(),
            },
            "rev-1",
            Some(&test_approval("text_revision")),
        )
        .unwrap();
    assert!(ok.success);

    let conflict = kernel
        .approve_editor_operation_with_approval(
            WriterOperation::TextInsert {
                chapter: "Chapter-1".to_string(),
                at: 3,
                text: "续写".to_string(),
                revision: "rev-1".to_string(),
            },
            "rev-2",
            Some(&test_approval("text_revision")),
        )
        .unwrap();
    assert!(!conflict.success);
    assert_eq!(conflict.error.unwrap().code, "conflict");
}

#[test]
fn approve_editor_operation_requires_context_for_memory_writes() {
    let memory = WriterMemory::open(std::path::Path::new(":memory:")).unwrap();
    let mut kernel = WriterAgentKernel::new("default", memory);
    let result = kernel
        .approve_editor_operation(
            WriterOperation::StyleUpdatePreference {
                key: "dialogue".to_string(),
                value: "prefers_subtext".to_string(),
            },
            "",
        )
        .unwrap();

    assert!(!result.success);
    assert_eq!(result.error.unwrap().code, "approval_required");
}

#[test]
fn execute_operation_records_annotation_without_text_revision() {
    let memory = WriterMemory::open(std::path::Path::new(":memory:")).unwrap();
    let mut kernel = WriterAgentKernel::new("default", memory);
    let result = kernel
        .execute_operation(
            WriterOperation::TextAnnotate {
                chapter: "Chapter-1".to_string(),
                from: 1,
                to: 4,
                message: "这里与设定冲突".to_string(),
                severity: crate::writer_agent::operation::AnnotationSeverity::Warning,
            },
            "",
            "",
        )
        .unwrap();

    assert!(result.success);
    assert!(result.revision_after.is_none());
    assert_eq!(kernel.ledger_snapshot().recent_decisions.len(), 1);
}

#[test]
fn execute_operation_rejects_write_without_approval_context() {
    let memory = WriterMemory::open(std::path::Path::new(":memory:")).unwrap();
    let mut kernel = WriterAgentKernel::new("default", memory);
    let result = kernel
        .execute_operation(
            WriterOperation::TextInsert {
                chapter: "Chapter-1".to_string(),
                at: 0,
                text: "续写".to_string(),
                revision: "rev-1".to_string(),
            },
            "",
            "rev-1",
        )
        .unwrap();

    assert!(!result.success);
    assert_eq!(result.error.unwrap().code, "approval_required");
}

#[test]
fn accepted_text_feedback_requires_durable_save() {
    let memory = WriterMemory::open(std::path::Path::new(":memory:")).unwrap();
    let mut kernel = WriterAgentKernel::new("default", memory);
    let proposal = kernel
        .create_llm_ghost_proposal(
            observation("林墨停在旧门前，风从裂开的门缝里钻出来。"),
            "他终于听见门后有人低声念出了他的名字。".to_string(),
            "test-model",
        )
        .unwrap();
    let operation = proposal.operations[0].clone();

    kernel
        .approve_editor_operation_with_approval(
            operation.clone(),
            "rev",
            Some(&test_approval_for_proposal("ghost", &proposal.id)),
        )
        .unwrap();
    kernel
        .apply_feedback(proposal_feedback(
            proposal.id.clone(),
            FeedbackAction::Accepted,
            None,
        ))
        .unwrap();
    assert!(!kernel
        .memory
        .list_style_preferences(20)
        .unwrap()
        .iter()
        .any(|preference| preference.key == "accepted_Ghost"));

    kernel
        .record_operation_durable_save(
            Some(proposal.id.clone()),
            operation,
            "editor_save:rev-2".to_string(),
        )
        .unwrap();
    kernel
        .apply_feedback(proposal_feedback(
            proposal.id.clone(),
            FeedbackAction::Accepted,
            None,
        ))
        .unwrap();

    assert!(kernel
        .memory
        .list_style_preferences(20)
        .unwrap()
        .iter()
        .any(|preference| preference.key == "accepted_Ghost"));
}

#[test]
fn execute_operation_upserts_canon_rule_and_style_preference() {
    let memory = WriterMemory::open(std::path::Path::new(":memory:")).unwrap();
    let mut kernel = WriterAgentKernel::new("default", memory);

    let rule = kernel
        .approve_editor_operation_with_approval(
            WriterOperation::CanonUpsertRule {
                rule: crate::writer_agent::operation::CanonRuleOp {
                    rule: "林墨不会主动弃刀。".to_string(),
                    category: "character_rule".to_string(),
                    priority: 8,
                },
            },
            "",
            Some(&test_approval("canon_test")),
        )
        .unwrap();
    assert!(rule.success);

    let style = kernel
        .approve_editor_operation_with_approval(
            WriterOperation::StyleUpdatePreference {
                key: "dialogue".to_string(),
                value: "prefers_subtext".to_string(),
            },
            "",
            Some(&test_approval("style_test")),
        )
        .unwrap();
    assert!(style.success);

    let ledger = kernel.ledger_snapshot();
    assert_eq!(ledger.canon_rules.len(), 1);
    assert_eq!(ledger.canon_rules[0].priority, 8);
    let preferences = kernel.memory.list_style_preferences(10).unwrap();
    assert!(preferences
        .iter()
        .any(|pref| pref.key == "style:dialogue.subtext" && pref.value == "prefers_subtext"));
}

#[test]
fn style_preference_operation_enforces_quality_gates() {
    let memory = WriterMemory::open(std::path::Path::new(":memory:")).unwrap();
    let mut kernel = WriterAgentKernel::new("default", memory);

    let vague = kernel
        .approve_editor_operation_with_approval(
            WriterOperation::StyleUpdatePreference {
                key: "tone".to_string(),
                value: "好".to_string(),
            },
            "",
            Some(&test_approval("style_quality")),
        )
        .unwrap();
    assert!(!vague.success);
    assert!(vague
        .error
        .as_ref()
        .is_some_and(|error| error.message.contains("too vague")));

    let first = kernel
        .approve_editor_operation_with_approval(
            WriterOperation::StyleUpdatePreference {
                key: "dialogue_subtext".to_string(),
                value: "对话偏短句留白，避免直接解释情绪".to_string(),
            },
            "",
            Some(&test_approval("style_quality")),
        )
        .unwrap();
    assert!(first.success);

    let duplicate = kernel
        .approve_editor_operation_with_approval(
            WriterOperation::StyleUpdatePreference {
                key: "dialogue_subtext".to_string(),
                value: "对话偏短句留白，避免直接解释情绪".to_string(),
            },
            "",
            Some(&test_approval("style_quality")),
        )
        .unwrap();
    assert!(!duplicate.success);
    assert!(duplicate
        .error
        .as_ref()
        .is_some_and(|error| error.message.contains("already exists")));

    let conflict = kernel
        .approve_editor_operation_with_approval(
            WriterOperation::StyleUpdatePreference {
                key: "dialogue_subtext".to_string(),
                value: "对话要完整解释每个角色的真实情绪".to_string(),
            },
            "",
            Some(&test_approval("style_quality")),
        )
        .unwrap();
    assert!(!conflict.success);
    assert!(conflict
        .error
        .as_ref()
        .is_some_and(|error| error.message.contains("conflicts")));

    let preferences = kernel.memory.list_style_preferences(10).unwrap();
    assert_eq!(
        preferences
            .iter()
            .filter(|preference| preference.key == "style:dialogue.subtext")
            .count(),
        1
    );
}

#[test]
fn style_preference_taxonomy_detects_same_slot_conflicts() {
    let memory = WriterMemory::open(std::path::Path::new(":memory:")).unwrap();
    let mut kernel = WriterAgentKernel::new("default", memory);

    assert_eq!(
        style_preference_taxonomy_slot("dialogue_subtext", "对话偏短句留白，避免直接解释情绪")
            .as_deref(),
        Some("dialogue.subtext")
    );

    let first = kernel
        .approve_editor_operation_with_approval(
            WriterOperation::StyleUpdatePreference {
                key: "dialogue_subtext".to_string(),
                value: "对话偏短句留白，避免直接解释情绪".to_string(),
            },
            "",
            Some(&test_approval("style_taxonomy")),
        )
        .unwrap();
    assert!(first.success);

    let same_slot_conflict = kernel
        .approve_editor_operation_with_approval(
            WriterOperation::StyleUpdatePreference {
                key: "dialogue_emotion_explanation".to_string(),
                value: "对话要完整解释每个角色的真实情绪".to_string(),
            },
            "",
            Some(&test_approval("style_taxonomy")),
        )
        .unwrap();
    assert!(!same_slot_conflict.success);
    assert!(same_slot_conflict
        .error
        .as_ref()
        .is_some_and(|error| error.message.contains("dialogue.subtext")));

    let same_slot_merge = kernel
        .approve_editor_operation_with_approval(
            WriterOperation::StyleUpdatePreference {
                key: "dialogue_subtext_followup".to_string(),
                value: "对话继续偏潜台词和短句留白，不要把情绪说满".to_string(),
            },
            "",
            Some(&test_approval("style_taxonomy")),
        )
        .unwrap();
    assert!(same_slot_merge.success);
    let preferences = kernel.memory.list_style_preferences(10).unwrap();
    assert!(preferences
        .iter()
        .any(|pref| pref.key == "style:dialogue.subtext"
            && pref.value.contains("对话偏短句留白，避免直接解释情绪")
            && pref
                .value
                .contains("对话继续偏潜台词和短句留白，不要把情绪说满")));

    let different_slot = kernel
        .approve_editor_operation_with_approval(
            WriterOperation::StyleUpdatePreference {
                key: "description_sensory_detail".to_string(),
                value: "描写优先保留气味、触感和画面细节".to_string(),
            },
            "",
            Some(&test_approval("style_taxonomy")),
        )
        .unwrap();
    assert!(different_slot.success);
}

