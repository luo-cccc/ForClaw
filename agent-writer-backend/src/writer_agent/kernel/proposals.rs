use super::*;

impl WriterAgentKernel {
    pub(super) fn register_proposals(
        &mut self,
        proposals: Vec<AgentProposal>,
        context_budgets: &HashMap<String, ContextBudgetTrace>,
    ) -> Vec<AgentProposal> {
        proposals
            .into_iter()
            .filter_map(|proposal| {
                let context_budget = context_budgets.get(&proposal.id).cloned();
                self.register_proposal(proposal, context_budget)
            })
            .collect()
    }

    pub(super) fn register_proposal(
        &mut self,
        proposal: AgentProposal,
        context_budget: Option<ContextBudgetTrace>,
    ) -> Option<AgentProposal> {
        self.prune_suppressed_slots(now_ms());
        if self.is_slot_suppressed(&proposal) {
            return None;
        }

        let slot = proposal_slot_key(&proposal);
        let existing = self
            .proposals
            .iter()
            .rev()
            .find(|existing| {
                self.is_pending_proposal(existing) && proposal_slot_key(existing) == slot
            })
            .cloned();

        if let Some(existing) = existing {
            if should_replace_proposal(&existing, &proposal) {
                self.memory
                    .update_proposal_trace_state(&existing.id, "superseded")
                    .ok();
                self.superseded_proposals.insert(existing.id);
            } else {
                return None;
            }
        }

        if let Some(context_budget) = context_budget.clone() {
            self.proposal_context_budgets
                .insert(proposal.id.clone(), context_budget);
        }
        self.proposals.push(proposal.clone());
        let created_at = self
            .observations
            .iter()
            .find(|observation| observation.id == proposal.observation_id)
            .map(|observation| observation.created_at)
            .unwrap_or_else(now_ms);
        self.memory
            .record_proposal_trace(
                &proposal_trace_summary(&proposal, "pending", context_budget),
                created_at,
            )
            .ok();
        self.memory
            .record_context_recalls(
                &self.project_id,
                &proposal.id,
                &proposal.observation_id,
                &proposal.evidence,
                created_at,
            )
            .ok();
        self.record_proposal_run_event(&proposal, created_at);
        if let Some(slot) = memory_operation_slot(&proposal) {
            self.record_memory_candidate_created_run_event(&proposal, slot, created_at);
        }
        for operation in proposal
            .operations
            .iter()
            .filter(|operation| operation_is_write_capable(operation))
        {
            self.push_operation_lifecycle(
                Some(proposal.id.clone()),
                operation,
                WriterOperationLifecycleState::Proposed,
                None,
                None,
                None,
                created_at,
            );
        }
        Some(proposal)
    }

    fn is_pending_proposal(&self, proposal: &AgentProposal) -> bool {
        !self.superseded_proposals.contains(&proposal.id)
            && !self
                .feedback_events
                .iter()
                .any(|f| f.proposal_id == proposal.id)
            && !proposal_expired(proposal, now_ms())
    }

    pub(super) fn suppress_slot_after_feedback(
        &mut self,
        proposal: &AgentProposal,
        feedback: &ProposalFeedback,
    ) {
        let ttl_ms = match feedback.action {
            FeedbackAction::Snoozed => 10 * 60 * 1_000,
            FeedbackAction::Rejected => 5 * 60 * 1_000,
            FeedbackAction::Edited => 2 * 60 * 1_000,
            FeedbackAction::Accepted | FeedbackAction::Explained => return,
        };
        self.suppressed_slots.push(SuppressedProposalSlot {
            slot: suppression_slot_key(proposal),
            until: feedback.created_at.saturating_add(ttl_ms),
        });
    }

    fn is_slot_suppressed(&self, proposal: &AgentProposal) -> bool {
        let slot = suppression_slot_key(proposal);
        let now = now_ms();
        self.suppressed_slots
            .iter()
            .any(|entry| entry.slot == slot && entry.until > now)
    }

    fn prune_suppressed_slots(&mut self, now: u64) {
        self.suppressed_slots.retain(|entry| entry.until > now);
    }
}
