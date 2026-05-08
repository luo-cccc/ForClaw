use super::*;

impl WriterAgentKernel {
    pub fn observe(
        &mut self,
        observation: WriterObservation,
    ) -> Result<Vec<AgentProposal>, String> {
        self.observe_with_precomputed_save_result(observation, None)
    }

    pub fn observe_save_result(
        &mut self,
        observation: WriterObservation,
        result: ChapterResultSummary,
    ) -> Result<Vec<AgentProposal>, String> {
        self.observe_with_precomputed_save_result(observation, Some(result))
    }

    fn observe_with_precomputed_save_result(
        &mut self,
        observation: WriterObservation,
        save_result: Option<ChapterResultSummary>,
    ) -> Result<Vec<AgentProposal>, String> {
        self.observation_counter += 1;
        let mut proposals = Vec::new();
        let mut proposal_context_budgets = HashMap::new();
        let obs_id = observation.id.clone();
        self.active_chapter = observation.chapter_title.clone();
        self.memory
            .record_observation_trace(
                &observation.id,
                observation.created_at,
                &format!("{:?}", observation.reason),
                observation.chapter_title.as_deref(),
                &snippet(&observation.paragraph, 120),
            )
            .ok();
        self.record_observation_run_event(&observation);

        let intent = self.intent.classify(
            &observation.paragraph,
            observation.has_selection(),
            observation.reason == super::observation::ObservationReason::ChapterSwitch,
        );

        if observation.reason == super::observation::ObservationReason::Save {
            let result = save_result
                .unwrap_or_else(|| chapter_result_from_observation(&observation, &self.memory));
            if !result.is_empty() {
                self.memory.upsert_chapter_result(&result).ok();
                let calibration = self.calibrate_chapter_mission(
                    &observation,
                    &result,
                    &obs_id,
                    self.proposal_counter,
                );
                if let Some(cal) = calibration {
                    self.proposal_counter += 1;
                    proposals.push(cal);
                }
                self.touch_promise_last_seen_from_result(&result).ok();
                proposals.extend(chapter_mission_result_proposals(
                    &observation,
                    &result,
                    &self.memory,
                    &obs_id,
                    &mut self.proposal_counter,
                    &self.session_id,
                ));
            }
        }

        if let Ok(promises) = self.memory.get_open_promises() {
            for (_kind, title, desc, chapter) in &promises {
                if observation.reason == super::observation::ObservationReason::ChapterSwitch {
                    proposals.push(AgentProposal {
                        id: proposal_id(&self.session_id, self.proposal_counter),
                        observation_id: obs_id.clone(),
                        kind: ProposalKind::PlotPromise,
                        priority: ProposalPriority::Normal,
                        target: None,
                        preview: format!("未回收伏笔: {} ({}章)", title, chapter),
                        operations: vec![],
                        rationale: format!("{}: {}", title, desc),
                        evidence: vec![EvidenceRef {
                            source: EvidenceSource::PromiseLedger,
                            reference: title.clone(),
                            snippet: desc.clone(),
                        }],
                        risks: vec![],
                        alternatives: vec![],
                        confidence: 0.7,
                        expires_at: None,
                    });
                    self.proposal_counter += 1;
                }
            }
        }

        if matches!(
            observation.reason,
            super::observation::ObservationReason::Save
                | super::observation::ObservationReason::ChapterSwitch
        ) {
            for candidate in memory_candidates_from_observation(
                &observation,
                &self.memory,
                &obs_id,
                &mut self.proposal_counter,
                &self.session_id,
            ) {
                proposals.push(candidate);
            }
        }

        if matches!(
            observation.reason,
            super::observation::ObservationReason::Idle
                | super::observation::ObservationReason::ChapterSwitch
                | super::observation::ObservationReason::Save
        ) {
            let paragraph_offset = observation
                .cursor
                .as_ref()
                .map(|cursor| {
                    cursor
                        .from
                        .saturating_sub(observation.paragraph.chars().count())
                })
                .unwrap_or(0);
            let chapter_id = observation.chapter_title.as_deref().unwrap_or("Chapter-1");
            let diagnostics = self.diagnostics.diagnose(
                &observation.paragraph,
                paragraph_offset,
                chapter_id,
                &observation.project_id,
                &self.memory,
            );
            if observation.reason == super::observation::ObservationReason::Save {
                let report =
                    crate::writer_agent::post_write_diagnostics::build_post_write_diagnostic_report(
                        &observation,
                        &diagnostics,
                        observation.created_at,
                    );
                self.record_post_write_diagnostic_report(&report);
                self.record_save_completed_run_event(
                    SaveCompletedEventContext {
                        observation_id: observation.id.clone(),
                        chapter_title: observation.chapter_title.clone(),
                        chapter_revision: observation.chapter_revision.clone(),
                        save_result: "chapter_save:observed".to_string(),
                    },
                    None,
                    None,
                    Some(&report),
                    observation.created_at,
                );
            }
            for diagnostic in diagnostics {
                proposals.push(diagnostic_to_proposal(
                    diagnostic,
                    &observation,
                    &obs_id,
                    &proposal_id(&self.session_id, self.proposal_counter),
                ));
                self.proposal_counter += 1;
            }
        }

        let should_offer_continuation = matches!(
            &intent.desired_behavior,
            AgentBehavior::SuggestContinuation | AgentBehavior::GenerateDraft
        );

        if observation.paragraph.chars().count() >= 32
            && should_offer_continuation
            && matches!(
                observation.reason,
                super::observation::ObservationReason::Idle
                    | super::observation::ObservationReason::Typed
            )
        {
            let context_pack = assemble_observation_context_with_default_budget(
                AgentTask::GhostWriting,
                &observation,
                &self.memory,
            );
            self.record_context_pack_built_run_event(
                &observation,
                &context_pack,
                observation.created_at,
            );
            self.record_task_packet_for(
                AgentTask::GhostWriting,
                &observation,
                &context_pack,
                "Continue from the current cursor while preserving chapter mission, canon, and open promises.",
                vec![
                    "Continuation fits the local paragraph without forcing a broad rewrite."
                        .to_string(),
                    "Continuation does not introduce canon or promise-ledger conflicts."
                        .to_string(),
                ],
            );
            let continuation = draft_continuation(&intent.primary, &observation, &context_pack);
            let insert_at = observation.cursor.as_ref().map(|c| c.to).unwrap_or(0);
            let chapter = observation
                .chapter_title
                .clone()
                .or_else(|| self.active_chapter.clone())
                .unwrap_or_else(|| "Chapter-1".to_string());
            let revision = observation
                .chapter_revision
                .clone()
                .unwrap_or_else(|| "missing".to_string());
            let mut alternatives = ghost_alternatives(
                &intent.primary,
                &observation,
                &context_pack,
                &chapter,
                insert_at,
                &revision,
            );
            let style_evidence = ghost_consume_style_preferences(&self.memory, &mut alternatives);
            let fallback_operation = WriterOperation::TextInsert {
                chapter: chapter.clone(),
                at: insert_at,
                text: continuation.clone(),
                revision: revision.clone(),
            };
            let primary_preview = alternatives
                .first()
                .map(|alternative| alternative.preview.clone())
                .unwrap_or_else(|| continuation.clone());
            let primary_operation = alternatives
                .first()
                .and_then(|alternative| alternative.operation.clone())
                .unwrap_or_else(|| fallback_operation.clone());
            let mut evidence = context_pack_evidence(&context_pack, &observation);
            evidence.extend(style_evidence.clone());
            let mut rationale = format!(
                "意图识别: {:?} ({:.0}%). ContextPack: {} sources, {}/{} chars.",
                intent.primary,
                intent.confidence * 100.0,
                context_pack.sources.len(),
                context_pack.total_chars,
                context_pack.budget_limit
            );
            if !style_evidence.is_empty() {
                rationale.push_str(" StyleLedger reordered the default ghost branch.");
            }

            let proposal_id_value = proposal_id(&self.session_id, self.proposal_counter);
            proposal_context_budgets.insert(
                proposal_id_value.clone(),
                context_budget_trace(&context_pack),
            );
            proposals.push(AgentProposal {
                id: proposal_id_value,
                observation_id: obs_id.clone(),
                kind: ProposalKind::Ghost,
                priority: ProposalPriority::Ambient,
                target: observation
                    .cursor
                    .clone()
                    .map(|c| super::observation::TextRange {
                        from: c.to,
                        to: c.to,
                    }),
                preview: primary_preview,
                operations: vec![primary_operation],
                rationale,
                evidence,
                risks: ghost_quality_risks(&self.memory, &self.project_id),
                alternatives,
                confidence: ghost_confidence(intent.confidence, &self.memory, &self.project_id),
                expires_at: Some(observation.created_at + 30_000),
            });
            self.proposal_counter += 1;
        }

        self.observations.push(observation);
        Ok(self.register_proposals(proposals, &proposal_context_budgets))
    }

