use super::*;
use agent_harness_core::execution_plan::{compile_plan, ExecutionPlan};
use agent_harness_core::task_packet::TaskPacket;

impl WriterAgentKernel {
    pub fn record_manual_exchange(
        &mut self,
        observation: &WriterObservation,
        message: &str,
        response: &str,
        source_refs: &[String],
    ) -> Result<(), String> {
        let scope = observation
            .chapter_title
            .as_deref()
            .unwrap_or("manual request");
        let title = format!("ManualRequest: {}", snippet(message, 48));
        let rationale = format!(
            "用户显式请求: {}\nAgent回应摘要: {}",
            snippet(message, 160),
            snippet(response, 240)
        );
        self.memory
            .record_decision(scope, &title, "answered", &[], &rationale, source_refs)
            .map_err(|e| e.to_string())?;
        self.memory
            .record_manual_agent_turn(&ManualAgentTurnSummary {
                project_id: observation.project_id.clone(),
                observation_id: observation.id.clone(),
                chapter_title: observation.chapter_title.clone(),
                user: message.to_string(),
                assistant: response.to_string(),
                source_refs: source_refs.to_vec(),
                created_at: crate::agent_runtime::now_ms(),
            })
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Run preflight checks without executing provider or tools.
    /// Returns a structured readiness report (ready / warning / blocked).
    /// Mirrors the first half of prepare_task_run — same gates, no AgentLoop.
    pub fn preflight(
        &mut self,
        request: &WriterAgentRunRequest,
    ) -> crate::writer_agent::run_preflight::WriterRunPreflightReport {
        use crate::writer_agent::run_preflight::{
            WriterContextQualitySummary, WriterRunPreflightReport,
        };
        let task = request.task.as_agent_task();
        let observation = &request.observation;
        let mut blocks: Vec<crate::writer_agent::run_preflight::PreflightItem> = Vec::new();
        let mut warnings: Vec<crate::writer_agent::run_preflight::PreflightItem> = Vec::new();
        let mut next_actions: Vec<String> = Vec::new();

        // Metacognitive gate
        if crate::writer_agent::metacognition::metacognitive_task_is_write_sensitive(&request.task)
        {
            let meta = self.trace_snapshot(40).metacognitive_snapshot;
            if let Some(reason) =
                crate::writer_agent::metacognition::metacognitive_write_gate_reason(&meta)
            {
                blocks.push(crate::writer_agent::run_preflight::PreflightItem {
                    code: "metacognitive_blocked".to_string(),
                    reason,
                });
                next_actions.push("Review metacognitive snapshot; run ContinuityDiagnostic or PlanningReview recovery".to_string());
            }
        }

        // Context pack → Story Impact. Keep preflight read-only: do not call
        // observe(), which records observations, proposals, and save diagnostics.
        let context_pack = self.context_pack_for_default(task.clone(), &request.observation);
        let (impact_radius, impact_budget) =
            crate::writer_agent::story_impact::compute_story_impact(
                &request.observation,
                &context_pack,
                &self.memory,
                None,
            );

        if context_pack.total_chars > context_pack.budget_limit && context_pack.budget_limit > 0 {
            warnings.push(crate::writer_agent::run_preflight::PreflightItem {
                code: "context_over_budget".to_string(),
                reason: format!(
                    "Context pack {} chars exceeds budget {} chars",
                    context_pack.total_chars, context_pack.budget_limit
                ),
            });
        }

        if impact_radius.truncated {
            warnings.push(crate::writer_agent::run_preflight::PreflightItem {
                code: "story_impact_truncated".to_string(),
                reason: format!(
                    "Story Impact truncated {} nodes; {} high-risk dropped",
                    impact_budget.truncated_node_count,
                    impact_budget.dropped_high_risk_sources.len()
                ),
            });
            if !impact_budget.dropped_high_risk_sources.is_empty() {
                next_actions.push(
                    "Review dropped high-risk story sources; consider expanding context budget"
                        .to_string(),
                );
            }
        }

        let required_context_sources = required_context_sources_for_preflight(task.clone());
        let packed_context = agent_harness_core::PackedContext {
            text: context_pack
                .sources
                .iter()
                .map(|source| source.content.as_str())
                .collect::<Vec<_>>()
                .join("\n\n"),
            sources: context_pack
                .sources
                .iter()
                .map(|source| agent_harness_core::ContextSourceReport {
                    source_type: context_quality_source_type(&source.source).to_string(),
                    id: source
                        .evidence_ref
                        .clone()
                        .unwrap_or_else(|| format!("{:?}", source.source)),
                    label: format!("{:?}", source.source),
                    original_chars: source.char_count,
                    included_chars: source.char_count,
                    truncated: source.truncated,
                    score: None,
                    taxonomy: taxonomy_for_source(&source.source).to_string(),
                    role: String::new(),
                    elapsed_ms: 0,
                    retrieval_status: "sync".to_string(),
                })
                .collect(),
            budget: agent_harness_core::ContextBudgetReport {
                max_chars: context_pack.budget_limit,
                included_chars: context_pack.total_chars,
                source_count: context_pack.sources.len(),
                truncated_source_count: context_pack
                    .sources
                    .iter()
                    .filter(|source| source.truncated)
                    .count(),
                warnings: context_pack
                    .budget_report
                    .source_reports
                    .iter()
                    .filter(|report| report.truncated)
                    .map(|report| {
                        format!(
                            "{} truncated: {}",
                            report.source,
                            report
                                .truncation_reason
                                .as_deref()
                                .unwrap_or("budget limited")
                        )
                    })
                    .collect(),
            },
        };
        let context_quality = agent_harness_core::evaluate_context_quality(
            &observation.id,
            &packed_context,
            &required_context_sources,
        );
        match &context_quality.recommendation {
            agent_harness_core::ContextQualityRecommendation::Critical { reason } => {
                blocks.push(crate::writer_agent::run_preflight::PreflightItem {
                    code: "context_quality_critical".to_string(),
                    reason: reason.clone(),
                });
                next_actions.push(
                    "Add missing story evidence before running this write-sensitive task."
                        .to_string(),
                );
            }
            agent_harness_core::ContextQualityRecommendation::Supplement { sources, actions } => {
                warnings.push(crate::writer_agent::run_preflight::PreflightItem {
                    code: "context_quality_supplement".to_string(),
                    reason: format!(
                        "Context quality suggests supplementing sources: {}",
                        if sources.is_empty() {
                            "review truncated or weak grounding sources".to_string()
                        } else {
                            sources.join(", ")
                        }
                    ),
                });
                next_actions.push(
                    "Supplement missing context sources or accept lower grounding confidence."
                        .to_string(),
                );
                for action in actions {
                    next_actions.push(format!("Action: {}", action));
                }
            }
            agent_harness_core::ContextQualityRecommendation::Sufficient => {}
        }
        for warning in &context_quality.warnings {
            warnings.push(crate::writer_agent::run_preflight::PreflightItem {
                code: "context_quality_warning".to_string(),
                reason: warning.clone(),
            });
        }

        // Task packet validation
        let task_packet = build_task_packet_for_observation(
            &self.project_id,
            &self.session_id,
            task.clone(),
            &request.observation,
            &context_pack,
            &objective_for_run_task(&request.task),
            success_criteria_for_run_task(&request.task),
        );
        if let Err(err) = task_packet.validate() {
            blocks.push(crate::writer_agent::run_preflight::PreflightItem {
                code: "task_packet_invalid".to_string(),
                reason: format!("TaskPacket validation failed: {}", err),
            });
            next_actions.push("Fix task configuration before retrying".to_string());
        }

        // Story Contract quality
        let (contract_quality, _gaps) = self.contract_quality_with_gaps();
        if task_requires_story_grounding(&request.task)
            && contract_quality <= StoryContractQuality::Vague
        {
            warnings.push(crate::writer_agent::run_preflight::PreflightItem {
                code: "story_contract_weak".to_string(),
                reason: format!(
                    "Story Contract quality is {:?}: task may lack story-level grounding",
                    contract_quality
                ),
            });
            next_actions
                .push("Strengthen the Story Contract in Settings before running".to_string());
        }

        // Tool inventory
        let tool_filter = tool_filter_for_run_request(task.clone(), &request.approval_mode);
        let registry = default_writing_tool_registry();
        let inventory = registry.effective_inventory(
            &tool_filter,
            &PermissionPolicy::new(PermissionMode::WorkspaceWrite),
        );

        // Token estimate
        let estimated_input = (context_pack.total_chars as u64).saturating_div(3) + 100;
        let estimated_total = estimated_input + 2_048;
        if estimated_total > 64_000 {
            blocks.push(crate::writer_agent::run_preflight::PreflightItem {
                code: "provider_budget_blocked".to_string(),
                reason: format!("Estimated {} tokens exceeds hard limit", estimated_total),
            });
            next_actions.push("Reduce scope or increase budget before running".to_string());
        } else if estimated_total > 32_000 {
            warnings.push(crate::writer_agent::run_preflight::PreflightItem {
                code: "provider_budget_approval".to_string(),
                reason: format!(
                    "Estimated {} tokens requires author approval",
                    estimated_total
                ),
            });
            next_actions.push("Approve provider budget in Explore before running".to_string());
        }

        // Readiness verdict
        let readiness = if !blocks.is_empty() {
            "blocked"
        } else if !warnings.is_empty() {
            "warning"
        } else {
            "ready"
        };
        if readiness == "ready" {
            next_actions.push("Task is ready to run.".to_string());
        } else if readiness == "blocked" {
            next_actions.push("Resolve blocks before this task can run.".to_string());
        } else {
            next_actions.push("Review warnings; task can still proceed.".to_string());
        }

        let provider_budget_decision = if estimated_total > 64_000 {
            "blocked"
        } else if estimated_total > 32_000 {
            "approval_required"
        } else {
            "allowed"
        };
        let source_refs: Vec<String> = context_pack
            .sources
            .iter()
            .map(|s| format!("{:?}", s.source))
            .collect();

        WriterRunPreflightReport {
            task: format!("{:?}", request.task),
            observation_id: observation.id.clone(),
            readiness: readiness.to_string(),
            blocks,
            warnings,
            context_source_count: context_pack.sources.len(),
            context_total_chars: context_pack.total_chars,
            context_budget_limit: context_pack.budget_limit,
            story_impact_truncated: impact_radius.truncated,
            story_impact_risk: format!("{:?}", impact_radius.risk),
            story_contract_quality: format!("{:?}", contract_quality),
            tool_allowed_count: inventory.allowed.len(),
            tool_blocked_count: inventory.blocked.len(),
            estimated_input_tokens: estimated_input,
            estimated_output_tokens: 2048,
            provider_budget_decision: provider_budget_decision.to_string(),
            task_packet_objective: task_packet.objective.clone(),
            source_refs,
            next_actions,
            context_quality: Some(WriterContextQualitySummary {
                overall_score: context_quality.overall_score,
                source_coverage: context_quality.source_coverage,
                truncation_risk: context_quality.truncation_risk,
                grounding_quality: context_quality.grounding_quality,
                missing_evidence: context_quality.missing_evidence.clone(),
                recommendation: context_quality_recommendation_label(
                    &context_quality.recommendation,
                )
                .to_string(),
            }),
        }
    }

    pub fn prepare_task_run<P, H>(
        &mut self,
        request: WriterAgentRunRequest,
        provider: Arc<P>,
        handler: H,
        model: &str,
    ) -> Result<WriterAgentPreparedRun<P, H>, String>
    where
        P: Provider + 'static,
        H: ToolHandler + 'static,
    {
        let task = request.task.as_agent_task();
        if crate::writer_agent::metacognition::metacognitive_task_is_write_sensitive(&request.task)
        {
            let meta = self.trace_snapshot(40).metacognitive_snapshot;
            if let Some(reason) =
                crate::writer_agent::metacognition::metacognitive_write_gate_reason(&meta)
            {
                self.record_metacognitive_gate_block_run_event(
                    &request.task,
                    request.observation.id.clone(),
                    &reason,
                    &meta,
                    now_ms(),
                );
                return Err(reason);
            }
        }
        let proposals = self.observe(request.observation.clone())?;
        let operations = proposals
            .iter()
            .flat_map(|proposal| proposal.operations.clone())
            .collect::<Vec<_>>();
        let mut context_pack = self.context_pack_for_default(task.clone(), &request.observation);
        let (impact_radius, impact_budget) =
            crate::writer_agent::story_impact::compute_story_impact(
                &request.observation,
                &context_pack,
                &self.memory,
                None,
            );
        let impact_summary = crate::writer_agent::story_impact::story_impact_context_summary(
            &impact_radius,
            &impact_budget,
        );
        append_context_source_with_budget(
            &mut context_pack,
            ContextSource::StoryImpactRadius,
            impact_summary,
            story_impact_context_budget(&task),
            story_impact_context_priority(&task),
            Some(format!("story_impact_radius:{}", request.observation.id)),
        );
        self.record_context_pack_built_run_event(&request.observation, &context_pack, now_ms());
        self.record_context_spine_run_events(&request.observation, &context_pack, now_ms());
        self.record_story_impact_radius_run_event(
            &request.observation.id,
            &impact_radius,
            &impact_budget,
            now_ms(),
        );
        let mut task_packet = build_task_packet_for_observation(
            &self.project_id,
            &self.session_id,
            task.clone(),
            &request.observation,
            &context_pack,
            &objective_for_run_task(&request.task),
            success_criteria_for_run_task(&request.task),
        );
        attach_story_impact_to_task_packet(&mut task_packet, &impact_radius, &impact_budget);
        let (contract_quality, contract_quality_gaps) = self.contract_quality_with_gaps();
        attach_story_contract_quality_gate_to_task_packet(
            &mut task_packet,
            &task,
            contract_quality,
            &contract_quality_gaps,
        );
        task_packet.validate().map_err(|error| error.to_string())?;
        self.push_task_packet_trace(
            request.observation.id.clone(),
            format!("{:?}", task),
            task_packet.clone(),
        );
        let task_receipt = (request.task == WriterAgentTask::ContinuityDiagnostic
            || request.task == WriterAgentTask::PlanningReview)
            .then(|| {
                if request.task == WriterAgentTask::PlanningReview {
                    crate::writer_agent::task_receipt::build_planning_review_receipt(
                        task_packet.id.clone(),
                        &request.observation,
                        &task_packet.objective,
                        &context_pack,
                        now_ms(),
                    )
                } else {
                    crate::writer_agent::task_receipt::build_continuity_diagnostic_receipt(
                        task_packet.id.clone(),
                        &request.observation,
                        &task_packet.objective,
                        &context_pack,
                        now_ms(),
                    )
                }
            });
        if let Some(receipt) = task_receipt.as_ref() {
            self.record_task_receipt_run_event(receipt);
        }

        let tool_filter = tool_filter_for_run_request(task.clone(), &request.approval_mode);
        let registry = default_writing_tool_registry();
        let tool_inventory = registry.effective_inventory(
            &tool_filter,
            &PermissionPolicy::new(PermissionMode::WorkspaceWrite),
        );
        let source_refs = source_refs_from_context_pack(&context_pack);
        let context_pack_summary = WriterAgentContextPackSummary {
            task: task.clone(),
            source_count: context_pack.sources.len(),
            total_chars: context_pack.total_chars,
            budget_limit: context_pack.budget_limit,
            source_refs: source_refs.clone(),
        };
        let system_prompt = render_run_system_prompt(&request, &context_pack, self);
        tracing::debug!(
            "WriterAgent {:?} ContextPack: {} sources, {}/{} chars",
            task,
            context_pack.sources.len(),
            context_pack.total_chars,
            context_pack.budget_limit
        );

        let mut agent = AgentLoop::new(
            AgentLoopConfig {
                max_rounds: 10,
                system_prompt,
                context_limit_tokens: Some(
                    agent_harness_core::resolve_context_window_info(model).tokens,
                ),
                tool_filter: Some(tool_filter),
            },
            provider,
            registry,
            handler,
        );
        agent.messages.extend(request.manual_history.clone());

        Ok(WriterAgentPreparedRun {
            request,
            agent,
            proposals,
            operations,
            task_packet,
            task_receipt,
            context_pack_summary,
            tool_inventory,
            source_refs,
            trace_refs: vec![],
        })
    }

    pub async fn run_task<P, H>(
        &mut self,
        request: WriterAgentRunRequest,
        provider: Arc<P>,
        handler: H,
        model: &str,
        on_event: Option<EventCallback>,
    ) -> Result<WriterAgentRunResult, String>
    where
        P: Provider + 'static,
        H: ToolHandler + 'static,
    {
        let completion_request = request.clone();
        let mut prepared = self.prepare_task_run(request, provider, handler, model)?;
        if let Some(callback) = on_event {
            prepared.set_event_callback(callback);
        }
        let result = prepared.run().await?;
        self.record_run_completion(&completion_request, &result)?;
        Ok(result)
    }

    pub(crate) fn compile_execution_plan(
        task: &TaskPacket,
        run_id: &str,
        now_ms: u64,
    ) -> ExecutionPlan {
        let mut plan = compile_plan(task, &format!("plan-{}", run_id), now_ms);
        for step in &mut plan.steps {
            if step.goal.contains("draft") {
                step.required_context.push("craft_prompt".into());
                step.required_context.push("chapter_mission".into());
            }
            if step.goal.contains("validate") {
                step.required_context.push("scene_craft_plan".into());
                step.required_context.push("quality_report".into());
            }
            if step.goal.contains("preflight") {
                step.required_context.extend_from_slice(&[
                    "outline".into(),
                    "lorebook".into(),
                    "chapter_mission".into(),
                    "story_contract".into(),
                ]);
            }
        }
        plan
    }

    pub fn record_run_completion(
        &mut self,
        request: &WriterAgentRunRequest,
        result: &WriterAgentRunResult,
    ) -> Result<(), String> {
        if request.task == WriterAgentTask::ManualRequest {
            self.record_manual_exchange(
                &request.observation,
                &request.user_instruction,
                &result.answer,
                &result.source_refs,
            )?;
        }
        if request.task == WriterAgentTask::ContinuityDiagnostic
            || request.task == WriterAgentTask::PlanningReview
        {
            if let Some(receipt) = result.task_receipt.as_ref() {
                let artifact = if request.task == WriterAgentTask::PlanningReview {
                    crate::writer_agent::task_receipt::build_planning_review_artifact(
                        receipt,
                        &result.answer,
                        now_ms(),
                    )
                    .map_err(|mismatches| {
                        format!(
                            "PlanningReview planning_review_report artifact failed receipt validation: {:?}",
                            mismatches
                        )
                    })?
                } else {
                    crate::writer_agent::task_receipt::build_diagnostic_report_artifact(
                        receipt,
                        &result.answer,
                        now_ms(),
                    )
                    .map_err(|mismatches| {
                        format!(
                            "ContinuityDiagnostic diagnostic_report artifact failed receipt validation: {:?}",
                            mismatches
                        )
                    })?
                };
                self.record_task_artifact_run_event(&artifact);

                let trigger = if request.task == WriterAgentTask::PlanningReview {
                    agent_harness_core::CompactionTrigger::PlanningReviewComplete
                } else {
                    agent_harness_core::CompactionTrigger::ContinuityDiagnosticComplete
                };
                self.record_compaction_trigger_event(&trigger, &result.source_refs, now_ms());
            }
        }
        Ok(())
    }
}

fn required_context_sources_for_preflight(
    task: crate::writer_agent::context::AgentTask,
) -> Vec<String> {
    use crate::writer_agent::context::AgentTask;
    match task {
        AgentTask::ChapterGeneration => vec![
            "project_brief",
            "chapter_mission",
            "next_beat",
            "outline",
            "previous_chapter",
            "canon",
            "promise",
        ],
        AgentTask::GhostWriting | AgentTask::InlineRewrite => vec![
            "cursor_prefix",
            "chapter_mission",
            "next_beat",
            "project_brief",
            "canon",
        ],
        AgentTask::ContinuityDiagnostic | AgentTask::CanonMaintenance => {
            vec!["canon", "chapter_mission", "project_brief", "outline"]
        }
        AgentTask::PlanningReview => vec![
            "chapter_mission",
            "project_brief",
            "next_beat",
            "canon",
            "promise",
        ],
        AgentTask::ProposalEvaluation => vec!["canon", "decision", "chapter_mission"],
        AgentTask::ManualRequest => Vec::new(),
    }
    .into_iter()
    .map(str::to_string)
    .collect()
}

fn context_quality_source_type(
    source: &crate::writer_agent::context::ContextSource,
) -> &'static str {
    use crate::writer_agent::context::ContextSource;
    match source {
        ContextSource::SystemContract => "system_contract",
        ContextSource::ProjectBrief => "project_brief",
        ContextSource::ChapterMission => "chapter_mission",
        ContextSource::NextBeat => "next_beat",
        ContextSource::ResultFeedback => "result_feedback",
        ContextSource::AuthorStyle => "author_style",
        ContextSource::CanonSlice => "canon",
        ContextSource::PromiseSlice => "promise",
        ContextSource::DecisionSlice => "decision",
        ContextSource::BookState => "book_state",
        ContextSource::ArcSnapshot => "arc_snapshot",
        ContextSource::VolumeSnapshot => "volume_snapshot",
        ContextSource::OutlineSlice => "outline",
        ContextSource::RagExcerpt => "rag",
        ContextSource::CursorPrefix => "cursor_prefix",
        ContextSource::CursorSuffix => "cursor_suffix",
        ContextSource::SelectedText => "selected_text",
        ContextSource::PreviousChapter => "previous_chapter",
        ContextSource::NextChapter => "next_chapter",
        ContextSource::NeighborText => "neighbor_text",
        ContextSource::StoryImpactRadius => "story_impact",
        ContextSource::ReaderCompensation => "reader_compensation",
    }
}

