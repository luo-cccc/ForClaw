use super::*;

impl WriterAgentKernel {
    pub fn ghost_context_pack(&self, observation: &WriterObservation) -> WritingContextPack {
        crate::writer_agent::context::query_story_os(
            AgentTask::GhostWriting,
            observation,
            &self.memory,
            AgentTask::GhostWriting.default_budget(),
        )
    }

    pub fn context_pack_for(
        &self,
        task: AgentTask,
        observation: &WriterObservation,
        total_budget: usize,
    ) -> WritingContextPack {
        crate::writer_agent::context::query_story_os(task, observation, &self.memory, total_budget)
    }

    pub fn context_pack_for_default(
        &self,
        task: AgentTask,
        observation: &WriterObservation,
    ) -> WritingContextPack {
        let total_budget = task.default_budget();
        crate::writer_agent::context::query_story_os(task, observation, &self.memory, total_budget)
    }
}
