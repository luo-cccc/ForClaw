pub async fn run_chapter_generation_pipeline<P>(
    config: ChapterGenerationConfig<P>,
    mut emit: impl FnMut(ChapterGenerationEvent) + Send,
    mut record_task_packet: impl FnMut(&BuiltChapterContext) + Send,
    mut record_provider_budget: impl FnMut(&BuiltChapterContext, &WriterProviderBudgetReport) + Send,
    mut ensure_provider_budget_allowed: impl FnMut(
            &BuiltChapterContext,
            &WriterProviderBudgetReport,
        ) -> Result<(), ChapterGenerationError>
        + Send,
    mut record_model_started: impl FnMut(&BuiltChapterContext, &WriterProviderBudgetReport) + Send,
) -> PipelineTerminal
where
    P: ChapterGenerationProject,
{
    let request_id = config.payload
        .request_id
        .clone()
        .unwrap_or_else(|| make_request_id("chapter"));

    let quality_mode = config.payload.quality_mode.unwrap_or_default();
    let pipeline_t0 = std::time::Instant::now();
    // Timing variables declared at point of assignment
    let mut provider_calls: usize = 0;
    let mut provider_retries: usize = 0;

    emit(ChapterGenerationEvent::progress_with_detail(
        &request_id,
        "start",
        "管道启动",
        "running",
        "生成管道已启动",
        0,
        None,
        Some(quality_mode),
    ));

    emit(ChapterGenerationEvent::progress(
        &request_id,
        PHASE_STARTED,
        "running",
        "正在理解任务并读取工程结构...",
        5,
        None,
        Some(quality_mode),
    ));

    let memory = crate::writer_agent::memory::WriterMemory::open(&config.memory_path).ok();
    let open_promise_count = memory
        .as_ref()
        .and_then(|m| m.get_open_promises().ok())
        .map(|p| p.len())
        .unwrap_or(0);
    let prior_chapter_summaries: Vec<String> = memory
        .as_ref()
        .and_then(|m| m.list_recent_chapter_results(&config.project_id, 3).ok())
        .map(|results| {
            results
                .into_iter()
                .map(|r| r.summary)
                .filter(|s| !s.trim().is_empty())
                .collect()
        })
        .unwrap_or_default();
    let compiled_input = memory.as_ref().map(|m| {
        crate::writer_agent::input_governance::compiler::compile_input(
            m,
            &config.project_id,
            config
                .payload
                .target_chapter_title
                .as_deref()
                .unwrap_or("target chapter"),
            &config.payload.user_instruction,
        )
    });

    let build_input = BuildChapterContextInput {
        request_id: request_id.clone(),
        target_chapter_title: config.payload.target_chapter_title.clone(),
        target_chapter_number: config.payload.target_chapter_number,
        user_instruction: config.payload.user_instruction.clone(),
        budget: config.payload.budget.clone().unwrap_or_default(),
        chapter_contract: config.payload.chapter_contract.clone().unwrap_or_default(),
        chapter_summary_override: config.payload.chapter_summary_override.clone(),
        user_profile_entries: config.user_profile_entries.clone(),
        compiled_input,
        open_promise_count,
    };

    let context_t0 = std::time::Instant::now();
    let mut context = match build_chapter_context(&config.project, build_input).await {
        Ok(context) => context,
        Err(error) => {
            emit(ChapterGenerationEvent::failed(
                &request_id,
                error.clone(),
                Some(quality_mode),
            ));
            return PipelineTerminal::Failed(error);
        }
    };

    // Preflight: select generation strategy based on context size and risk.
    let strategy = select_generation_strategy(&context, 0);
    context.generation_strategy = strategy.clone();

    // P14-P19: Load world assets and run preflight checks
    let world_assets = load_world_assets_for_project(&config.project, &config.project_id);
    let canon_constraints = crate::writer_agent::world_bible::compile_canon_constraints(&world_assets);
    let preflight_result = crate::writer_agent::world_bible::preflight_canon_constraints(
        &world_assets,
        &canon_constraints,
        &context.target.summary,
    );
    if !preflight_result.warnings.is_empty() {
        for warning in &preflight_result.warnings {
            context.warnings.push(format!(
                "[world-bible preflight] {}: {}",
                warning.code, warning.message
            ));
        }
    }
    // P18: Inject conflict set warnings into preflight
    if !preflight_result.conflict_set.is_empty() {
        for conflict in &preflight_result.conflict_set {
            context.warnings.push(format!(
                "[world-bible conflict] {}: {} — overlapping terms: {:?}",
                conflict.constraint_a_id, conflict.constraint_b_id, conflict.overlapping_terms
            ));
        }
    }

    // P14-P19: Compile scene contract from world assets and attach to context
    // so it flows into the draft generation prompt.
    let scene_contract = {
        let mission = context
            .craft_plan
            .as_ref()
            .map(|p| p.objective.clone())
            .unwrap_or_else(|| context.target.title.clone());
        crate::writer_agent::world_bible::compile_scene_contract(
            &context.target.title,
            &mission,
            &world_assets,
            &canon_constraints,
            &context.required_state_deltas,
            Some(8),
        )
    };
    context.scene_contract = Some(scene_contract.clone());
    context.world_assets = world_assets.clone();

    // P18: Add world bible assets as context sources with evidence-type taxonomy
    for asset in &world_assets {
        let taxonomy = match asset.approval_status {
            crate::writer_agent::world_bible::ApprovalStatus::Proposed => {
                agent_harness_core::TAXONOMY_WORLD_PROPOSED_RULE
            }
            crate::writer_agent::world_bible::ApprovalStatus::Approved => {
                agent_harness_core::TAXONOMY_WORLD_APPROVED_RULE
            }
            crate::writer_agent::world_bible::ApprovalStatus::Rejected => {
                agent_harness_core::TAXONOMY_WORLD_RAW_EVIDENCE
            }
        };
        let content = format!("{}: {}", asset.name, asset.summary);
        context.sources.push(crate::chapter_generation::ChapterContextSource {
            source_type: "world_bible".to_string(),
            id: asset.id.clone(),
            label: format!("World Bible: {}", asset.name),
            original_chars: content.chars().count(),
            included_chars: content.chars().count(),
            truncated: false,
            score: None,
            taxonomy: taxonomy.to_string(),
            role: "grounding".to_string(),
            elapsed_ms: 0,
            retrieval_status: "ok".to_string(),
        });
    }

    let context_built_ms = context_t0.elapsed().as_millis() as u64;

    record_task_packet(&context);

    // Checkpoint 1: context built
    let mut checkpoint_counter: usize = 0;
    let mut budget_spent_micros: u64 = 0;
    write_chapter_generation_checkpoint(
        &config.memory_path,
        &config.project_id,
        &request_id,
        &mut checkpoint_counter,
        "context_built",
        &context.target.title,
        budget_spent_micros,
        &[],
    );

    emit(ChapterGenerationEvent {
        request_id: request_id.clone(),
        phase: PHASE_PREFLIGHT.to_string(),
        detail: Some("预检完成".to_string()),
        status: "done".to_string(),
        message: format!(
            "检索到 {} 个上下文来源，当前提示上下文 {} 字。策略: {:?}",
            context.sources.len(),
            context.budget.included_chars,
            strategy,
        ),
        progress: 25,
        target_chapter_title: Some(context.target.title.clone()),
        sources: Some(context.sources.clone()),
        budget: Some(context.budget.clone()),
        receipt: Some(context.receipt.clone()),
        intent_artifact: Some(context.intent_artifact.clone()),
        selected_evidence: Some(context.selected_evidence.clone()),
        rule_stack: Some(context.rule_stack.clone()),
        trace_artifact: Some(context.trace_artifact.clone()),
        scene_plan: Some(context.scene_plan.clone()),
        settlement_delta: None,
        settlement_apply: None,
        length_telemetry: None,
        artifact_refs: None,
        saved: None,
        chapter_contract: Some(context.chapter_contract.clone()),
        output_chars: None,
        conflict: None,
        error: None,
            generation_strategy: Some(strategy),
            quality_report: None,
            timing: None,
            quality_mode: Some(quality_mode),
            warnings: context.warnings.clone(),
        });

        emit(ChapterGenerationEvent::progress_with_detail(
            &request_id,
            PHASE_SCENE_PLAN,
        "场景规划完成",
        "running",
        "正在规划本章场景与长度目标...",
        35,
        Some(context.target.title.clone()),
        Some(quality_mode),
    ));

    emit(ChapterGenerationEvent::progress_with_detail(
        &request_id,
        PHASE_SEGMENT_DRAFT,
        "正在写第一段",
        "running",
        "正在撰写章节初稿...",
        45,
        Some(context.target.title.clone()),
        Some(quality_mode),
    ));

    // Checkpoint: before provider call (draft generation)
    write_agent_checkpoint(
        &config.memory_path,
        &config.project_id,
        &request_id,
        &mut checkpoint_counter,
        agent_harness_core::execution_plan::CheckpointPhase::ProviderCallBefore,
        &context.target.title,
        budget_spent_micros,
        &[],
        agent_harness_core::execution_plan::ResumePolicy::Rerun,
    );

    let draft_t0 = std::time::Instant::now();
    let mut draft = match generate_chapter_draft(
        &config.settings,
        &context,
        config.payload.provider_budget_approval.as_ref(),
        |context, report| ensure_provider_budget_allowed(context, report),
        |context, report| record_model_started(context, report),
    )
    .await
    {
        Ok(draft) => {
            record_provider_budget(&context, &draft.provider_budget);
            budget_spent_micros = budget_spent_micros
                .saturating_add(draft.provider_budget.estimated_cost_micros);
            draft
        }
        Err(error) => {
            if let Some(report) = provider_budget_report_from_error(&error) {
                record_provider_budget(&context, &report);
            }
            emit(ChapterGenerationEvent::failed(
                &request_id,
                error.clone(),
                Some(quality_mode),
            ));
            return PipelineTerminal::Failed(error);
        }
    };

    // Checkpoint: after provider call (draft generation)
    write_agent_checkpoint(
        &config.memory_path,
        &config.project_id,
        &request_id,
        &mut checkpoint_counter,
        agent_harness_core::execution_plan::CheckpointPhase::ProviderCallAfter,
        &context.target.title,
        budget_spent_micros,
        &[format!("draft:{}", request_id).as_str()],
        agent_harness_core::execution_plan::ResumePolicy::Rerun,
    );
    let draft_produced_ms = draft_t0.elapsed().as_millis() as u64;
    provider_calls += draft.provider_budget.provider_calls as usize;
    provider_retries += draft.provider_budget.provider_retries as usize;

    let draft_chars_before_repairs = draft.output_chars;
    let length_repair_t0 = std::time::Instant::now();
    let mut continuation_applied = false;
    let mut compress_applied = false;
    let mut hard_compress_applied = false;
    let mut continuation_latency_ms: u64 = 0;
    let mut compress_latency_ms: u64 = 0;
    let mut hard_compress_latency_ms: u64 = 0;

    match chapter_contract_outcome(
        &draft.content,
        &context.chapter_contract,
        ChapterContractPhase::ModelOutput,
    ) {
        ChapterContractOutcome::UnderMinChars => {
            emit(ChapterGenerationEvent::progress_with_detail(
                &request_id,
                PHASE_MERGE,
                "正在合并段落",
                "running",
                "初稿字数不足，正在续写以满足章节长度约束...",
                55,
                Some(context.target.title.clone()),
                Some(quality_mode),
            ));
            // Checkpoint: before provider call (continuation)
            write_agent_checkpoint(
                &config.memory_path,
                &config.project_id,
                &request_id,
                &mut checkpoint_counter,
                agent_harness_core::execution_plan::CheckpointPhase::ProviderCallBefore,
                &context.target.title,
                budget_spent_micros,
                &[],
                agent_harness_core::execution_plan::ResumePolicy::Rerun,
            );
            let continuation_t0 = std::time::Instant::now();
            let continuation = match continue_chapter_draft(
                &config.settings,
                &context,
                &draft.content,
                config.payload.provider_budget_approval.as_ref(),
                |context, report| ensure_provider_budget_allowed(context, report),
                |context, report| record_model_started(context, report),
            )
            .await
            {
                Ok(output) => {
                    record_provider_budget(&context, &output.provider_budget);
                    output
                }
                Err(error) => {
                    if let Some(report) = provider_budget_report_from_error(&error) {
                        record_provider_budget(&context, &report);
                    }
                    emit(ChapterGenerationEvent::failed(
                &request_id,
                error.clone(),
                Some(quality_mode),
            ));
                    return PipelineTerminal::Failed(error);
                }
            };
            // Checkpoint: after provider call (continuation)
            write_agent_checkpoint(
                &config.memory_path,
                &config.project_id,
                &request_id,
                &mut checkpoint_counter,
                agent_harness_core::execution_plan::CheckpointPhase::ProviderCallAfter,
                &context.target.title,
                budget_spent_micros,
                &[],
                agent_harness_core::execution_plan::ResumePolicy::Rerun,
            );
            if !continuation.content.is_empty() {
                if !draft.content.ends_with('\n') {
                    draft.content.push('\n');
                }
                draft.content.push_str(&continuation.content);
                draft.content = draft.content.trim().to_string();
                draft.output_chars = char_count(&draft.content);
                continuation_applied = true;
                continuation_latency_ms = continuation_t0.elapsed().as_millis() as u64;
            }
            provider_calls += continuation.provider_budget.provider_calls as usize;
            provider_retries += continuation.provider_budget.provider_retries as usize;
        }
        ChapterContractOutcome::OverMaxChars => {
            emit(ChapterGenerationEvent::progress(
                &request_id,
                PHASE_COMPRESS,
                "running",
                "初稿字数超出目标区间，正在压缩正文...",
                55,
                Some(context.target.title.clone()),
                Some(quality_mode),
            ));
            // Checkpoint: before provider call (compress)
            write_agent_checkpoint(
                &config.memory_path,
                &config.project_id,
                &request_id,
                &mut checkpoint_counter,
                agent_harness_core::execution_plan::CheckpointPhase::ProviderCallBefore,
                &context.target.title,
                budget_spent_micros,
                &[],
                agent_harness_core::execution_plan::ResumePolicy::Rerun,
            );
            let compress_t0 = std::time::Instant::now();
            let compressed = match compress_chapter_draft(
                &config.settings,
                &context,
                &draft.content,
                config.payload.provider_budget_approval.as_ref(),
                |context, report| ensure_provider_budget_allowed(context, report),
                |context, report| record_model_started(context, report),
            )
            .await
            {
                Ok(output) => {
                    record_provider_budget(&context, &output.provider_budget);
                    output
                }
                Err(error) => {
                    if let Some(report) = provider_budget_report_from_error(&error) {
                        record_provider_budget(&context, &report);
                    }
                    emit(ChapterGenerationEvent::failed(
                &request_id,
                error.clone(),
                Some(quality_mode),
            ));
                    return PipelineTerminal::Failed(error);
                }
            };
            // Checkpoint: after provider call (compress)
            write_agent_checkpoint(
                &config.memory_path,
                &config.project_id,
                &request_id,
                &mut checkpoint_counter,
                agent_harness_core::execution_plan::CheckpointPhase::ProviderCallAfter,
                &context.target.title,
                budget_spent_micros,
                &[],
                agent_harness_core::execution_plan::ResumePolicy::Rerun,
            );
            if !compressed.content.is_empty() {
                draft.content = compressed.content.trim().to_string();
                draft.output_chars = char_count(&draft.content);
                compress_applied = true;
                compress_latency_ms = compress_t0.elapsed().as_millis() as u64;
            }
            provider_calls += compressed.provider_budget.provider_calls as usize;
            provider_retries += compressed.provider_budget.provider_retries as usize;
        }
        ChapterContractOutcome::Valid
        | ChapterContractOutcome::UnderSaveFloor
        | ChapterContractOutcome::OverSaveCeiling => {}
    }

    if chapter_contract_outcome(
        &draft.content,
        &context.chapter_contract,
        ChapterContractPhase::ModelOutput,
    ) == ChapterContractOutcome::OverMaxChars
    {
        emit(ChapterGenerationEvent::progress(
            &request_id,
            PHASE_COMPRESS,
            "running",
            "修复后字数仍超出目标区间，正在进行强压缩...",
            60,
            Some(context.target.title.clone()),
            Some(quality_mode),
        ));
        // Checkpoint: before provider call (hard compress)
        write_agent_checkpoint(
            &config.memory_path,
            &config.project_id,
            &request_id,
            &mut checkpoint_counter,
            agent_harness_core::execution_plan::CheckpointPhase::ProviderCallBefore,
            &context.target.title,
            budget_spent_micros,
            &[],
            agent_harness_core::execution_plan::ResumePolicy::Rerun,
        );
        let hard_compress_t0 = std::time::Instant::now();
        let compressed = match compress_chapter_draft_hard(
            &config.settings,
            &context,
            &draft.content,
            config.payload.provider_budget_approval.as_ref(),
            |context, report| ensure_provider_budget_allowed(context, report),
            |context, report| record_model_started(context, report),
        )
        .await
        {
            Ok(output) => {
                record_provider_budget(&context, &output.provider_budget);
                output
            }
            Err(error) => {
                if let Some(report) = provider_budget_report_from_error(&error) {
                    record_provider_budget(&context, &report);
                }
                emit(ChapterGenerationEvent::failed(
                &request_id,
                error.clone(),
                Some(quality_mode),
            ));
                return PipelineTerminal::Failed(error);
            }
        };
        // Checkpoint: after provider call (hard compress)
        write_agent_checkpoint(
            &config.memory_path,
            &config.project_id,
            &request_id,
            &mut checkpoint_counter,
            agent_harness_core::execution_plan::CheckpointPhase::ProviderCallAfter,
            &context.target.title,
            budget_spent_micros,
            &[],
            agent_harness_core::execution_plan::ResumePolicy::Rerun,
        );
        if !compressed.content.is_empty() {
            draft.content = compressed.content.trim().to_string();
            draft.output_chars = char_count(&draft.content);
            hard_compress_applied = true;
            hard_compress_latency_ms = hard_compress_t0.elapsed().as_millis() as u64;
        }
        provider_calls += compressed.provider_budget.provider_calls as usize;
        provider_retries += compressed.provider_budget.provider_retries as usize;
    }

    emit(ChapterGenerationEvent::progress_with_detail(
        &request_id,
        PHASE_LENGTH_VALIDATE,
        "正在校验长度",
        "running",
        "正在校验章节长度约束...",
        63,
        Some(context.target.title.clone()),
        Some(quality_mode),
    ));

    if let Err(error) = validate_generated_content(
        &draft.content,
        &context.chapter_contract,
        ChapterContractPhase::ModelOutput,
    ) {
        emit(ChapterGenerationEvent::failed(
            &request_id,
            error.clone(),
            Some(quality_mode),
        ));
        return PipelineTerminal::Failed(error);
    }
    let length_repair_ms = length_repair_t0.elapsed().as_millis() as u64;

    // Checkpoint 2: draft produced (after length repairs)
    write_chapter_generation_checkpoint(
        &config.memory_path,
        &config.project_id,
        &request_id,
        &mut checkpoint_counter,
        "draft_produced",
        &context.target.title,
        budget_spent_micros,
        &[format!("draft:{}", request_id).as_str()],
    );

    // Quality evaluation: always evaluate draft quality after length repairs.
    let scene_craft_plan = context
        .craft_plan
        .as_ref()
        .cloned()
        .unwrap_or_default();
    // P14-P19: Reuse scene contract already compiled before draft generation
    let scene_contract = context.scene_contract.clone();

    let quality_signals = ChapterQualitySignals {
        anchor_keywords: context.quality_anchor_keywords.clone(),
        author_voice: context.author_voice_snapshot.clone(),
        required_anchors: context.required_story_anchors.clone(),
        required_state_deltas: context.required_state_deltas.clone(),
        prior_chapter_summaries,
        scene_contract,
        world_assets: context.world_assets.clone(),
        canon_constraints: Vec::new(),
        canon_terms: Vec::new(),
    };
    let quality_report_t0 = std::time::Instant::now();
    let quality_report_before = if quality_mode == GenerationQualityMode::Fast {
        ChapterQualityReport::default()
    } else {
        evaluate_chapter_quality_with_signals(
            &draft.content,
            &context.target.title,
            &scene_craft_plan,
            &[],
            context.chapter_contract.min_chars,
            context.chapter_contract.max_chars,
            &quality_signals,
        )
    };
    let quality_report_ms = quality_report_t0.elapsed().as_millis() as u64;

    // Targeted revision: if quality report has major/fatal issues, attempt
    // a single revision pass with the ChapterTargetedRevision profile.
    
    let mut quality_report_after_revision: Option<ChapterQualityReport> = None;
    let mut quality_report_after_attempt: Option<ChapterQualityReport> = None;
    let draft_before_revision = draft.content.clone();
    let mut revised_text_attempt: Option<String> = None;
    let mut revision_budget_skipped = false;
    let mut revision_attempted = false;
    let revision_t0 = std::time::Instant::now();
    let should_revise = match quality_mode {
        GenerationQualityMode::Fast => false,
        GenerationQualityMode::Balanced => {
            !quality_report_before.fatal_issues.is_empty()
                || !quality_report_before.major_issues.is_empty()
        }
        GenerationQualityMode::Strict => {
            let has_fatal_or_major = !quality_report_before.fatal_issues.is_empty()
                || !quality_report_before.major_issues.is_empty();
            let has_strict_gate = quality_report_before.metric_results.iter().any(|m| {
                matches!(
                    m.metric.as_str(),
                    "scene_repetition"
                        | "plot_progression"
                        | "new_information_density"
                        | "state_delta_coverage"
                ) && m.score < 0.5
            });
            has_fatal_or_major || has_strict_gate
        }
    };
    // P13: enforce provider call limit before revision
    let provider_call_limit = quality_mode.max_provider_calls();
    if provider_calls >= provider_call_limit && should_revise {
        match quality_mode {
            GenerationQualityMode::Strict => {
                let error = ChapterGenerationError::new(
                    "PROVIDER_CALL_LIMIT_EXCEEDED",
                    format!(
                        "Strict mode provider call limit ({}) reached before revision; approval required to continue.",
                        provider_call_limit
                    ),
                    true,
                );
                emit(ChapterGenerationEvent::failed(
                    &request_id,
                    error.clone(),
                    Some(quality_mode),
                ));
                return PipelineTerminal::Failed(error);
            }
            _ => {
                revision_budget_skipped = true;
                // Provider call limit reached — skip revision, keep original draft
            }
        }
    }

    if should_revise && !revision_budget_skipped {
        let revision_prompt = build_revision_prompt(
            &draft.content,
            &quality_report_before,
            3,
        );
        if !revision_prompt.is_empty() {
            emit(ChapterGenerationEvent::progress_with_detail(
                &request_id,
                PHASE_SCENE_PLAN,
                "定向修订中",
                "running",
                "正在定向修订低分项...",
                65,
                Some(context.target.title.clone()),
                Some(quality_mode),
            ));
            let revision_messages =
                vec![serde_json::json!({"role": "user", "content": revision_prompt})];
            // Record revision provider budget with approval gate
            let revision_budget = chapter_generation_provider_budget_for_profile(
                &config.settings,
                &revision_messages,
                crate::llm_runtime::LlmRequestProfile::ChapterTargetedRevision,
            );
            let revision_budget = crate::writer_agent::provider_budget::apply_provider_budget_approval(
                revision_budget,
                config.payload.provider_budget_approval.as_ref(),
            );
            if revision_budget.decision
                == crate::writer_agent::provider_budget::WriterProviderBudgetDecision::ApprovalRequired
            {
                revision_budget_skipped = true;
                // Budget not approved — skip revision, keep original draft
            } else if ensure_provider_budget_allowed(&context, &revision_budget).is_err() {
                revision_budget_skipped = true;
                // Budget denied — skip revision
            } else {
                record_provider_budget(&context, &revision_budget);
                revision_attempted = true;
                // Checkpoint: before provider call (revision)
                write_agent_checkpoint(
                    &config.memory_path,
                    &config.project_id,
                    &request_id,
                    &mut checkpoint_counter,
                    agent_harness_core::execution_plan::CheckpointPhase::ProviderCallBefore,
                    &context.target.title,
                    budget_spent_micros,
                    &[],
                    agent_harness_core::execution_plan::ResumePolicy::Rerun,
                );
                let revision_result = crate::llm_runtime::chat_text_profile_with_usage(
                    &config.settings,
                    revision_messages,
                    crate::llm_runtime::LlmRequestProfile::ChapterTargetedRevision,
                    300,
                )
                .await;
                // Checkpoint: after provider call (revision)
                write_agent_checkpoint(
                    &config.memory_path,
                    &config.project_id,
                    &request_id,
                    &mut checkpoint_counter,
                    agent_harness_core::execution_plan::CheckpointPhase::ProviderCallAfter,
                    &context.target.title,
                    budget_spent_micros,
                    &[],
                    agent_harness_core::execution_plan::ResumePolicy::Rerun,
                );
                if let Ok((revised, mut revision_usage)) = revision_result {
                    revision_usage.repaired = true;
                    provider_calls += revision_usage.provider_calls as usize;
                    provider_retries += revision_usage.provider_retries as usize;
                    // Strict length validation per plan.md: re-run ModelOutput contract
                    let length_ok = matches!(
                        chapter_contract_outcome(
                            &revised,
                            &context.chapter_contract,
                            ChapterContractPhase::ModelOutput,
                        ),
                        ChapterContractOutcome::Valid
                    );
                    if length_ok {
                        let after = evaluate_chapter_quality_with_signals(
                            &revised,
                            &context.target.title,
                            &scene_craft_plan,
                            &[],
                            context.chapter_contract.min_chars,
                            context.chapter_contract.max_chars,
                            &quality_signals,
                        );
                        quality_report_after_attempt = Some(after.clone());
                        revised_text_attempt = Some(revised.clone());
                        if after.overall_score > quality_report_before.overall_score {
                            draft.content = revised;
                            draft.output_chars = char_count(&draft.content);
                            quality_report_after_revision = Some(after);
                        }
                    }
                }
            }
        }
    }
    let targeted_revision_ms = revision_t0.elapsed().as_millis() as u64;
    let revision_improved = quality_report_after_revision.is_some();
    let had_issues = !quality_report_before.fatal_issues.is_empty()
        || !quality_report_before.major_issues.is_empty();
    let final_quality = quality_report_after_revision
        .clone()
        .unwrap_or(quality_report_before.clone());
    let target_changes = build_revision_target_changes_with_text(
        &quality_report_before,
        quality_report_after_attempt.as_ref(),
        revision_attempted,
        revision_budget_skipped,
        Some(&draft_before_revision),
        revised_text_attempt.as_deref(),
    );
    let craft_memory_updates = record_craft_memory_feedback(
        &config,
        &context,
        &quality_report_before,
        quality_report_after_attempt
            .as_ref()
            .unwrap_or(&final_quality),
        &target_changes,
        had_issues,
        revision_attempted,
    );

    let revision_report = RevisionReport {
        chapter_title: context.target.title.clone(),
        request_id: request_id.clone(),
        triggered: had_issues,
        budget_skipped: revision_budget_skipped,
        top_issues_before: quality_report_before.top_revision_targets.clone(),
        score_before: quality_report_before.overall_score,
        score_after: quality_report_after_attempt.as_ref().map(|r| r.overall_score),
        accepted: revision_improved,
        reason: if revision_improved {
            "Revision improved overall quality score".to_string()
        } else if !had_issues {
            "No major or fatal issues detected — revision not needed".to_string()
        } else if revision_budget_skipped {
            "Revision skipped due to budget constraints".to_string()
        } else {
            "Revision did not improve quality score — keeping original draft".to_string()
        },
        target_changes,
        craft_memory_updates,
    };

    // Checkpoint 3: quality report produced
    let quality_artifact_ref_1 = format!("quality_report.before:{}", request_id);
    let quality_artifact_ref_2 = format!("revision_report:{}", request_id);
    write_chapter_generation_checkpoint(
        &config.memory_path,
        &config.project_id,
        &request_id,
        &mut checkpoint_counter,
        "quality_report_produced",
        &context.target.title,
        budget_spent_micros,
        &[
            quality_artifact_ref_1.as_str(),
            quality_artifact_ref_2.as_str(),
        ],
    );

    emit(ChapterGenerationEvent::progress_with_detail(
        &request_id,
        PHASE_SAVE,
        "正在保存",
        "running",
        "正在保存章节并检查编辑器冲突...",
        70,
        Some(context.target.title.clone()),
        Some(quality_mode),
    ));

    // P17: Strict mode hard block — check for high-severity violations before save
    if quality_mode == GenerationQualityMode::Strict {
        let hard_violations: Vec<_> = final_quality
            .world_consistency_violations
            .iter()
            .filter(|v| matches!(v.severity, crate::writer_agent::world_bible::ConstraintSeverity::Hard))
            .collect();
        if !hard_violations.is_empty() {
            let violation_ids: Vec<String> = hard_violations
                .iter()
                .map(|v| v.constraint_id.clone())
                .collect();
            let error = ChapterGenerationError::with_details(
                "STRICT_MODE_BLOCKED",
                format!(
                    "Strict mode blocked save due to {} hard severity canon violation(s): {}",
                    hard_violations.len(),
                    violation_ids.join(", ")
                ),
                true,
                format!(
                    "Violations: {}",
                    hard_violations
                        .iter()
                        .map(|v| format!("{}: {}", v.constraint_id, v.message))
                        .collect::<Vec<_>>()
                        .join("; ")
                ),
            );
            emit(ChapterGenerationEvent::failed(
                &request_id,
                error.clone(),
                Some(quality_mode),
            ));
            return PipelineTerminal::Failed(error);
        }

        // P17: new_information_density score < 0.5 in Strict mode triggers warning or block
        // Check via the LOW_INFORMATION_DENSITY world consistency violation
        let nid_violation = final_quality
            .world_consistency_violations
            .iter()
            .find(|v| v.constraint_id == "LOW_INFORMATION_DENSITY");
        if let Some(v) = nid_violation {
            let error = ChapterGenerationError::new(
                "STRICT_MODE_INFO_DENSITY_LOW",
                format!(
                    "Strict mode blocked save: new_information_density violation — {}",
                    v.message
                ),
                true,
            );
            emit(ChapterGenerationEvent::failed(
                &request_id,
                error.clone(),
                Some(quality_mode),
            ));
            return PipelineTerminal::Failed(error);
        }
    }

    let save_input = SaveGeneratedChapterInput {
        request_id: request_id.clone(),
        target: context.target.clone(),
        generated_content: draft.content.clone(),
        chapter_contract: context.chapter_contract.clone(),
        base_revision: context.base_revision.clone(),
        save_mode: config.payload.save_mode,
        frontend_state: config.payload.frontend_state.clone(),
        receipt: context.receipt.clone(),
    };
    let save_t0 = std::time::Instant::now();
    let saved = match save_generated_chapter(&config.project, save_input) {
        Ok(saved) => saved,
        Err(error) => {
            if let Some(conflict) = save_conflict_from_error(&error) {
                emit(ChapterGenerationEvent::conflict(
                    &request_id,
                    conflict.clone(),
                    Some(quality_mode),
                ));
                return PipelineTerminal::Conflict(conflict);
            }
            emit(ChapterGenerationEvent::failed(
                &request_id,
                error.clone(),
                Some(quality_mode),
            ));
            return PipelineTerminal::Failed(error);
        }
    };
    let save_prepared_ms = save_t0.elapsed().as_millis() as u64;

    // Checkpoint 4: save prepared (AgentCheckpoint with SavePrepared phase)
    // Captures full state including conflict check results and approval status.
    let save_artifact_ref = format!("saved:{}/{}", saved.chapter_title, saved.new_revision);
    let approval_refs: Vec<String> = if config.payload.save_mode == crate::chapter_generation::SaveMode::SaveAsDraft {
        vec!["draft_auto_approved".to_string()]
    } else {
        vec!["manual_save".to_string()]
    };
    write_agent_checkpoint_with_payload(
        &config.memory_path,
        &config.project_id,
        &request_id,
        &mut checkpoint_counter,
        agent_harness_core::execution_plan::CheckpointPhase::SavePrepared,
        &saved.chapter_title,
        budget_spent_micros,
        &[save_artifact_ref.as_str()],
        agent_harness_core::execution_plan::ResumePolicy::RequireApproval,
        approval_refs.clone(),
        Some(serde_json::json!({
            "save_mode": format!("{:?}", config.payload.save_mode),
            "conflict_check": "passed",
            "approval_refs": approval_refs,
            "output_chars": saved.output_chars,
        })),
    );

    emit(ChapterGenerationEvent::progress_with_detail(
        &request_id,
        PHASE_POLISH,
        "正在润色",
        "running",
        "正在更新大纲状态...",
        85,
        Some(saved.chapter_title.clone()),
        Some(quality_mode),
    ));

    let settlement_t0 = std::time::Instant::now();
    let mut warnings = Vec::new();
    if let Err(error) = update_outline_after_generation(&config.project, &context.target, &saved) {
        warnings.push(format!("Outline update skipped: {}", error.message));
    }
    let settlement_delta = match build_chapter_settlement_delta(&config, &context, &draft.content, &saved) {
        Ok(delta) => delta,
        Err(error) => {
            warnings.push(format!("Settlement build failed: {}", error));
            ChapterSettlementDelta {
                chapter_title: saved.chapter_title.clone(),
                chapter_revision: saved.new_revision.clone(),
                summary: String::new(),
                extraction: ChapterSettlementExtraction::default(),
                chapter_result: ChapterResultDelta::default(),
                promise_updates: Vec::new(),
                arc_updates: Vec::new(),
                book_state_updates: Vec::new(),
                chapter_fact_delta: Vec::new(),
                promise_delta: Vec::new(),
                arc_delta: Vec::new(),
                book_state_delta: Vec::new(),
                continuity_issues: context.warnings.clone(),
                repairable: true,
                ..Default::default()
            }
        }
    };
    let settlement_apply =
        match crate::writer_agent::memory::WriterMemory::open(&config.memory_path) {
            Ok(memory) => match crate::writer_agent::settlement_apply::apply_chapter_settlement_delta(
                &memory,
                &config.project_id,
                &settlement_delta,
            ) {
                Ok(result) => Some(result),
                Err(error) => {
                    warnings.push(format!("Settlement apply failed: {}", error));
                    None
                }
            },
            Err(error) => {
                warnings.push(format!("Settlement memory open failed: {}", error));
                None
            }
        };
    let settlement_ms = settlement_t0.elapsed().as_millis() as u64;

    // Checkpoint 5: write-after settlement (WriteAfter phase)
    write_agent_checkpoint(
        &config.memory_path,
        &config.project_id,
        &request_id,
        &mut checkpoint_counter,
        agent_harness_core::execution_plan::CheckpointPhase::WriteAfter,
        &saved.chapter_title,
        budget_spent_micros,
        &[],
        agent_harness_core::execution_plan::ResumePolicy::Skip,
    );

    let timing = ChapterGenerationTiming {
        context_built_ms,
        draft_produced_ms,
        length_repair_ms,
        quality_report_ms,
        targeted_revision_ms,
        save_prepared_ms,
        settlement_ms,
        total_ms: pipeline_t0.elapsed().as_millis() as u64,
        provider_calls,
        provider_retries,
    };

    let length_telemetry = ChapterLengthTelemetry {
        target_chars: context.chapter_contract.target_chars,
        min_chars: context.chapter_contract.min_chars,
        max_chars: context.chapter_contract.max_chars,
        save_hard_floor_chars: context.chapter_contract.save_hard_floor_chars,
        save_hard_ceiling_chars: context.chapter_contract.save_hard_ceiling_chars,
        draft_chars: Some(draft_chars_before_repairs),
        final_chars: Some(saved.output_chars),
        continuation_applied,
        compress_applied,
        hard_compress_applied,
        phase_telemetry: LengthPhaseTelemetry {
            continuation_count: if continuation_applied { 1 } else { 0 },
            compress_count: if compress_applied { 1 } else { 0 },
            hard_compress_count: if hard_compress_applied { 1 } else { 0 },
            continuation_latency_ms,
            compress_latency_ms,
            hard_compress_latency_ms,
        },
        warning: if saved.output_chars < context.chapter_contract.min_chars
            || saved.output_chars > context.chapter_contract.max_chars
        {
            Some("saved output required hard-bound save success but remained outside preferred model-output band".to_string())
        } else {
            None
        },
    };
    let artifact_refs =
        match persist_chapter_runtime_artifacts(
            &config.project,
            &request_id,
            &context,
            &settlement_delta,
            &length_telemetry,
            &draft.content,
        ) {
            Ok(artifacts) => {
                // Also persist quality report alongside other artifacts
                let runtime_dir = config
                    .project
                    .project_data_dir()
                    .join("chapter_runtime");
                let stem = format!(
                    "{}-{}",
                    context.target.title,
                    request_id
                        .chars()
                        .filter(|c| c.is_alphanumeric() || *c == '-')
                        .collect::<String>()
                );
                // Write before/after quality reports
                let before_path =
                    runtime_dir.join(format!("{}.quality_report.before.json", stem));
                let after_path =
                    runtime_dir.join(format!("{}.quality_report.after.json", stem));
                let mut refs = artifacts.artifact_refs;
                write_runtime_artifact(
                    &before_path,
                    &quality_report_before,
                    format!("chapter_runtime/{}.quality_report.before.json", stem),
                    &mut refs,
                    &mut warnings,
                );
                write_runtime_artifact(
                    &after_path,
                    &final_quality,
                    format!("chapter_runtime/{}.quality_report.after.json", stem),
                    &mut refs,
                    &mut warnings,
                );
                // Write context quality report
                if let Some(ref cq) = context.context_quality {
                    let cq_path = runtime_dir.join(format!("{}.context_quality.json", stem));
                    write_runtime_artifact(
                        &cq_path,
                        cq,
                        format!("chapter_runtime/{}.context_quality.json", stem),
                        &mut refs,
                        &mut warnings,
                    );
                }
                // Write revision report
                let rev_path = runtime_dir.join(format!("{}.revision_report.json", stem));
                write_runtime_artifact(
                    &rev_path,
                    &revision_report,
                    format!("chapter_runtime/{}.revision_report.json", stem),
                    &mut refs,
                    &mut warnings,
                );
                Some(refs)
            }
            Err(error) => {
                warnings.push(format!("Runtime artifacts skipped: {}", error));
                None
            }
        };

    emit(ChapterGenerationEvent {
        request_id: request_id.clone(),
        phase: PHASE_COMPLETED.to_string(),
        detail: Some("生成完成".to_string()),
        status: "done".to_string(),
        message: format!("{} 初稿已保存。", saved.chapter_title),
        progress: 100,
        target_chapter_title: Some(saved.chapter_title.clone()),
        sources: None,
        budget: None,
        receipt: None,
        intent_artifact: Some(context.intent_artifact.clone()),
        selected_evidence: Some(context.selected_evidence.clone()),
        rule_stack: Some(context.rule_stack.clone()),
        trace_artifact: Some(context.trace_artifact.clone()),
        scene_plan: Some(context.scene_plan.clone()),
        settlement_delta: Some(settlement_delta.clone()),
        settlement_apply,
        length_telemetry: Some(length_telemetry),
        artifact_refs,
        saved: Some(saved.clone()),
        chapter_contract: Some(context.chapter_contract.clone()),
        output_chars: Some(saved.output_chars),
        conflict: None,
        error: None,
            generation_strategy: Some(context.generation_strategy.clone()),
            quality_report: Some(final_quality.clone()),
            timing: Some(timing),
            quality_mode: Some(quality_mode),
            warnings,
    });

    emit(ChapterGenerationEvent::progress_with_detail(
        &request_id,
        "end",
        "管道完成",
        "done",
        "生成管道已结束",
        100,
        Some(saved.chapter_title.clone()),
        Some(quality_mode),
    ));

    PipelineTerminal::Completed {
        saved,
        generated_content: draft.content,
        settlement_delta: Box::new(settlement_delta),
        quality_report: Some(final_quality),
    }
}

