use agent_harness_core::{
    default_writing_tool_registry, EffectiveToolInventory, PermissionMode, PermissionPolicy,
    ToolDescriptor, ToolFilter, ToolSideEffectLevel,
};
use std::time::{SystemTime, UNIX_EPOCH};

pub type AgentToolDescriptor = ToolDescriptor;

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

pub fn registered_tools() -> Vec<AgentToolDescriptor> {
    default_writing_tool_registry().list()
}

pub fn effective_tool_inventory() -> EffectiveToolInventory {
    let registry = default_writing_tool_registry();
    let filter = ToolFilter {
        intent: None,
        include_requires_approval: true,
        include_disabled: false,
        max_side_effect_level: Some(ToolSideEffectLevel::Write),
        required_tags: Vec::new(),
    };
    let policy = PermissionPolicy::new(PermissionMode::WorkspaceWrite);
    registry.effective_inventory(&filter, &policy)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_registry_marks_chapter_generation_as_approval_required_write() {
        let tools = registered_tools();
        let chapter_tool = tools
            .iter()
            .find(|tool| tool.name == "generate_chapter_draft")
            .expect("chapter generation tool registered");
        assert_eq!(
            chapter_tool.side_effect_level,
            agent_harness_core::ToolSideEffectLevel::Write
        );
        assert!(chapter_tool.requires_approval);
    }

    #[test]
    fn effective_inventory_blocks_chapter_generation_by_default() {
        let inventory = effective_tool_inventory();
        assert!(inventory
            .blocked
            .iter()
            .any(|entry| entry.descriptor.name == "generate_chapter_draft"
                && entry.status == agent_harness_core::EffectiveToolStatus::ApprovalRequired));
        assert!(!inventory
            .allowed
            .iter()
            .any(|tool| tool.name == "generate_chapter_draft"));
    }
}
