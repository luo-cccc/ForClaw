use serde::Serialize;

use crate::writer_agent;

#[derive(Serialize, Clone)]
pub(crate) struct StreamChunk {
    pub(crate) content: String,
}

#[derive(Serialize, Clone)]
pub(crate) struct StreamEnd {
    pub(crate) reason: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub(crate) struct InlineWriterOperationEvent {
    pub(crate) request_id: String,
    pub(crate) proposal: writer_agent::proposal::AgentProposal,
    pub(crate) operation: writer_agent::operation::WriterOperation,
}
