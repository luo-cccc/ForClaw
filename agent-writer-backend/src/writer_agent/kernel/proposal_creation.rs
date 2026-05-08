use super::*;

impl WriterAgentKernel {
    pub fn create_llm_ghost_proposal(
        &mut self,
        observation: WriterObservation,
        continuation: String,
        model: &str,
    ) -> Result<AgentProposal, String> {
        let continuation = sanitize_continuation(&continuation);
        if continuation.is_empty() {
            return Err("empty LLM continuation".to_string());
        }

        let intent = self.intent.classify(
            &observation.paragraph,
            observation.has_selection(),
            observation.reason == super::observation::ObservationReason::ChapterSwitch,
        );
        let context_pack = self.ghost_context_pack(&observation);
        self.record_context_pack_built_run_event(&observation, &context_pack, now_ms());
        self.record_task_packet_for(
            AgentTask::GhostWriting,
            &observation,
            &context_pack,
            "Generate an LLM-backed ghost continuation grounded in the active writing context.",
            vec![
                "LLM ghost is short enough to review inline.".to_string(),
                "LLM ghost cites the same required context pack used for generation.".to_string(),
            ],
        );
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

        let proposal = AgentProposal {
            id: proposal_id(&self.session_id, self.proposal_counter),
            observation_id: observation.id.clone(),
            kind: ProposalKind::Ghost,
            priority: ProposalPriority::Ambient,
            target: observation
                .cursor
                .clone()
                .map(|c| super::observation::TextRange {
                    from: c.to,
                    to: c.to,
                }),
            preview: continuation.clone(),
            operations: vec![WriterOperation::TextInsert {
                chapter,
                at: insert_at,
                text: continuation,
                revision,
            }],
            rationale: format!(
                "LLM增强续写: {}. 意图识别: {:?} ({:.0}%). ContextPack: {} sources, {}/{} chars.",
                model,
                intent.primary,
                intent.confidence * 100.0,
                context_pack.sources.len(),
                context_pack.total_chars,
                context_pack.budget_limit
            ),
            evidence: context_pack_evidence(&context_pack, &observation),
            risks: ghost_quality_risks(&self.memory, &self.project_id),
            alternatives: vec![],
            confidence: ghost_confidence(intent.confidence, &self.memory, &self.project_id),
            expires_at: Some(observation.created_at + 60_000),
        };

        self.proposal_counter += 1;
        self.register_proposal(proposal, Some(context_budget_trace(&context_pack)))
            .ok_or_else(|| "duplicate LLM continuation suppressed".to_string())
    }

    pub fn create_inline_operation_proposal(
        &mut self,
        observation: WriterObservation,
        instruction: &str,
        draft: String,
        model: &str,
    ) -> Result<AgentProposal, String> {
        let draft = sanitize_continuation(&draft);
        if draft.is_empty() {
            return Err("empty inline operation draft".to_string());
        }

        let context_pack = assemble_observation_context_with_default_budget(
            AgentTask::InlineRewrite,
            &observation,
            &self.memory,
        );
        self.record_context_pack_built_run_event(&observation, &context_pack, now_ms());
        let chapter = observation
            .chapter_title
            .clone()
            .or_else(|| self.active_chapter.clone())
            .unwrap_or_else(|| "Chapter-1".to_string());
        let revision = observation
            .chapter_revision
            .clone()
            .unwrap_or_else(|| "missing".to_string());
        let operation = if let Some(selection) = observation.selection.as_ref() {
            if selection.from < selection.to {
                WriterOperation::TextReplace {
                    chapter: chapter.clone(),
                    from: selection.from,
                    to: selection.to,
                    text: draft.clone(),
                    revision,
                }
            } else {
                WriterOperation::TextInsert {
                    chapter: chapter.clone(),
                    at: observation
                        .cursor
                        .as_ref()
                        .map(|c| c.to)
                        .unwrap_or(selection.to),
                    text: draft.clone(),
                    revision,
                }
            }
        } else {
            WriterOperation::TextInsert {
                chapter: chapter.clone(),
                at: observation.cursor.as_ref().map(|c| c.to).unwrap_or(0),
                text: draft.clone(),
                revision,
            }
        };

        let target = match &operation {
            WriterOperation::TextReplace { from, to, .. } => Some(super::observation::TextRange {
                from: *from,
                to: *to,
            }),
            WriterOperation::TextInsert { at, .. } => {
                Some(super::observation::TextRange { from: *at, to: *at })
            }
            _ => None,
        };

        let proposal = AgentProposal {
            id: proposal_id(&self.session_id, self.proposal_counter),
            observation_id: observation.id.clone(),
            kind: ProposalKind::ParallelDraft,
            priority: ProposalPriority::Normal,
            target,
            preview: draft.clone(),
            operations: vec![operation],
            rationale: format!(
                "Inline typed operation via {}. Instruction: {}. ContextPack: {} sources, {}/{} chars.",
                model,
                snippet(instruction, 120),
                context_pack.sources.len(),
                context_pack.total_chars,
                context_pack.budget_limit
            ),
            evidence: context_pack_evidence(&context_pack, &observation),
            risks: vec!["Inline operation should be previewed before accepting.".into()],
            alternatives: vec![],
            confidence: 0.78,
            expires_at: Some(observation.created_at + 120_000),
        };

        self.proposal_counter += 1;
        self.register_proposal(proposal, Some(context_budget_trace(&context_pack)))
            .ok_or_else(|| "duplicate inline operation suppressed".to_string())
    }

