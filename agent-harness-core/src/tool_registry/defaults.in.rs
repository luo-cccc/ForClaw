fn filter_block_reason(
    tool: &ToolDescriptor,
    filter: &ToolFilter,
) -> Option<(EffectiveToolStatus, String)> {
    if !filter.include_disabled && !tool.enabled_by_default {
        return Some((
            EffectiveToolStatus::Disabled,
            format!("Tool '{}' is disabled by default", tool.name),
        ));
    }

    if !filter.include_requires_approval && tool.requires_approval {
        return Some((
            EffectiveToolStatus::ApprovalRequired,
            format!(
                "Tool '{}' requires approval and the filter excludes approval tools",
                tool.name
            ),
        ));
    }

    if let Some(level) = filter.max_side_effect_level {
        if tool.side_effect_level > level {
            return Some((
                EffectiveToolStatus::SideEffectTooHigh,
                format!(
                    "Tool '{}' side effect {:?} exceeds max {:?}",
                    tool.name, tool.side_effect_level, level
                ),
            ));
        }
    }

    if let Some(intent) = filter.intent.as_ref() {
        if !tool.supports_intent(intent) {
            return Some((
                EffectiveToolStatus::IntentMismatch,
                format!("Tool '{}' does not support intent {:?}", tool.name, intent),
            ));
        }
    }

    for tag in &filter.required_tags {
        if !tool.tags.iter().any(|candidate| candidate == tag) {
            return Some((
                EffectiveToolStatus::MissingTag,
                format!("Tool '{}' is missing required tag '{}'", tool.name, tag),
            ));
        }
    }

    None
}

