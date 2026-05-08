//! ProposalFeedback — what the author did with a proposal.
//! This is the learning signal that turns the agent into a partner.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProposalFeedback {
    #[serde(rename = "proposalId")]
    pub proposal_id: String,
    pub action: FeedbackAction,
    #[serde(rename = "finalText")]
    pub final_text: Option<String>,
    pub reason: Option<String>,
    #[serde(rename = "createdAt")]
    pub created_at: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum FeedbackAction {
    Accepted,
    Rejected,
    Edited,
    Snoozed,
    Explained,
}

impl ProposalFeedback {
    pub fn is_positive(&self) -> bool {
        matches!(self.action, FeedbackAction::Accepted)
    }

    pub fn is_negative(&self) -> bool {
        matches!(
            self.action,
            FeedbackAction::Rejected | FeedbackAction::Snoozed
        )
    }
}
