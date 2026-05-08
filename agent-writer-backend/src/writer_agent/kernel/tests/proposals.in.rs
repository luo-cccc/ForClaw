#[test]
fn pure_kernel_rejects_outline_update_without_project_runtime() {
    let memory = WriterMemory::open(std::path::Path::new(":memory:")).unwrap();
    let mut kernel = WriterAgentKernel::new("default", memory);
    let result = kernel
        .approve_editor_operation_with_approval(
            WriterOperation::OutlineUpdate {
                node_id: "Chapter-1".to_string(),
                patch: serde_json::json!({"summary": "new"}),
            },
            "",
            Some(&test_approval("outline_test")),
        )
        .unwrap();

    assert!(!result.success);
    assert!(result
        .error
        .unwrap()
        .message
        .contains("project storage runtime"));
}

#[test]
fn create_llm_ghost_proposal_registers_typed_operation() {
    let memory = WriterMemory::open(std::path::Path::new(":memory:")).unwrap();
    let mut kernel = WriterAgentKernel::new("default", memory);
    let proposal = kernel
            .create_llm_ghost_proposal(
                observation("林墨停在旧门前，风从裂开的门缝里钻出来，带着潮湿的冷意。他没有立刻推门，只把手按在刀柄上。"),
                "他终于听见门后有人低声念出了他的名字。".to_string(),
                "test-model",
            )
            .unwrap();

    assert!(proposal.rationale.contains("LLM增强续写"));
    assert!(matches!(
        proposal.operations.first(),
        Some(WriterOperation::TextInsert { .. })
    ));
    assert_eq!(kernel.status().pending_proposals, 1);
}

#[test]
fn create_inline_operation_proposal_uses_selection_replace() {
    let memory = WriterMemory::open(std::path::Path::new(":memory:")).unwrap();
    let mut kernel = WriterAgentKernel::new("default", memory);
    let mut obs = observation("林墨握住刀柄，沉默片刻。");
    obs.reason = ObservationReason::Explicit;
    obs.selection = Some(super::super::observation::TextSelection {
        from: 2,
        to: 6,
        text: "握住刀柄".to_string(),
    });

    let proposal = kernel
        .create_inline_operation_proposal(
            obs,
            "改得更紧张",
            "指节一点点扣紧刀柄".to_string(),
            "test-model",
        )
        .unwrap();

    assert_eq!(proposal.kind, ProposalKind::ParallelDraft);
    assert!(proposal.rationale.contains("Inline typed operation"));
    assert!(matches!(
        proposal.operations.first(),
        Some(WriterOperation::TextReplace { from: 2, to: 6, .. })
    ));
}

#[test]
fn create_inline_operation_proposal_without_selection_inserts_at_cursor() {
    let memory = WriterMemory::open(std::path::Path::new(":memory:")).unwrap();
    let mut kernel = WriterAgentKernel::new("default", memory);
    let mut obs = observation("林墨停在门前。");
    obs.reason = ObservationReason::Explicit;
    obs.cursor = Some(TextRange { from: 7, to: 7 });

    let proposal = kernel
        .create_inline_operation_proposal(
            obs,
            "补一句动作",
            "他把呼吸压得更低。".to_string(),
            "test-model",
        )
        .unwrap();

    assert!(matches!(
        proposal.operations.first(),
        Some(WriterOperation::TextInsert { at: 7, .. })
    ));
}

#[test]
fn duplicate_ghost_proposal_is_suppressed_for_same_observation_slot() {
    let memory = WriterMemory::open(std::path::Path::new(":memory:")).unwrap();
    let mut kernel = WriterAgentKernel::new("default", memory);
    let obs = observation("林墨停在旧门前，风从裂开的门缝里钻出来，带着潮湿的冷意。他没有立刻推门，只把手按在刀柄上。");

    let first = kernel.observe(obs.clone()).unwrap();
    let second = kernel.observe(obs).unwrap();

    assert!(first
        .iter()
        .any(|proposal| proposal.kind == ProposalKind::Ghost));
    assert!(!second
        .iter()
        .any(|proposal| proposal.kind == ProposalKind::Ghost));
    assert_eq!(kernel.status().pending_proposals, 1);
}