fn build_chapter_settlement_delta<P: ChapterGenerationProject>(
    config: &ChapterGenerationConfig<P>,
    context: &BuiltChapterContext,
    generated_content: &str,
    saved: &SaveGeneratedChapterOutput,
) -> Result<ChapterSettlementDelta, String> {
    let memory = crate::writer_agent::memory::WriterMemory::open(&config.memory_path)
        .map_err(|e| e.to_string())?;
    Ok(build_basic_chapter_settlement_delta(
        &config.project_id,
        &saved.chapter_title,
        &saved.new_revision,
        generated_content,
        crate::agent_runtime::now_ms(),
        &memory,
        context
            .warnings
            .iter()
            .filter(|warning| !warning.trim().is_empty())
            .cloned()
            .collect(),
        ))
}

fn record_craft_memory_feedback<P: ChapterGenerationProject>(
    config: &ChapterGenerationConfig<P>,
    context: &BuiltChapterContext,
    before: &ChapterQualityReport,
    after: &ChapterQualityReport,
    target_changes: &[RevisionTargetChange],
    had_issues: bool,
    revision_attempted: bool,
) -> Vec<CraftMemoryUpdate> {
    let Some(conn) = config.project.open_memory_db() else {
        return Vec::new();
    };
    let scope = context.target.title.clone();
    let selected_rule_ids: Vec<String> = context
        .craft_plan
        .as_ref()
        .map(|plan| plan.selected_craft_rules.clone())
        .unwrap_or_default();
    if selected_rule_ids.is_empty() {
        return Vec::new();
    }

    let mut updates = Vec::new();
    for rule_id in selected_rule_ids {
        let Some(rule) = craft_library_for_stats()
            .iter()
            .find(|candidate| candidate.id == rule_id)
        else {
            continue;
        };
        let matched_metrics = matched_quality_metrics_for_rule(rule, before);
        if matched_metrics.is_empty() {
            continue;
        }
        let score_before = average_metric_score(before, &matched_metrics);
        let score_after = average_metric_score(after, &matched_metrics);
        let severe_before = matched_metrics.iter().any(|metric| {
            before.metric_results.iter().any(|result| {
                result.metric == *metric
                    && (result.severity == IssueSeverity::Major
                        || result.severity == IssueSeverity::Fatal)
            })
        });
        let severe_after = matched_metrics.iter().any(|metric| {
            after.metric_results.iter().any(|result| {
                result.metric == *metric
                    && (result.severity == IssueSeverity::Major
                        || result.severity == IssueSeverity::Fatal)
            })
        });
        let delta = score_after - score_before;
        let decision = if delta > 0.01 || (!severe_after && (severe_before || score_after >= 0.8)) {
            "accepted"
        } else if had_issues
            && (delta < -0.01
                || severe_after
                || (revision_attempted && target_intersects_metrics(target_changes, &matched_metrics)))
        {
            "rejected"
        } else {
            continue;
        };

        if decision == "accepted" {
            let _ = crate::writer_agent::memory::record_craft_accept(&conn, &rule_id, &scope);
        } else {
            let _ = crate::writer_agent::memory::record_craft_reject(&conn, &rule_id, &scope);
        }

        let evidence_ref = format!(
            "revision_report:{}:{}",
            scope,
            matched_metrics.join("+")
        );
        let reason = craft_memory_feedback_reason(
            decision,
            score_before,
            score_after,
            severe_before,
            severe_after,
        );
        let event = crate::writer_agent::memory::CraftFeedbackEvent {
            rule_id: rule_id.clone(),
            scope: scope.clone(),
            action: decision.to_string(),
            matched_metrics: matched_metrics.clone(),
            score_before,
            score_after,
            evidence_ref: evidence_ref.clone(),
            reason: reason.clone(),
        };
        let _ = crate::writer_agent::memory::record_craft_feedback_event(&conn, &event);
        let (example_refs, bad_pattern_refs) = record_craft_pattern_memory(
            &conn,
            &rule_id,
            &scope,
            decision,
            &matched_metrics,
            target_changes,
            delta,
            &evidence_ref,
            &reason,
        );
        updates.push(CraftMemoryUpdate {
            rule_id,
            scope: scope.clone(),
            decision: decision.to_string(),
            diagnostic_signals: rule.diagnostic_signals.clone(),
            matched_metrics,
            score_before,
            score_after,
            evidence_ref,
            reason,
            example_refs,
            bad_pattern_refs,
        });
    }

    updates
}

