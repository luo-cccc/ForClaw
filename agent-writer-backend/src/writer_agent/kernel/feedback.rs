use super::*;

impl WriterAgentKernel {
    pub fn apply_feedback(&mut self, feedback: ProposalFeedback) -> Result<(), String> {
        let proposal = self
            .proposals
            .iter()
            .find(|p| p.id == feedback.proposal_id)
            .cloned();
        let positive_feedback_ready = proposal
            .as_ref()
            .map(|prop| !feedback.is_positive() || self.proposal_positive_feedback_ready(prop))
            .unwrap_or(true);
        let feedback_result = if feedback.is_positive() && !positive_feedback_ready {
            Some("deferred:missing_durable_save".to_string())
        } else {
            Some("recorded".to_string())
        };

        self.memory
            .record_feedback(
                &feedback.proposal_id,
                match feedback.action {
                    FeedbackAction::Accepted => "accepted",
                    FeedbackAction::Rejected => "rejected",
                    FeedbackAction::Edited => "edited",
                    FeedbackAction::Snoozed => "snoozed",
                    FeedbackAction::Explained => "explained",
                },
                feedback.reason.as_deref().unwrap_or(""),
                feedback.final_text.as_deref().unwrap_or(""),
            )
            .map_err(|e| format!("feedback: {}", e))?;

        if feedback.is_positive() && positive_feedback_ready {
            if let Some(prop) = proposal.as_ref() {
                record_memory_candidate_feedback(
                    &self.memory,
                    prop,
                    true,
                    feedback.reason.as_deref(),
                    feedback.created_at,
                );
                record_memory_audit_event(&self.memory, prop, &feedback);
                record_feedback_style_preference(
                    &self.memory,
                    &format!("accepted_{:?}", prop.kind),
                    &prop.rationale,
                    true,
                );
                self.memory
                    .record_decision(
                        self.active_chapter.as_deref().unwrap_or("project"),
                        &format!("{:?}", prop.kind),
                        "accepted",
                        &[],
                        &prop.rationale,
                        &prop
                            .evidence
                            .iter()
                            .map(|e| e.reference.clone())
                            .collect::<Vec<_>>(),
                    )
                    .ok();
            }
        } else if !feedback.is_positive() {
            if let Some(prop) = proposal.as_ref() {
                let action = match feedback.action {
                    FeedbackAction::Rejected => "rejected",
                    FeedbackAction::Edited => "edited",
                    FeedbackAction::Snoozed => "snoozed",
                    FeedbackAction::Explained => "explained",
                    FeedbackAction::Accepted => "accepted",
                };
                if feedback.is_negative() || matches!(feedback.action, FeedbackAction::Edited) {
                    record_memory_candidate_feedback(
                        &self.memory,
                        prop,
                        false,
                        feedback.reason.as_deref(),
                        feedback.created_at,
                    );
                    record_memory_audit_event(&self.memory, prop, &feedback);
                    self.memory
                        .record_decision(
                            self.active_chapter.as_deref().unwrap_or("project"),
                            &format!("{:?}", prop.kind),
                            action,
                            &[],
                            feedback.reason.as_deref().unwrap_or(&prop.rationale),
                            &prop
                                .evidence
                                .iter()
                                .map(|e| e.reference.clone())
                                .collect::<Vec<_>>(),
                        )
                        .ok();
                }
                if prop.kind == ProposalKind::Ghost
                    && matches!(feedback.action, FeedbackAction::Explained)
                {
                    record_feedback_style_preference(
                        &self.memory,
                        "ignored_ghost",
                        &prop.rationale,
                        false,
                    );
                }
            }
        }

        if let Some(prop) = proposal.as_ref() {
            self.suppress_slot_after_feedback(prop, &feedback);
        }

        self.memory
            .record_feedback_trace(&super::memory::FeedbackTraceSummary {
                proposal_id: feedback.proposal_id.clone(),
                action: format!("{:?}", feedback.action),
                reason: feedback.reason.clone(),
                created_at: feedback.created_at,
            })
            .ok();
        self.memory
            .update_proposal_trace_state(
                &feedback.proposal_id,
                &format!("feedback:{:?}", feedback.action),
            )
            .ok();

        if let Some(prop) = proposal.as_ref() {
            for operation in prop
                .operations
                .iter()
                .filter(|operation| operation_is_write_capable(operation))
            {
                self.push_operation_lifecycle(
                    Some(prop.id.clone()),
                    operation,
                    WriterOperationLifecycleState::FeedbackRecorded,
                    None,
                    None,
                    feedback_result.clone(),
                    feedback.created_at,
                );
            }
        }

        self.record_feedback_run_event(&feedback, feedback_result.as_deref());
        self.feedback_events.push(feedback);
        Ok(())
    }