#[test]
fn implicit_ghost_rejections_snooze_repeated_ignored_slot() {
    let memory = WriterMemory::open(std::path::Path::new(":memory:")).unwrap();
    let mut kernel = WriterAgentKernel::new("default", memory);
    let paragraph = "林墨停在旧门前，风从裂开的门缝里钻出来，带着潮湿的冷意。他没有立刻推门，只把手按在刀柄上。";

    let mut first_obs = observation(paragraph);
    first_obs.id = "obs-ignored-1".to_string();
    let first = kernel.observe(first_obs).unwrap();
    let first_ghost = first
        .iter()
        .find(|proposal| proposal.kind == ProposalKind::Ghost)
        .unwrap();
    let first_id = first_ghost.id.clone();
    assert!(!kernel
        .record_implicit_ghost_rejection(&first_id, now_ms())
        .unwrap());
    assert_eq!(kernel.status().pending_proposals, 0);

    let mut second_obs = observation(paragraph);
    second_obs.id = "obs-ignored-2".to_string();
    second_obs.cursor = Some(TextRange { from: 11, to: 11 });
    let second = kernel.observe(second_obs).unwrap();
    let second_id = second
        .iter()
        .find(|proposal| proposal.kind == ProposalKind::Ghost)
        .unwrap()
        .id
        .clone();
    assert!(!kernel
        .record_implicit_ghost_rejection(&second_id, now_ms())
        .unwrap());

    let mut third_obs = observation(paragraph);
    third_obs.id = "obs-ignored-3".to_string();
    third_obs.cursor = Some(TextRange { from: 12, to: 12 });
    let third = kernel.observe(third_obs).unwrap();
    let third_id = third
        .iter()
        .find(|proposal| proposal.kind == ProposalKind::Ghost)
        .unwrap()
        .id
        .clone();
    assert!(kernel
        .record_implicit_ghost_rejection(&third_id, now_ms())
        .unwrap());

    let mut fourth_obs = observation(paragraph);
    fourth_obs.id = "obs-ignored-4".to_string();
    fourth_obs.cursor = Some(TextRange { from: 13, to: 13 });
    let fourth = kernel.observe(fourth_obs).unwrap();
    assert!(!fourth
        .iter()
        .any(|proposal| proposal.kind == ProposalKind::Ghost));
}

#[test]
fn llm_ghost_supersedes_local_ghost_for_same_observation_slot() {
    let memory = WriterMemory::open(std::path::Path::new(":memory:")).unwrap();
    let mut kernel = WriterAgentKernel::new("default", memory);
    let obs = observation("林墨停在旧门前，风从裂开的门缝里钻出来，带着潮湿的冷意。他没有立刻推门，只把手按在刀柄上。");

    let local = kernel.observe(obs.clone()).unwrap();
    let local_ghost = local
        .iter()
        .find(|proposal| proposal.kind == ProposalKind::Ghost)
        .unwrap()
        .id
        .clone();
    let llm = kernel
        .create_llm_ghost_proposal(
            obs,
            "他终于听见门后有人低声念出了他的名字。".to_string(),
            "test-model",
        )
        .unwrap();

    assert!(llm.rationale.contains("LLM增强续写"));
    assert!(kernel.superseded_proposals.contains(&local_ghost));
    assert_eq!(kernel.status().pending_proposals, 1);
}

#[test]
fn rejected_ghost_suppresses_same_slot_temporarily() {
    let memory = WriterMemory::open(std::path::Path::new(":memory:")).unwrap();
    let mut kernel = WriterAgentKernel::new("default", memory);
    let obs = observation("林墨停在旧门前，风从裂开的门缝里钻出来，带着潮湿的冷意。他没有立刻推门，只把手按在刀柄上。");
    let first = kernel.observe(obs.clone()).unwrap();
    let ghost = first
        .iter()
        .find(|proposal| proposal.kind == ProposalKind::Ghost)
        .unwrap();

    kernel
        .apply_feedback(ProposalFeedback {
            proposal_id: ghost.id.clone(),
            action: FeedbackAction::Rejected,
            final_text: None,
            reason: Some("too soon".to_string()),
            created_at: now_ms(),
        })
        .unwrap();

    let mut next_obs = obs;
    next_obs.id = "obs-2".to_string();
    let second = kernel.observe(next_obs).unwrap();

    assert!(!second
        .iter()
        .any(|proposal| proposal.kind == ProposalKind::Ghost));
}

#[test]
fn pending_proposals_excludes_superseded_feedback_and_expired() {
    let memory = WriterMemory::open(std::path::Path::new(":memory:")).unwrap();
    let mut kernel = WriterAgentKernel::new("default", memory);
    let obs = observation("林墨停在旧门前，风从裂开的门缝里钻出来，带着潮湿的冷意。他没有立刻推门，只把手按在刀柄上。");
    let local = kernel.observe(obs.clone()).unwrap();
    let local_id = local
        .iter()
        .find(|proposal| proposal.kind == ProposalKind::Ghost)
        .unwrap()
        .id
        .clone();
    let llm = kernel
        .create_llm_ghost_proposal(
            obs,
            "他终于听见门后有人低声念出了他的名字。".to_string(),
            "test-model",
        )
        .unwrap();

    let pending = kernel.pending_proposals();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].id, llm.id);
    assert!(!pending.iter().any(|proposal| proposal.id == local_id));
}