pub fn record_manual_craft_edit_feedback(
    conn: &rusqlite::Connection,
    request: ManualCraftEditFeedbackRequest,
) -> Result<ManualCraftEditFeedbackResult, String> {
    if !request.author_approved {
        return Err("manual craft edit feedback requires explicit author approval".to_string());
    }
    if request.before_text.trim().is_empty() || request.after_text.trim().is_empty() {
        return Err("beforeText and afterText are required".to_string());
    }
    if request.before_text == request.after_text {
        return Err("beforeText and afterText must differ".to_string());
    }

    crate::writer_agent::memory::ensure_craft_tables(conn)?;

    let source_ref = request.source_ref.clone().unwrap_or_else(|| {
        format!(
            "manual_edit:{}:{}",
            request.chapter_title,
            crate::agent_runtime::now_ms()
        )
    });
    let quality_signals = ChapterQualitySignals {
        anchor_keywords: request.anchor_keywords.clone(),
        author_voice: request.author_voice.clone(),
        required_anchors: Vec::new(),
        required_state_deltas: Vec::new(),
        prior_chapter_summaries: Vec::new(),
        scene_contract: None,
        world_assets: Vec::new(),
        canon_constraints: Vec::new(),
        canon_terms: Vec::new(),
    };
    let min_chars = request.target_min_chars.unwrap_or(0);
    let max_chars = request.target_max_chars.unwrap_or_else(|| {
        request
            .before_text
            .chars()
            .count()
            .max(request.after_text.chars().count())
            .max(1)
            * 4
    });
    let scene_plan = SceneCraftPlan::default();
    let quality_before = evaluate_chapter_quality_with_signals(
        &request.before_text,
        &request.chapter_title,
        &scene_plan,
        &request.open_promise_keywords,
        min_chars,
        max_chars,
        &quality_signals,
    );
    let quality_after = evaluate_chapter_quality_with_signals(
        &request.after_text,
        &request.chapter_title,
        &scene_plan,
        &request.open_promise_keywords,
        min_chars,
        max_chars,
        &quality_signals,
    );
    let target_changes = build_manual_craft_edit_target_changes(
        &quality_before,
        &quality_after,
        &request.before_text,
        &request.after_text,
        &request.metrics,
    );

    let mut craft_memory_updates = Vec::new();
    for change in &target_changes {
        if !is_observed_manual_change(change) {
            continue;
        }
        let Some(rule) = craft_rule_for_metric(&change.metric) else {
            continue;
        };
        let Some(score_after) = change.score_after else {
            continue;
        };
        let delta = change.delta.unwrap_or(0.0);
        if delta <= 0.01 {
            continue;
        }
        let scope = request.chapter_title.clone();
        let evidence_ref = format!("{}:{}", source_ref, change.metric);
        let reason = format!(
            "Author manual edit improved {} from {:.2} to {:.2}.",
            change.metric, change.score_before, score_after
        );
        let matched_metrics = vec![change.metric.clone()];

        let _ = crate::writer_agent::memory::record_craft_accept(conn, &rule.id, &scope);
        let event = crate::writer_agent::memory::CraftFeedbackEvent {
            rule_id: rule.id.clone(),
            scope: scope.clone(),
            action: "author_manual_edit_accepted".to_string(),
            matched_metrics: matched_metrics.clone(),
            score_before: change.score_before,
            score_after,
            evidence_ref: evidence_ref.clone(),
            reason: reason.clone(),
        };
        let _ = crate::writer_agent::memory::record_craft_feedback_event(conn, &event);

        let (example_refs, bad_pattern_refs) = record_author_manual_edit_pattern_memory(
            conn,
            &rule.id,
            &scope,
            change,
            delta,
            &evidence_ref,
            &reason,
        );
        craft_memory_updates.push(CraftMemoryUpdate {
            rule_id: rule.id.clone(),
            scope,
            decision: "author_manual_edit_accepted".to_string(),
            diagnostic_signals: rule.diagnostic_signals.clone(),
            matched_metrics,
            score_before: change.score_before,
            score_after,
            evidence_ref,
            reason,
            example_refs,
            bad_pattern_refs,
        });
    }

    let example_refs = craft_memory_updates
        .iter()
        .flat_map(|update| update.example_refs.clone())
        .collect();
    let bad_pattern_refs = craft_memory_updates
        .iter()
        .flat_map(|update| update.bad_pattern_refs.clone())
        .collect();

    Ok(ManualCraftEditFeedbackResult {
        chapter_title: request.chapter_title,
        source_ref,
        score_before: quality_before.overall_score,
        score_after: quality_after.overall_score,
        target_changes,
        craft_memory_updates,
        example_refs,
        bad_pattern_refs,
        quality_before,
        quality_after,
    })
}