fn taxonomy_for_source(source: &crate::writer_agent::context::ContextSource) -> &'static str {
    use crate::writer_agent::context::ContextSource;
    use agent_harness_core::{
        TAXONOMY_AUTHOR_VOICE, TAXONOMY_CANON, TAXONOMY_INSTRUCTION, TAXONOMY_LORE,
        TAXONOMY_MEMORY, TAXONOMY_OUTLINE, TAXONOMY_PRIOR_CHAPTER, TAXONOMY_PROJECT_BRAIN,
        TAXONOMY_PROMISE, TAXONOMY_SCENE_PLAN, TAXONOMY_UNKNOWN,
    };
    match source {
        ContextSource::ProjectBrief => TAXONOMY_PROJECT_BRAIN,
        ContextSource::ChapterMission | ContextSource::NextBeat => TAXONOMY_SCENE_PLAN,
        ContextSource::CanonSlice => TAXONOMY_CANON,
        ContextSource::PromiseSlice => TAXONOMY_PROMISE,
        ContextSource::DecisionSlice => TAXONOMY_MEMORY,
        ContextSource::AuthorStyle => TAXONOMY_AUTHOR_VOICE,
        ContextSource::OutlineSlice | ContextSource::StoryImpactRadius => TAXONOMY_OUTLINE,
        ContextSource::PreviousChapter => TAXONOMY_PRIOR_CHAPTER,
        ContextSource::BookState | ContextSource::ArcSnapshot | ContextSource::VolumeSnapshot => {
            TAXONOMY_LORE
        }
        ContextSource::SystemContract => TAXONOMY_INSTRUCTION,
        _ => TAXONOMY_UNKNOWN,
    }
}

fn context_quality_recommendation_label(
    recommendation: &agent_harness_core::ContextQualityRecommendation,
) -> &'static str {
    match recommendation {
        agent_harness_core::ContextQualityRecommendation::Sufficient => "sufficient",
        agent_harness_core::ContextQualityRecommendation::Supplement { .. } => "supplement",
        agent_harness_core::ContextQualityRecommendation::Critical { .. } => "critical",
    }
}