#[test]
fn trace_snapshot_records_observation_proposal_and_state() {
    let memory = WriterMemory::open(std::path::Path::new(":memory:")).unwrap();
    let mut kernel = WriterAgentKernel::new("default", memory);
    let obs = observation("林墨停在旧门前，风从裂开的门缝里钻出来，带着潮湿的冷意。他没有立刻推门，只把手按在刀柄上。");
    let local = kernel.observe(obs.clone()).unwrap();
    let local_id = local
        .iter()
        .find(|proposal| proposal.kind == ProposalKind::Ghost)
        .unwrap()
        .id
        .clone();
    let llm = kernel
        .create_llm_ghost_proposal(
            obs,
            "他终于听见门后有人低声念出了他的名字。".to_string(),
            "test-model",
        )
        .unwrap();

    let trace = kernel.trace_snapshot(10);
    assert_eq!(trace.recent_observations.len(), 1);
    assert!(trace
        .recent_proposals
        .iter()
        .any(|proposal| proposal.id == local_id && proposal.state == "superseded"));
    let llm_trace = trace
        .recent_proposals
        .iter()
        .find(|proposal| proposal.id == llm.id && proposal.state == "pending")
        .expect("llm proposal trace should exist");
    let budget = llm_trace
        .context_budget
        .as_ref()
        .expect("context budget should be recorded for LLM proposal");
    assert_eq!(budget.task, "GhostWriting");
    assert!(budget.used <= budget.total_budget);
    assert!(!budget.source_reports.is_empty());
}

#[test]
fn trace_snapshot_survives_kernel_restart() {
    let db_path = std::env::temp_dir().join(format!(
        "forge-trace-{}.sqlite",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let ghost_id = {
        let memory = WriterMemory::open(&db_path).unwrap();
        let mut kernel = WriterAgentKernel::new("default", memory);
        let obs = observation("林墨停在旧门前，风从裂开的门缝里钻出来，带着潮湿的冷意。他没有立刻推门，只把手按在刀柄上。");
        let proposals = kernel.observe(obs).unwrap();
        let ghost = proposals
            .iter()
            .find(|proposal| proposal.kind == ProposalKind::Ghost)
            .unwrap()
            .id
            .clone();
        kernel
            .apply_feedback(ProposalFeedback {
                proposal_id: ghost.clone(),
                action: FeedbackAction::Rejected,
                final_text: None,
                reason: Some("too early".to_string()),
                created_at: 42,
            })
            .unwrap();
        ghost
    };

    let memory = WriterMemory::open(&db_path).unwrap();
    let kernel = WriterAgentKernel::new("default", memory);
    let trace = kernel.trace_snapshot(10);
    let _ = std::fs::remove_file(&db_path);

    assert_eq!(trace.recent_observations.len(), 1);
    let ghost_trace = trace
        .recent_proposals
        .iter()
        .find(|proposal| proposal.id == ghost_id && proposal.state == "feedback:Rejected")
        .expect("persisted ghost trace should exist");
    assert!(ghost_trace.context_budget.is_some());
    assert!(trace
        .recent_feedback
        .iter()
        .any(|feedback| feedback.proposal_id == ghost_id
            && feedback.action == "Rejected"
            && feedback.reason.as_deref() == Some("too early")));
}

#[test]
fn proposal_ids_do_not_collide_across_kernel_restarts() {
    let db_path = std::env::temp_dir().join(format!(
        "forge-proposal-id-{}.sqlite",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));

    let first_id = {
        let memory = WriterMemory::open(&db_path).unwrap();
        let mut kernel = WriterAgentKernel::new("default", memory);
        let proposals = kernel
                .observe(observation("林墨停在旧门前，风从裂开的门缝里钻出来，带着潮湿的冷意。他没有立刻推门，只把手按在刀柄上。"))
                .unwrap();
        proposals
            .iter()
            .find(|proposal| proposal.kind == ProposalKind::Ghost)
            .unwrap()
            .id
            .clone()
    };

    let second_id = {
        let memory = WriterMemory::open(&db_path).unwrap();
        let mut kernel = WriterAgentKernel::new("default", memory);
        let mut obs = observation("林墨停在旧门前，风从裂开的门缝里钻出来，带着潮湿的冷意。他没有立刻推门，只把手按在刀柄上。");
        obs.id = "obs-restart-second".to_string();
        let proposals = kernel.observe(obs).unwrap();
        proposals
            .iter()
            .find(|proposal| proposal.kind == ProposalKind::Ghost)
            .unwrap()
            .id
            .clone()
    };

    let memory = WriterMemory::open(&db_path).unwrap();
    let kernel = WriterAgentKernel::new("default", memory);
    let trace = kernel.trace_snapshot(10);
    let _ = std::fs::remove_file(&db_path);

    assert_ne!(first_id, second_id);
    assert!(trace
        .recent_proposals
        .iter()
        .any(|proposal| proposal.id == first_id));
    assert!(trace
        .recent_proposals
        .iter()
        .any(|proposal| proposal.id == second_id));
}

