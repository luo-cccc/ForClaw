use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::permission::{PermissionDecision, PermissionPolicy};
use crate::router::Intent;

include!("tool_registry/types.in.rs");

include!("tool_registry/defaults.in.rs");

#[cfg(test)]
mod tests {
    use super::*;
    use crate::permission::{PermissionMode, PermissionPolicy};

    #[test]
    fn registry_filters_by_intent_and_approval() {
        let registry = default_writing_tool_registry();
        let tools = registry.filter(&ToolFilter {
            intent: Some(Intent::GenerateContent),
            include_requires_approval: false,
            include_disabled: false,
            max_side_effect_level: Some(ToolSideEffectLevel::ProviderCall),
            required_tags: Vec::new(),
            allowed_names: Vec::new(),
        });

        assert!(tools
            .iter()
            .any(|tool| tool.name == "generate_bounded_continuation"));
        assert!(!tools
            .iter()
            .any(|tool| tool.name == "generate_chapter_draft"));
    }

    #[test]
    fn default_registry_exposes_schema_for_real_callable_tools() {
        let registry = default_writing_tool_registry();
        for (name, required_field) in [
            ("load_current_chapter", "chapter"),
            ("load_outline_node", "chapter"),
            ("search_lorebook", "keyword"),
            ("query_project_brain", "query"),
            ("generate_bounded_continuation", "prompt"),
        ] {
            let tool = registry.get(name).expect("tool registered");
            let schema = tool.input_schema.as_ref().expect("tool has schema");
            assert_eq!(schema["type"], "object");
            assert_eq!(schema["additionalProperties"], false);
            assert!(schema["required"]
                .as_array()
                .is_some_and(|required| required.iter().any(|field| field == required_field)));
            assert!(schema["properties"][required_field].is_object());
        }
    }

    #[test]
    fn effective_openai_tools_include_read_and_provider_tools_only() {
        let registry = default_writing_tool_registry();
        let policy = PermissionPolicy::new(PermissionMode::WorkspaceWrite);
        let tools = registry.to_effective_openai_tools(
            &ToolFilter {
                intent: Some(Intent::GenerateContent),
                include_requires_approval: true,
                include_disabled: false,
                max_side_effect_level: Some(ToolSideEffectLevel::Write),
                required_tags: Vec::new(),
                allowed_names: Vec::new(),
            },
            &policy,
        );
        let names: Vec<&str> = tools
            .iter()
            .filter_map(|tool| tool["function"]["name"].as_str())
            .collect();

        assert!(names.contains(&"load_current_chapter"));
        assert!(names.contains(&"search_lorebook"));
        assert!(names.contains(&"query_project_brain"));
        assert!(names.contains(&"generate_bounded_continuation"));
        assert!(!names.contains(&"generate_chapter_draft"));
        assert!(!names.contains(&"record_run_trace"));
    }

    #[test]
    fn registry_generation_changes_on_upsert() {
        let mut registry = ToolRegistry::new();
        assert_eq!(registry.generation(), 0);

        registry
            .register(ToolDescriptor::new(
                "read",
                "Read something.",
                "none",
                "text",
                ToolSideEffectLevel::Read,
                false,
                100,
                10,
                ToolStage::Context,
            ))
            .unwrap();
        assert_eq!(registry.generation(), 1);

        registry.upsert(ToolDescriptor::new(
            "read",
            "Read something else.",
            "none",
            "text",
            ToolSideEffectLevel::Read,
            false,
            100,
            10,
            ToolStage::Context,
        ));
        assert_eq!(registry.generation(), 2);
        assert_eq!(registry.len(), 1);
    }

    #[test]
    fn effective_inventory_blocks_approval_required_write_tool() {
        let registry = default_writing_tool_registry();
        let policy = PermissionPolicy::new(PermissionMode::WorkspaceWrite);
        let inventory = registry.effective_inventory(
            &ToolFilter {
                intent: Some(Intent::GenerateContent),
                include_requires_approval: true,
                include_disabled: false,
                max_side_effect_level: Some(ToolSideEffectLevel::Write),
                required_tags: Vec::new(),
                allowed_names: Vec::new(),
            },
            &policy,
        );

        assert!(inventory
            .allowed
            .iter()
            .any(|tool| tool.name == "generate_bounded_continuation"));
        assert!(!inventory
            .allowed
            .iter()
            .any(|tool| tool.name == "generate_chapter_draft"));

        let blocked = inventory
            .blocked
            .iter()
            .find(|entry| entry.descriptor.name == "generate_chapter_draft")
            .expect("chapter draft tool should be present in blocked inventory");
        assert_eq!(blocked.status, EffectiveToolStatus::ApprovalRequired);
        assert!(blocked
            .reason
            .as_deref()
            .is_some_and(|reason| reason.contains("requires explicit approval")));
    }