    pub fn record_implicit_ghost_rejection(
        &mut self,
        proposal_id: &str,
        created_at: u64,
    ) -> Result<bool, String> {
        let Some(proposal) = self.proposals.iter().find(|p| p.id == proposal_id).cloned() else {
            return Ok(false);
        };
        if proposal.kind != ProposalKind::Ghost
            || self
                .feedback_events
                .iter()
                .any(|feedback| feedback.proposal_id == proposal.id)
        {
            return Ok(false);
        }

        let slot = suppression_slot_key(&proposal);
        self.prune_ignored_ghost_slots(created_at);
        let suppressed = if let Some(index) = self
            .ignored_ghost_slots
            .iter()
            .position(|entry| entry.slot == slot)
        {
            let entry = &mut self.ignored_ghost_slots[index];
            entry.count = entry.count.saturating_add(1);
            entry.last_seen = created_at;
            if entry.count >= 3 {
                self.ignored_ghost_slots.remove(index);
                true
            } else {
                false
            }
        } else {
            self.ignored_ghost_slots.push(IgnoredGhostSlot {
                slot,
                count: 1,
                last_seen: created_at,
            });
            false
        };
        let feedback = ProposalFeedback {
            proposal_id: proposal.id.clone(),
            action: if suppressed {
                FeedbackAction::Snoozed
            } else {
                FeedbackAction::Explained
            },
            final_text: None,
            reason: Some(if suppressed {
                "Implicit rejection: author continued writing over repeated ghost text.".to_string()
            } else {
                "Implicit pass: author continued writing instead of accepting ghost text."
                    .to_string()
            }),
            created_at,
        };
        self.apply_feedback(feedback)?;
        Ok(suppressed)
    }

    fn prune_ignored_ghost_slots(&mut self, now: u64) {
        self.ignored_ghost_slots
            .retain(|entry| now.saturating_sub(entry.last_seen) <= 10 * 60 * 1_000);
    }

    fn proposal_positive_feedback_ready(&self, proposal: &AgentProposal) -> bool {
        proposal
            .operations
            .iter()
            .filter(|operation| operation_is_write_capable(operation))
            .all(|operation| {
                if operation_requires_durable_save(operation) {
                    self.lifecycle_has_state(
                        &proposal.id,
                        operation,
                        WriterOperationLifecycleState::DurablySaved,
                    )
                } else {
                    self.lifecycle_has_state(
                        &proposal.id,
                        operation,
                        WriterOperationLifecycleState::Applied,
                    ) || self.lifecycle_has_state(
                        &proposal.id,
                        operation,
                        WriterOperationLifecycleState::DurablySaved,
                    )
                }
            })
    }

    pub(super) fn proposal_state(&self, proposal: &AgentProposal, now: u64) -> String {
        if self.superseded_proposals.contains(&proposal.id) {
            return "superseded".to_string();
        }
        if let Some(feedback) = self
            .feedback_events
            .iter()
            .find(|feedback| feedback.proposal_id == proposal.id)
        {
            return format!("feedback:{:?}", feedback.action);
        }
        if proposal_expired(proposal, now) {
            return "expired".to_string();
        }
        "pending".to_string()
    }

    pub(super) fn story_review_queue_status(
        &self,
        proposal: &AgentProposal,
        now: u64,
    ) -> StoryReviewQueueStatus {
        if proposal_expired(proposal, now) {
            return StoryReviewQueueStatus::Expired;
        }
        if let Some(feedback) = self
            .feedback_events
            .iter()
            .find(|feedback| feedback.proposal_id == proposal.id)
        {
            return match feedback.action {
                FeedbackAction::Accepted | FeedbackAction::Edited => {
                    StoryReviewQueueStatus::Accepted
                }
                FeedbackAction::Snoozed => StoryReviewQueueStatus::Snoozed,
                FeedbackAction::Rejected | FeedbackAction::Explained => {
                    StoryReviewQueueStatus::Ignored
                }
            };
        }
        StoryReviewQueueStatus::Pending
    }
}

fn record_feedback_style_preference(memory: &WriterMemory, key: &str, value: &str, accepted: bool) {
    if validate_style_preference_with_memory(key, value, memory)
        == MemoryCandidateQuality::Acceptable
    {
        let _ = memory.upsert_style_preference(key, value, accepted);
    }
}