fn record_craft_pattern_memory(
    conn: &rusqlite::Connection,
    rule_id: &str,
    scope: &str,
    decision: &str,
    matched_metrics: &[String],
    target_changes: &[RevisionTargetChange],
    score_delta: f32,
    evidence_ref: &str,
    reason: &str,
) -> (Vec<String>, Vec<String>) {
    let mut example_refs = Vec::new();
    let mut bad_pattern_refs = Vec::new();
    let Some(change) = best_target_change_for_metrics(target_changes, matched_metrics) else {
        return (example_refs, bad_pattern_refs);
    };
    let now = crate::agent_runtime::now_ms();
    let primary_metric = matched_metrics
        .first()
        .map(String::as_str)
        .unwrap_or(rule_id);
    if decision == "accepted" {
        let excerpt = if !change.changed_excerpt_after.trim().is_empty() {
            change.changed_excerpt_after.clone()
        } else {
            change.evidence_after.clone().unwrap_or_default()
        };
        if excerpt.trim().is_empty() {
            return (example_refs, bad_pattern_refs);
        }
        let id = stable_craft_memory_id("good", rule_id, scope, primary_metric, &excerpt);
        let example = crate::writer_agent::memory::CraftExampleMemory {
            id: id.clone(),
            rule_id: rule_id.to_string(),
            scope: scope.to_string(),
            excerpt_ref: evidence_ref.to_string(),
            excerpt: snippet_for_craft_memory(&excerpt, 260),
            reason: reason.to_string(),
            pattern: primary_metric.to_string(),
            scene_types: vec!["chapter_targeted_revision".to_string()],
            score_delta,
            created_at: now,
        };
        if crate::writer_agent::memory::record_craft_example(conn, &example).is_ok() {
            example_refs.push(format!("craft_examples:{}", id));
        }
    } else if decision == "rejected" {
        let evidence_excerpt = if !change.changed_excerpt_after.trim().is_empty() {
            change.changed_excerpt_after.clone()
        } else if !change.evidence_before.trim().is_empty() {
            change.evidence_before.clone()
        } else {
            change.evidence_after.clone().unwrap_or_default()
        };
        let id = stable_craft_memory_id("bad", rule_id, scope, primary_metric, &evidence_excerpt);
        let pattern = crate::writer_agent::memory::CraftBadPatternMemory {
            id: id.clone(),
            rule_id: rule_id.to_string(),
            scope: scope.to_string(),
            pattern: primary_metric.to_string(),
            evidence_ref: evidence_ref.to_string(),
            evidence_excerpt: snippet_for_craft_memory(&evidence_excerpt, 260),
            correction: change.revision_hint.clone(),
            rejected_count: 1,
            created_at: now,
            updated_at: now,
        };
        if crate::writer_agent::memory::record_craft_bad_pattern(conn, &pattern).is_ok() {
            bad_pattern_refs.push(format!("craft_bad_patterns:{}", id));
        }
    }
    (example_refs, bad_pattern_refs)
}

