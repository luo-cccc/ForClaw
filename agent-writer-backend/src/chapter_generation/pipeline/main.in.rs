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

    emit(ChapterGenerationEvent::progress_with_detail(
        &request_id,
        "start",
        "管道启动",
        "running",
        "生成管道已启动",
        0,
        None,
    ));

    emit(ChapterGenerationEvent::progress(
        &request_id,
        PHASE_STARTED,
        "running",
        "正在理解任务并读取工程结构...",
        5,
        None,
    ));

    let memory = crate::writer_agent::memory::WriterMemory::open(&config.memory_path).ok();
    let open_promise_count = memory
        .as_ref()
        .and_then(|m| m.get_open_promises().ok())
        .map(|p| p.len())
        .unwrap_or(0);
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

    let mut context = match build_chapter_context(&config.project, build_input).await {
        Ok(context) => context,
        Err(error) => {
            emit(ChapterGenerationEvent::failed(&request_id, error.clone()));
            return PipelineTerminal::Failed(error);
        }
    };

    // Preflight: select generation strategy based on context size and risk.
    let strategy = select_generation_strategy(&context, 0);
    context.generation_strategy = strategy.clone();

    record_task_packet(&context);

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
    ));

    emit(ChapterGenerationEvent::progress_with_detail(
        &request_id,
        PHASE_SEGMENT_DRAFT,
        "正在写第一段",
        "running",
        "正在撰写章节初稿...",
        45,
        Some(context.target.title.clone()),
    ));

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
            draft
        }
        Err(error) => {
            if let Some(report) = provider_budget_report_from_error(&error) {
                record_provider_budget(&context, &report);
            }
            emit(ChapterGenerationEvent::failed(&request_id, error.clone()));
            return PipelineTerminal::Failed(error);
        }
    };
    let draft_chars_before_repairs = draft.output_chars;
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
            ));
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
                    emit(ChapterGenerationEvent::failed(&request_id, error.clone()));
                    return PipelineTerminal::Failed(error);
                }
            };
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
        }
        ChapterContractOutcome::OverMaxChars => {
            emit(ChapterGenerationEvent::progress(
                &request_id,
                PHASE_COMPRESS,
                "running",
                "初稿字数超出目标区间，正在压缩正文...",
                55,
                Some(context.target.title.clone()),
            ));
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
                    emit(ChapterGenerationEvent::failed(&request_id, error.clone()));
                    return PipelineTerminal::Failed(error);
                }
            };
            if !compressed.content.is_empty() {
                draft.content = compressed.content.trim().to_string();
                draft.output_chars = char_count(&draft.content);
                compress_applied = true;
                compress_latency_ms = compress_t0.elapsed().as_millis() as u64;
            }
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
        ));
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
                emit(ChapterGenerationEvent::failed(&request_id, error.clone()));
                return PipelineTerminal::Failed(error);
            }
        };
        if !compressed.content.is_empty() {
            draft.content = compressed.content.trim().to_string();
            draft.output_chars = char_count(&draft.content);
            hard_compress_applied = true;
            hard_compress_latency_ms = hard_compress_t0.elapsed().as_millis() as u64;
        }
    }

    emit(ChapterGenerationEvent::progress_with_detail(
        &request_id,
        PHASE_LENGTH_VALIDATE,
        "正在校验长度",
        "running",
        "正在校验章节长度约束...",
        63,
        Some(context.target.title.clone()),
    ));

    if let Err(error) = validate_generated_content(
        &draft.content,
        &context.chapter_contract,
        ChapterContractPhase::ModelOutput,
    ) {
        emit(ChapterGenerationEvent::failed(&request_id, error.clone()));
        return PipelineTerminal::Failed(error);
    }

    // Quality evaluation: always evaluate draft quality after length repairs.
    let scene_craft_plan = context
        .craft_plan
        .as_ref()
        .cloned()
        .unwrap_or_default();
    let quality_signals = ChapterQualitySignals {
        anchor_keywords: context.quality_anchor_keywords.clone(),
        author_voice: context.author_voice_snapshot.clone(),
    };
    let quality_report_before = evaluate_chapter_quality_with_signals(
        &draft.content,
        &context.target.title,
        &scene_craft_plan,
        &[],
        context.chapter_contract.min_chars,
        context.chapter_contract.max_chars,
        &quality_signals,
    );

    // Targeted revision: if quality report has major/fatal issues, attempt
    // a single revision pass with the ChapterTargetedRevision profile.
    
    let mut quality_report_after_revision: Option<ChapterQualityReport> = None;
    let mut quality_report_after_attempt: Option<ChapterQualityReport> = None;
    let draft_before_revision = draft.content.clone();
    let mut revised_text_attempt: Option<String> = None;
    let mut revision_budget_skipped = false;
    let mut revision_attempted = false;
    if !quality_report_before.fatal_issues.is_empty() || !quality_report_before.major_issues.is_empty() {
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
                let revision_result = crate::llm_runtime::chat_text_profile(
                    &config.settings,
                    revision_messages,
                    crate::llm_runtime::LlmRequestProfile::ChapterTargetedRevision,
                    300,
                )
                .await;
                if let Ok(revised) = revision_result {
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

    emit(ChapterGenerationEvent::progress_with_detail(
        &request_id,
        PHASE_SAVE,
        "正在保存",
        "running",
        "正在保存章节并检查编辑器冲突...",
        70,
        Some(context.target.title.clone()),
    ));

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
    let saved = match save_generated_chapter(&config.project, save_input) {
        Ok(saved) => saved,
        Err(error) => {
            if let Some(conflict) = save_conflict_from_error(&error) {
                emit(ChapterGenerationEvent::conflict(
                    &request_id,
                    conflict.clone(),
                ));
                return PipelineTerminal::Conflict(conflict);
            }
            emit(ChapterGenerationEvent::failed(&request_id, error.clone()));
            return PipelineTerminal::Failed(error);
        }
    };

    emit(ChapterGenerationEvent::progress_with_detail(
        &request_id,
        PHASE_POLISH,
        "正在润色",
        "running",
        "正在更新大纲状态...",
        85,
        Some(saved.chapter_title.clone()),
    ));

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
            warnings: vec![],
        }
    }

    pub fn failed(request_id: &str, error: ChapterGenerationError) -> Self {
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
            warnings: vec![],
        }
    }

    pub fn conflict(request_id: &str, conflict: SaveConflict) -> Self {
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
            warnings: vec![],
        }
    }
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