    fn calibrate_chapter_mission(
        &self,
        observation: &WriterObservation,
        result: &ChapterResultSummary,
        observation_id: &str,
        proposal_counter: u64,
    ) -> Option<AgentProposal> {
        let chapter_title = observation.chapter_title.as_deref()?;
        let mission = self
            .memory
            .get_chapter_mission(&self.project_id, chapter_title)
            .ok()??;

        let status = calibrated_mission_status(&mission, result);
        if mission.status == status {
            return None;
        }

        let source_ref = format!("result_feedback:{}", result.source_ref);
        self.memory
            .record_decision(
                chapter_title,
                "Chapter mission calibration",
                &format!("mission_status:{}", status),
                &[],
                &mission.render_for_context(),
                std::slice::from_ref(&result.source_ref),
            )
            .ok();

        let preview = match status.as_str() {
            "completed" => format!("Chapter mission completed: {}", chapter_title),
            "drifted" => format!("Chapter mission drifted: {}", chapter_title),
            "needs_review" => format!("Chapter mission needs review: {}", chapter_title),
            _ => format!("Chapter mission status → {}: {}", status, chapter_title),
        };
        let rationale = format!(
            "Save result triggered mission calibration from {} to {} based on must_include/must_not/expected_ending checks.",
            mission.status, status
        );

        Some(AgentProposal {
            id: proposal_id(&self.session_id, proposal_counter),
            observation_id: observation_id.to_string(),
            kind: ProposalKind::ChapterMission,
            priority: if status == "drifted" {
                ProposalPriority::Urgent
            } else {
                ProposalPriority::Normal
            },
            target: observation.cursor.clone(),
            preview,
            operations: vec![WriterOperation::ChapterMissionUpsert {
                mission: crate::writer_agent::operation::ChapterMissionOp {
                    project_id: self.project_id.clone(),
                    chapter_title: chapter_title.to_string(),
                    mission: mission.mission.clone(),
                    must_include: mission.must_include.clone(),
                    must_not: mission.must_not.clone(),
                    expected_ending: mission.expected_ending.clone(),
                    status: status.clone(),
                    source_ref: source_ref.clone(),
                    blocked_reason: String::new(),
                    retired_history: String::new(),
                },
            }],
            rationale,
            evidence: vec![
                EvidenceRef {
                    source: EvidenceSource::ChapterMission,
                    reference: format!("{}:mission", chapter_title),
                    snippet: mission.mission.clone(),
                },
                EvidenceRef {
                    source: EvidenceSource::ChapterText,
                    reference: result.source_ref.clone(),
                    snippet: result.summary.clone(),
                },
            ],
            confidence: 0.82,
            expires_at: Some(observation.created_at + 120_000),
            alternatives: vec![],
            risks: if status == "drifted" {
                vec!["Chapter has drifted from its mission — review before continuing.".into()]
            } else {
                vec![]
            },
        })
    }

    fn touch_promise_last_seen_from_result(
        &self,
        result: &ChapterResultSummary,
    ) -> Result<(), String> {
        let haystack = mission_result_haystack(result);
        for promise in self
            .memory
            .get_open_promise_summaries()
            .map_err(|e| e.to_string())?
        {
            let title_hit =
                !promise.title.trim().is_empty() && haystack.contains(promise.title.trim());
            let description_hit = !promise.description.trim().is_empty()
                && cue_hit_score(&promise.description, &haystack) > 0;
            if title_hit || description_hit {
                self.memory
                    .touch_promise_last_seen(promise.id, &result.chapter_title, &result.source_ref)
                    .map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    }
}