fn build_manual_craft_edit_target_changes(
    before: &ChapterQualityReport,
    after: &ChapterQualityReport,
    before_text: &str,
    after_text: &str,
    requested_metrics: &[String],
) -> Vec<RevisionTargetChange> {
    let mut changes =
        build_revision_target_changes_with_text(before, Some(after), true, false, Some(before_text), Some(after_text));
    let requested: std::collections::BTreeSet<&str> = requested_metrics
        .iter()
        .map(String::as_str)
        .filter(|metric| !metric.trim().is_empty())
        .collect();
    if !requested.is_empty() {
        changes.retain(|change| requested.contains(change.metric.as_str()));
    }

    for before_metric in &before.metric_results {
        if !requested.is_empty() && !requested.contains(before_metric.metric.as_str()) {
            continue;
        }
        if changes
            .iter()
            .any(|change| change.metric == before_metric.metric)
        {
            continue;
        }
        let Some(after_metric) = after
            .metric_results
            .iter()
            .find(|metric| metric.metric == before_metric.metric)
        else {
            continue;
        };
        let delta = after_metric.score - before_metric.score;
        if delta <= 0.01 {
            continue;
        }
        let changed = changed_text_excerpt_for_manual_edit(
            before_text,
            after_text,
            &before_metric.metric,
        );
        let sentence_changes = crate::chapter_generation::compute_sentence_changes(
            before_text, after_text, &before_metric.metric,
        );
        changes.push(RevisionTargetChange {
            metric: before_metric.metric.clone(),
            revision_hint: before_metric.revision_hint.clone(),
            score_before: before_metric.score,
            score_after: Some(after_metric.score),
            delta: Some(delta),
            status: RevisionTargetChangeStatus::Improved,
            evidence_before: before_metric.evidence_excerpt.clone(),
            evidence_after: Some(after_metric.evidence_excerpt.clone()),
            changed_excerpt_before: changed
                .as_ref()
                .map(|change| change.0.clone())
                .unwrap_or_default(),
            changed_excerpt_after: changed
                .as_ref()
                .map(|change| change.1.clone())
                .unwrap_or_default(),
            text_change_summary: format!(
                "Author manual edit improved {} by {:+.2}.",
                before_metric.metric, delta
            ),
            sentence_changes,
        });
    }
    changes
}