    pub fn create_llm_memory_proposals(
        &mut self,
        observation: WriterObservation,
        value: serde_json::Value,
        model: &str,
    ) -> Vec<AgentProposal> {
        let feedback = MemoryExtractionFeedback::from_memory(&self.memory);
        let candidates = llm_memory_candidates_from_value(value, &observation, model)
            .into_iter()
            .filter_map(|candidate| feedback.apply_to_candidate(candidate))
            .collect::<Vec<_>>();
        let mut proposals = Vec::new();
        for candidate in candidates {
            let proposal = match candidate {
                MemoryCandidate::Canon(entity) => {
                    match validate_canon_candidate_with_memory(&entity, &self.memory) {
                        MemoryCandidateQuality::Acceptable => canon_candidate_proposal(
                            &observation,
                            &observation.id,
                            &mut self.proposal_counter,
                            &self.session_id,
                            entity,
                            CandidateSource::Llm(model.to_string()),
                        ),
                        MemoryCandidateQuality::Conflict {
                            existing_name,
                            reason,
                        } => canon_conflict_candidate_proposal(
                            &observation,
                            &observation.id,
                            &mut self.proposal_counter,
                            &self.session_id,
                            entity,
                            existing_name,
                            reason,
                            CandidateSource::Llm(model.to_string()),
                        ),
                        MemoryCandidateQuality::MergeableAttributes {
                            existing_name,
                            attributes,
                        } => canon_attribute_merge_candidate_proposal(
                            &observation,
                            &observation.id,
                            &mut self.proposal_counter,
                            &self.session_id,
                            existing_name,
                            attributes,
                            entity.confidence,
                            CandidateSource::Llm(model.to_string()),
                        ),
                        MemoryCandidateQuality::Vague { .. }
                        | MemoryCandidateQuality::Duplicate { .. } => continue,
                    }
                }
                MemoryCandidate::Promise(promise) => {
                    if validate_promise_candidate_with_dedup(&promise, &self.memory)
                        != MemoryCandidateQuality::Acceptable
                    {
                        continue;
                    }
                    promise_candidate_proposal(
                        &observation,
                        &observation.id,
                        &mut self.proposal_counter,
                        &self.session_id,
                        promise,
                        CandidateSource::Llm(model.to_string()),
                    )
                }
            };

            if let Some(registered) = self.register_proposal(proposal, None) {
                proposals.push(registered);
            }
        }
        proposals
    }

    pub fn diagnose_paragraph(
        &self,
        paragraph: &str,
        paragraph_offset: usize,
        chapter_id: &str,
    ) -> Vec<DiagnosticResult> {
        self.diagnostics.diagnose(
            paragraph,
            paragraph_offset,
            chapter_id,
            &self.project_id,
            &self.memory,
        )
    }
}
