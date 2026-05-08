//! Proposal lifecycle helpers for WriterAgentKernel.

use crate::writer_agent::proposal::{AgentProposal, ProposalKind, ProposalPriority};

pub(crate) fn should_replace_proposal(existing: &AgentProposal, incoming: &AgentProposal) -> bool {
    if is_llm_ghost(incoming) && !is_llm_ghost(existing) {
        return true;
    }

    if priority_weight(&incoming.priority) > priority_weight(&existing.priority) {
        return true;
    }

    incoming.confidence > existing.confidence + 0.05
}

fn is_llm_ghost(proposal: &AgentProposal) -> bool {
    proposal.kind == ProposalKind::Ghost && proposal.rationale.contains("LLM增强续写")
}

pub(crate) fn priority_weight(priority: &ProposalPriority) -> u8 {
    match priority {
        ProposalPriority::Ambient => 0,
        ProposalPriority::Normal => 1,
        ProposalPriority::Urgent => 2,
    }
}

pub(crate) fn proposal_expired(proposal: &AgentProposal, now: u64) -> bool {
    proposal
        .expires_at
        .map(|expires_at| expires_at <= now)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proposal(
        kind: ProposalKind,
        priority: ProposalPriority,
        confidence: f64,
        rationale: &str,
        expires_at: Option<u64>,
    ) -> AgentProposal {
        AgentProposal {
            id: "proposal-id".into(),
            observation_id: "observation-id".into(),
            kind,
            priority,
            target: None,
            preview: "preview".into(),
            operations: Vec::new(),
            rationale: rationale.into(),
            evidence: Vec::new(),
            risks: Vec::new(),
            alternatives: Vec::new(),
            confidence,
            expires_at,
        }
    }

    #[test]
    fn llm_ghost_replaces_local_ghost() {
        let existing = proposal(
            ProposalKind::Ghost,
            ProposalPriority::Ambient,
            0.9,
            "local",
            None,
        );
        let incoming = proposal(
            ProposalKind::Ghost,
            ProposalPriority::Ambient,
            0.4,
            "LLM增强续写",
            None,
        );

        assert!(should_replace_proposal(&existing, &incoming));
    }

    #[test]
    fn replacement_prefers_priority_before_confidence_delta() {
        let existing = proposal(
            ProposalKind::StyleNote,
            ProposalPriority::Ambient,
            0.9,
            "existing",
            None,
        );
        let incoming = proposal(
            ProposalKind::StyleNote,
            ProposalPriority::Normal,
            0.2,
            "incoming",
            None,
        );

        assert!(should_replace_proposal(&existing, &incoming));
    }

    #[test]
    fn replacement_uses_confidence_margin() {
        let existing = proposal(
            ProposalKind::StyleNote,
            ProposalPriority::Normal,
            0.6,
            "existing",
            None,
        );
        let incoming = proposal(
            ProposalKind::StyleNote,
            ProposalPriority::Normal,
            0.64,
            "incoming",
            None,
        );
        let stronger = proposal(
            ProposalKind::StyleNote,
            ProposalPriority::Normal,
            0.66,
            "stronger",
            None,
        );

        assert!(!should_replace_proposal(&existing, &incoming));
        assert!(should_replace_proposal(&existing, &stronger));
    }

    #[test]
    fn detects_expiry_at_boundary() {
        let active = proposal(
            ProposalKind::Question,
            ProposalPriority::Normal,
            0.5,
            "active",
            Some(11),
        );
        let expired = proposal(
            ProposalKind::Question,
            ProposalPriority::Normal,
            0.5,
            "expired",
            Some(10),
        );
        let timeless = proposal(
            ProposalKind::Question,
            ProposalPriority::Normal,
            0.5,
            "timeless",
            None,
        );

        assert!(!proposal_expired(&active, 10));
        assert!(proposal_expired(&expired, 10));
        assert!(!proposal_expired(&timeless, 10));
    }
}