fn record_author_manual_edit_pattern_memory(
    conn: &rusqlite::Connection,
    rule_id: &str,
    scope: &str,
    change: &RevisionTargetChange,
    score_delta: f32,
    evidence_ref: &str,
    reason: &str,
) -> (Vec<String>, Vec<String>) {
    let mut example_refs = Vec::new();
    let mut bad_pattern_refs = Vec::new();
    if change.changed_excerpt_after.trim().is_empty()
        || change.changed_excerpt_before.trim().is_empty()
    {
        return (example_refs, bad_pattern_refs);
    }

    let now = crate::agent_runtime::now_ms();
    let good_id = stable_craft_memory_id(
        "manual-good",
        rule_id,
        scope,
        &change.metric,
        &change.changed_excerpt_after,
    );
    let example = crate::writer_agent::memory::CraftExampleMemory {
        id: good_id.clone(),
        rule_id: rule_id.to_string(),
        scope: scope.to_string(),
        excerpt_ref: evidence_ref.to_string(),
        excerpt: snippet_for_craft_memory(&change.changed_excerpt_after, 260),
        reason: reason.to_string(),
        pattern: change.metric.clone(),
        scene_types: vec!["author_manual_edit".to_string()],
        score_delta,
        created_at: now,
    };
    if crate::writer_agent::memory::record_craft_example(conn, &example).is_ok() {
        example_refs.push(format!("craft_examples:{}", good_id));
    }

    let bad_id = stable_craft_memory_id(
        "manual-bad",
        rule_id,
        scope,
        &change.metric,
        &change.changed_excerpt_before,
    );
    let pattern = crate::writer_agent::memory::CraftBadPatternMemory {
        id: bad_id.clone(),
        rule_id: rule_id.to_string(),
        scope: scope.to_string(),
        pattern: change.metric.clone(),
        evidence_ref: evidence_ref.to_string(),
        evidence_excerpt: snippet_for_craft_memory(&change.changed_excerpt_before, 260),
        correction: if change.revision_hint.trim().is_empty() {
            "Use the author-approved after edit as the correction pattern.".to_string()
        } else {
            change.revision_hint.clone()
        },
        rejected_count: 1,
        created_at: now,
        updated_at: now,
    };
    if crate::writer_agent::memory::record_craft_bad_pattern(conn, &pattern).is_ok() {
        bad_pattern_refs.push(format!("craft_bad_patterns:{}", bad_id));
    }

    (example_refs, bad_pattern_refs)
}