pub fn default_writing_tool_registry() -> ToolRegistry {
    use Intent::{AnalyzeText, Chat, ExecutePlan, GenerateContent, Linter, RetrieveKnowledge};
    use ToolSideEffectLevel::{None, ProviderCall, Read, Write};
    use ToolStage::{Context, Execute, Observe, Plan, Reflect};

    let mut registry = ToolRegistry::new();
    for tool in [
        ToolDescriptor::new(
            "classify_writing_intent",
            "Classify a user request into the writing agent's lightweight intent taxonomy.",
            "writer_request",
            "intent",
            None,
            false,
            100,
            200,
            Observe,
        )
        .with_tags(&["router", "writing"])
        .with_supported_intents(&[
            Chat,
            RetrieveKnowledge,
            AnalyzeText,
            GenerateContent,
            ExecutePlan,
            Linter,
        ]),
        ToolDescriptor::new(
            "load_current_chapter",
            "Load the active chapter text and revision metadata for context-aware writing.",
            "chapter_title",
            "chapter_text",
            Read,
            false,
            500,
            1_800,
            Context,
        )
        .with_tags(&["project", "chapter", "read"])
        .with_supported_intents(&[AnalyzeText, GenerateContent, ExecutePlan, Linter])
        .with_input_schema(single_string_input_schema(
            "chapter",
            "Exact chapter title to load from the project.",
            160,
        )),
        ToolDescriptor::new(
            "load_outline_node",
            "Load the outline summary and status for a target chapter.",
            "chapter_title",
            "outline_node",
            Read,
            false,
            500,
            800,
            Context,
        )
        .with_tags(&["project", "outline", "read"])
        .with_supported_intents(&[AnalyzeText, GenerateContent, ExecutePlan])
        .with_input_schema(single_string_input_schema(
            "chapter",
            "Exact chapter title or outline node id to inspect.",
            160,
        )),
        ToolDescriptor::new(
            "search_lorebook",
            "Search project lore entries before inventing details about named entities or rules.",
            "keywords",
            "lorebook_entries",
            Read,
            false,
            800,
            1_200,
            Context,
        )
        .with_tags(&["project", "lore", "read"])
        .with_supported_intents(&[
            RetrieveKnowledge,
            AnalyzeText,
            GenerateContent,
            ExecutePlan,
            Linter,
        ])
        .with_input_schema(single_string_input_schema(
            "keyword",
            "Character, place, object, rule, or story term to search in the lorebook.",
            160,
        )),
        ToolDescriptor::new(
            "query_project_brain",
            "Run semantic retrieval over the embedded project brain.",
            "semantic_query",
            "rag_snippets",
            ProviderCall,
            false,
            2_500,
            1_500,
            Context,
        )
        .with_tags(&["project", "rag", "provider"])
        .with_supported_intents(&[RetrieveKnowledge, AnalyzeText, GenerateContent, ExecutePlan])
        .with_input_schema(single_string_input_schema(
            "query",
            "Focused semantic query for project memory and embedded notes.",
            400,
        )),
        ToolDescriptor::new(
            "read_user_drift_profile",
            "Read learned user preferences and durable writing profile entries.",
            "none",
            "preference_entries",
            Read,
            false,
            500,
            800,
            Context,
        )
        .with_tags(&["memory", "preference", "read"])
        .with_supported_intents(&[Chat, AnalyzeText, GenerateContent, ExecutePlan]),
        ToolDescriptor::new(
            "load_domain_profile",
            "Load the active domain capability profile for tool selection, context priority, and quality gates.",
            "domain_id",
            "domain_profile",
            Read,
            false,
            700,
            1_200,
            Context,
        )
        .with_tags(&["domain", "capability", "read"])
        .with_supported_intents(&[Chat, AnalyzeText, GenerateContent, ExecutePlan, Linter]),
        ToolDescriptor::new(
            "pack_agent_context",
            "Assemble named context sources under a strict character budget with truncation reports.",
            "context_sources",
            "packed_context",
            None,
            false,
            100,
            0,
            Context,
        )
        .with_tags(&["context", "budget", "internal"])
        .with_supported_intents(&[Chat, RetrieveKnowledge, AnalyzeText, GenerateContent, ExecutePlan, Linter]),
        ToolDescriptor::new(
            "plan_chapter_task",
            "Break a writing request into observable plan steps before executing edits or generation.",
            "writer_goal",
            "execution_plan",
            None,
            false,
            300,
            600,
            Plan,
        )
        .with_tags(&["planner", "writing"])
        .with_supported_intents(&[AnalyzeText, GenerateContent, ExecutePlan]),
        ToolDescriptor::new(
            "generate_bounded_continuation",
            "Generate a short context-bounded continuation preview for proactive co-writing.",
            "agent_observation_context",
            "suggestion_preview",
            ProviderCall,
            false,
            6_000,
            2_400,
            Execute,
        )
        .with_tags(&["generation", "preview", "provider"])
        .with_supported_intents(&[GenerateContent])
        .with_input_schema(single_string_input_schema(
            "prompt",
            "Bounded prompt containing the local scene context and requested continuation direction.",
            4_000,
        )),
        ToolDescriptor::new(
            "generate_chapter_draft",
            "Generate and save a full chapter draft using project context and conflict checks.",
            "chapter_generation_payload",
            "saved_chapter",
            Write,
            true,
            120_000,
            12_000,
            Execute,
        )
        .with_tags(&["generation", "chapter", "write"])
        .with_supported_intents(&[GenerateContent, ExecutePlan]),
        ToolDescriptor::new(
            "record_run_trace",
            "Record agent run events for UI status, debugging, and future trajectory compression.",
            "run_event",
            "run_trace",
            None,
            false,
            50,
            0,
            Reflect,
        )
        .with_tags(&["trace", "observability", "internal"])
        .with_supported_intents(&[Chat, RetrieveKnowledge, AnalyzeText, GenerateContent, ExecutePlan, Linter]),
    ] {
        registry
            .register(tool)
            .expect("default writing tools are uniquely named");
    }
    registry
}

fn single_string_input_schema(
    field: &str,
    description: &str,
    max_length: usize,
) -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            field: {
                "type": "string",
                "description": description,
                "minLength": 1,
                "maxLength": max_length,
            },
        },
        "required": [field],
        "additionalProperties": false,
    })
}