    #[test]
    fn effective_inventory_reports_filter_reasons() {
        let registry = default_writing_tool_registry();
        let policy = PermissionPolicy::new(PermissionMode::WorkspaceWrite);
        let inventory = registry.effective_inventory(
            &ToolFilter {
                intent: Some(Intent::Linter),
                include_requires_approval: true,
                include_disabled: false,
                max_side_effect_level: Some(ToolSideEffectLevel::Read),
                required_tags: Vec::new(),
                allowed_names: Vec::new(),
            },
            &policy,
        );

        let blocked = inventory
            .blocked
            .iter()
            .find(|entry| entry.descriptor.name == "query_project_brain")
            .expect("provider-call tool should be blocked by side-effect ceiling");
        assert_eq!(blocked.status, EffectiveToolStatus::SideEffectTooHigh);
    }

    #[test]
    fn filter_allowed_names_blocks_tools_not_in_list() {
        let registry = default_writing_tool_registry();
        let policy = PermissionPolicy::new(PermissionMode::WorkspaceWrite);
        let inventory = registry.effective_inventory(
            &ToolFilter {
                intent: None,
                include_requires_approval: true,
                include_disabled: false,
                max_side_effect_level: Some(ToolSideEffectLevel::Write),
                required_tags: Vec::new(),
                allowed_names: vec![
                    "load_current_chapter".to_string(),
                    "search_lorebook".to_string(),
                ],
            },
            &policy,
        );
        assert!(inventory
            .allowed
            .iter()
            .any(|t| t.name == "load_current_chapter"));
        assert!(inventory
            .allowed
            .iter()
            .any(|t| t.name == "search_lorebook"));
        assert!(!inventory
            .allowed
            .iter()
            .any(|t| t.name == "generate_chapter_draft"));
        let blocked = inventory
            .blocked
            .iter()
            .find(|e| e.descriptor.name == "generate_chapter_draft")
            .expect("generate_chapter_draft should be blocked");
        assert_eq!(blocked.status, EffectiveToolStatus::IntentMismatch);
    }

    #[test]
    fn filter_empty_allowed_names_allows_all() {
        let registry = default_writing_tool_registry();
        let policy = PermissionPolicy::new(PermissionMode::WorkspaceWrite);
        let inventory = registry.effective_inventory(
            &ToolFilter {
                intent: None,
                include_requires_approval: true,
                include_disabled: false,
                max_side_effect_level: Some(ToolSideEffectLevel::Write),
                required_tags: Vec::new(),
                allowed_names: Vec::new(),
            },
            &policy,
        );
        assert!(
            inventory
                .allowed
                .iter()
                .any(|t| t.name == "load_current_chapter"),
            "Empty allowed_names should not block any tool"
        );
    }

    #[test]
    fn filter_tools_by_allowed_list_returns_only_whitelisted() {
        let registry = default_writing_tool_registry();
        let tools = registry.list();
        let filtered = filter_tools_by_allowed_list(
            &tools,
            &vec![
                "load_current_chapter".to_string(),
                "search_lorebook".to_string(),
            ],
        );
        assert_eq!(filtered.len(), 2);
        assert!(filtered.iter().any(|t| t.name == "load_current_chapter"));
        assert!(filtered.iter().any(|t| t.name == "search_lorebook"));
        assert!(!filtered.iter().any(|t| t.name == "generate_chapter_draft"));
    }

    #[test]
    fn filter_tools_by_allowed_list_empty_passes_all() {
        let registry = default_writing_tool_registry();
        let tools = registry.list();
        let filtered = filter_tools_by_allowed_list(&tools, &vec![]);
        assert_eq!(filtered.len(), tools.len());
    }
}