fn craft_rule_for_metric(metric: &str) -> Option<&'static CraftRule> {
    craft_library_for_stats()
        .iter()
        .find(|rule| rule.diagnostic_signals.iter().any(|signal| signal == metric))
}

fn is_observed_manual_change(change: &RevisionTargetChange) -> bool {
    change.status == RevisionTargetChangeStatus::Improved
        && !change.changed_excerpt_before.trim().is_empty()
        && !change.changed_excerpt_after.trim().is_empty()
}

fn changed_text_excerpt_for_manual_edit(
    before_text: &str,
    after_text: &str,
    metric: &str,
) -> Option<(String, String)> {
    let change_report = build_revision_target_changes_with_text(
        &single_metric_report(metric, before_text),
        Some(&single_metric_report(metric, after_text)),
        true,
        false,
        Some(before_text),
        Some(after_text),
    );
    change_report.into_iter().find_map(|change| {
        if change.changed_excerpt_before.trim().is_empty()
            || change.changed_excerpt_after.trim().is_empty()
        {
            None
        } else {
            Some((change.changed_excerpt_before, change.changed_excerpt_after))
        }
    })
}

fn single_metric_report(metric: &str, text: &str) -> ChapterQualityReport {
    let result = QualityMetricResult {
        metric: metric.to_string(),
        score: 0.4,
        severity: IssueSeverity::Major,
        evidence_excerpt: snippet_for_craft_memory(text, 120),
        rule_source: "manual_edit_diff".to_string(),
        reason: "manual edit diff carrier".to_string(),
        revision_hint: "Compare author before/after edit.".to_string(),
    };
    ChapterQualityReport {
        chapter_title: "manual_edit_diff".to_string(),
        overall_score: result.score,
        fatal_issues: Vec::new(),
        major_issues: vec![QualityIssue {
            metric: result.metric.clone(),
            severity: IssueSeverity::Major,
            evidence: result.evidence_excerpt.clone(),
            description: result.reason.clone(),
        }],
        metric_results: vec![result],
        top_revision_targets: vec![metric.to_string()],
        no_fatal_issue: true,
        world_consistency_violations: Vec::new(),
        canon_constraint_violations: Vec::new(),
    }
}

fn best_target_change_for_metrics<'a>(
    target_changes: &'a [RevisionTargetChange],
    matched_metrics: &[String],
) -> Option<&'a RevisionTargetChange> {
    target_changes
        .iter()
        .find(|change| matched_metrics.iter().any(|metric| metric == &change.metric))
        .or_else(|| target_changes.first())
}

fn stable_craft_memory_id(
    kind: &str,
    rule_id: &str,
    scope: &str,
    metric: &str,
    excerpt: &str,
) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    rule_id.hash(&mut hasher);
    scope.hash(&mut hasher);
    metric.hash(&mut hasher);
    excerpt.hash(&mut hasher);
    format!("{}-{}-{}-{:x}", kind, rule_id, metric, hasher.finish())
}

fn snippet_for_craft_memory(text: &str, max_chars: usize) -> String {
    let mut snippet: String = text.trim().chars().take(max_chars).collect();
    if text.trim().chars().count() > max_chars {
        snippet.push_str("...");
    }
    snippet
}

fn matched_quality_metrics_for_rule(
    rule: &CraftRule,
    report: &ChapterQualityReport,
) -> Vec<String> {
    rule.diagnostic_signals
        .iter()
        .filter(|signal| {
            report
                .metric_results
                .iter()
                .any(|metric| metric.metric == **signal)
        })
        .cloned()
        .collect()
}

fn average_metric_score(report: &ChapterQualityReport, metrics: &[String]) -> f32 {
    let scores: Vec<f32> = metrics
        .iter()
        .filter_map(|metric| {
            report
                .metric_results
                .iter()
                .find(|result| result.metric == *metric)
                .map(|result| result.score)
        })
        .collect();
    if scores.is_empty() {
        0.0
    } else {
        scores.iter().sum::<f32>() / scores.len() as f32
    }
}

fn target_intersects_metrics(target_changes: &[RevisionTargetChange], metrics: &[String]) -> bool {
    target_changes
        .iter()
        .any(|change| metrics.iter().any(|metric| metric == &change.metric))
}

fn craft_memory_feedback_reason(
    decision: &str,
    score_before: f32,
    score_after: f32,
    severe_before: bool,
    severe_after: bool,
) -> String {
    let delta = score_after - score_before;
    if decision == "accepted" {
        if delta > 0.01 {
            format!(
                "Matched diagnostic metrics improved from {:.2} to {:.2}.",
                score_before, score_after
            )
        } else if severe_before && !severe_after {
            "Matched diagnostic metrics cleared major/fatal severity.".to_string()
        } else {
            "Matched diagnostic metrics stayed strong without major/fatal severity.".to_string()
        }
    } else if severe_after {
        format!(
            "Matched diagnostic metrics still have major/fatal severity after revision; score {:.2}->{:.2}.",
            score_before, score_after
        )
    } else {
        format!(
            "Matched diagnostic metrics did not improve; score delta {:+.2}.",
            delta
        )
    }
}

fn write_runtime_artifact<T: serde::Serialize>(
    path: &std::path::Path,
    value: &T,
    artifact_ref: String,
    artifact_refs: &mut Vec<String>,
    warnings: &mut Vec<String>,
) {
    match serde_json::to_string_pretty(value) {
        Ok(json) => match std::fs::write(path, json) {
            Ok(()) => artifact_refs.push(artifact_ref),
            Err(error) => warnings.push(format!(
                "Runtime artifact {} skipped: {}",
                path.display(),
                error
            )),
        },
        Err(error) => warnings.push(format!(
            "Runtime artifact {} serialization failed: {}",
            path.display(),
            error
        )),
    }
}

pub fn select_generation_strategy(
    context: &BuiltChapterContext,
    repair_history: usize,
) -> GenerationStrategy {
    let total_chars = context.budget.included_chars;
    if repair_history > 2 {
        return GenerationStrategy::RepairHeavyMode;
    }
    if total_chars < 8_000 && !context.impact_truncated {
        return GenerationStrategy::InteractiveFastDraft;
    }
    if total_chars > 15_000 || context.impact_truncated {
        return GenerationStrategy::BackgroundLongChapter;
    }
    GenerationStrategy::InteractiveSafeDraft
}

impl ChapterGenerationEvent {
    pub fn progress(
        request_id: &str,
        phase: &str,
        status: &str,
        message: &str,
        progress: u8,
        target_chapter_title: Option<String>,
        quality_mode: Option<crate::chapter_generation::GenerationQualityMode>,
    ) -> Self {
        Self {
            request_id: request_id.to_string(),
            phase: phase.to_string(),
            detail: None,
            status: status.to_string(),
            message: message.to_string(),
            progress,
            target_chapter_title,
            sources: None,
            budget: None,
            receipt: None,
            intent_artifact: None,
            selected_evidence: None,
            rule_stack: None,
            trace_artifact: None,
            scene_plan: None,
            settlement_delta: None,
            settlement_apply: None,
            length_telemetry: None,
            artifact_refs: None,
            saved: None,
            chapter_contract: None,
            output_chars: None,
            conflict: None,
            error: None,
            generation_strategy: None,
            quality_report: None,
            timing: None,
            quality_mode,
            warnings: vec![],
        }
    }

    pub fn progress_with_detail(
        request_id: &str,
        phase: &str,
        detail: &str,
        status: &str,
        message: &str,
        progress: u8,
        target_chapter_title: Option<String>,
        quality_mode: Option<crate::chapter_generation::GenerationQualityMode>,
    ) -> Self {
        Self {
            request_id: request_id.to_string(),
            phase: phase.to_string(),
            detail: Some(detail.to_string()),
            status: status.to_string(),
            message: message.to_string(),
            progress,
            target_chapter_title,
            sources: None,
            budget: None,
            receipt: None,
            intent_artifact: None,
            selected_evidence: None,
            rule_stack: None,
            trace_artifact: None,
            scene_plan: None,
            settlement_delta: None,
            settlement_apply: None,
            length_telemetry: None,
            artifact_refs: None,
            saved: None,
            chapter_contract: None,
            output_chars: None,
            conflict: None,
            error: None,
            generation_strategy: None,
            quality_report: None,
            timing: None,
            quality_mode,
            warnings: vec![],
        }
    }

    pub fn failed(
        request_id: &str,
        error: ChapterGenerationError,
        quality_mode: Option<crate::chapter_generation::GenerationQualityMode>,
    ) -> Self {
        Self {
            request_id: request_id.to_string(),
            phase: PHASE_FAILED.to_string(),
            detail: None,
            status: "error".to_string(),
            message: error.message.clone(),
            progress: 100,
            target_chapter_title: None,
            sources: None,
            budget: None,
            receipt: None,
            intent_artifact: None,
            selected_evidence: None,
            rule_stack: None,
            trace_artifact: None,
            scene_plan: None,
            settlement_delta: None,
            settlement_apply: None,
            length_telemetry: None,
            artifact_refs: None,
            saved: None,
            chapter_contract: None,
            output_chars: None,
            conflict: None,
            error: Some(error),
            generation_strategy: None,
            quality_report: None,
            timing: None,
            quality_mode,
            warnings: vec![],
        }
    }

    pub fn conflict(
        request_id: &str,
        conflict: SaveConflict,
        quality_mode: Option<crate::chapter_generation::GenerationQualityMode>,
    ) -> Self {
        Self {
            request_id: request_id.to_string(),
            phase: PHASE_CONFLICT.to_string(),
            detail: None,
            status: "conflict".to_string(),
            message: format!("保存被阻止：{}。", conflict.reason),
            progress: 100,
            target_chapter_title: conflict.open_chapter_title.clone(),
            sources: None,
            budget: None,
            receipt: None,
            intent_artifact: None,
            selected_evidence: None,
            rule_stack: None,
            trace_artifact: None,
            scene_plan: None,
            settlement_delta: None,
            settlement_apply: None,
            length_telemetry: None,
            artifact_refs: None,
            saved: None,
            chapter_contract: None,
            output_chars: None,
            conflict: Some(conflict),
            error: None,
            generation_strategy: None,
            quality_report: None,
            timing: None,
            quality_mode,
            warnings: vec![],
        }
    }
}

fn write_chapter_generation_checkpoint(
    memory_path: &std::path::Path,
    project_id: &str,
    request_id: &str,
    counter: &mut usize,
    step: &str,
    chapter_title: &str,
    budget_spent_micros: u64,
    artifact_refs: &[&str],
) {
    *counter = counter.saturating_add(1);
    let checkpoint_id = format!("{}-cp-{}", request_id, counter);
    let payload = serde_json::json!({
        "chapter_title": chapter_title,
        "request_id": request_id,
        "step": step,
    });
    let checkpoint = crate::writer_agent::supervised_sprint::LongTaskCheckpoint::new(
        &checkpoint_id,
        request_id,
        "chapter_generation",
        step,
        payload,
    )
    .with_budget(budget_spent_micros)
    .with_artifacts(artifact_refs.iter().map(|s| s.to_string()).collect())
    .with_source("pipeline");

    if let Ok(memory) = crate::writer_agent::memory::WriterMemory::open(memory_path) {
        let _ = memory.insert_long_task_checkpoint(project_id, &checkpoint);
    }
}

fn write_agent_checkpoint(
    memory_path: &std::path::Path,
    project_id: &str,
    request_id: &str,
    counter: &mut usize,
    phase: agent_harness_core::execution_plan::CheckpointPhase,
    _chapter_title: &str,
    budget_spent_micros: u64,
    artifact_refs: &[&str],
    resume_policy: agent_harness_core::execution_plan::ResumePolicy,
) {
    write_agent_checkpoint_with_payload(
        memory_path,
        project_id,
        request_id,
        counter,
        phase,
        _chapter_title,
        budget_spent_micros,
        artifact_refs,
        resume_policy,
        vec![],
        None,
    );
}

#[allow(clippy::too_many_arguments)]
fn write_agent_checkpoint_with_payload(
    memory_path: &std::path::Path,
    project_id: &str,
    request_id: &str,
    counter: &mut usize,
    phase: agent_harness_core::execution_plan::CheckpointPhase,
    _chapter_title: &str,
    budget_spent_micros: u64,
    artifact_refs: &[&str],
    resume_policy: agent_harness_core::execution_plan::ResumePolicy,
    approval_refs: Vec<String>,
    safe_resume_payload: Option<serde_json::Value>,
) {
    *counter = counter.saturating_add(1);
    let checkpoint_id = format!("{}-agent-cp-{}", request_id, counter);
    let checkpoint = agent_harness_core::execution_plan::AgentCheckpoint {
        checkpoint_id,
        task_id: request_id.to_string(),
        plan_id: format!("chapter-generation-{}", request_id),
        step_id: format!("{:?}", phase),
        phase,
        input_hash: String::new(),
        context_hash: String::new(),
        artifact_refs: artifact_refs.iter().map(|s| s.to_string()).collect(),
        tool_effects: vec![],
        provider_usage: None,
        budget_spent: budget_spent_micros,
        approval_refs,
        resume_policy,
        task_kind: Some("chapter_generation".to_string()),
        safe_resume_payload,
        source: Some("pipeline".to_string()),
        created_at_ms: Some(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
        ),
    };

    if let Ok(memory) = crate::writer_agent::memory::WriterMemory::open(memory_path) {
        let _ = memory.insert_agent_checkpoint(project_id, &checkpoint);
    }
}

fn load_world_assets_for_project<P: ChapterGenerationProject>(
    project: &P,
    _project_id: &str,
) -> Vec<crate::writer_agent::world_bible::WorldAsset> {
    let data_dir = project.project_data_dir();

    // P14: Prefer world_bible_index.json (new TypedWorldAsset system)
    let bible_index_path = data_dir.join("world_bible_index.json");
    if bible_index_path.exists() {
        let text = match std::fs::read_to_string(&bible_index_path) {
            Ok(t) => t,
            Err(_) => return Vec::new(),
        };
        match serde_json::from_str::<crate::writer_agent::world_bible::WorldBibleIndex>(&text) {
            Ok(index) => {
                return index
                    .assets
                    .iter()
                    .map(|a| a.to_world_asset())
                    .collect();
            }
            Err(_) => {
                // Fall through to legacy world_assets.json or empty
            }
        }
    }

    // Backward compat: load legacy world_assets.json
    let path = data_dir.join("world_assets.json");
    if !path.exists() {
        return Vec::new();
    }
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(_) => return Vec::new(),
    };
    serde_json::from_str::<Vec<crate::writer_agent::world_bible::WorldAsset>>(&text)
        .unwrap_or_default()
}

fn make_draft_title(target_title: &str, request_id: &str) -> String {
    let suffix = request_id
        .chars()
        .rev()
        .take(6)
        .collect::<String>()
        .chars()
        .rev()
        .collect::<String>();
    format!("{} draft {}", target_title, suffix)
}

pub fn make_request_id(prefix: &str) -> String {
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default();
    format!("{}-{}", prefix, millis)
}

pub fn map_provider_error(error: String) -> ChapterGenerationError {
    let lower = error.to_lowercase();
    if lower.contains("timeout") || lower.contains("timed out") {
        ChapterGenerationError::with_details(
            "PROVIDER_TIMEOUT",
            "The model provider timed out.",
            true,
            error,
        )
    } else if lower.contains("429") || lower.contains("rate limit") {
        ChapterGenerationError::with_details(
            "PROVIDER_RATE_LIMITED",
            "The model provider rate-limited the request.",
            true,
            error,
        )
    } else if lower.contains("api key") || lower.contains("unauthorized") || lower.contains("401") {
        ChapterGenerationError::with_details(
            "PROVIDER_NOT_CONFIGURED",
            "The model provider is not configured.",
            true,
            error,
        )
    } else {
        ChapterGenerationError::with_details(
            "PROVIDER_CALL_FAILED",
            "The model provider call failed.",
            true,
            error,
        )
    }
}
