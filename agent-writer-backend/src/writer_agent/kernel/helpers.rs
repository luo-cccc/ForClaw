//! Operation helpers, validation, and policy functions.
//! Extracted from kernel.rs.

use super::run_loop_ext::WriterAgentTask;
use crate::writer_agent::context::AgentTask;
use crate::writer_agent::memory::{
    ChapterMissionSummary, StoryContractQuality, StoryContractSummary, WriterMemory,
};
use crate::writer_agent::operation::WriterOperation;
use agent_harness_core::{FeedbackContract, ToolFilter, ToolPolicyContract, ToolSideEffectLevel};

pub(crate) fn tool_policy_for_task(task: &AgentTask) -> ToolPolicyContract {
    match task {
        AgentTask::ChapterGeneration => ToolPolicyContract {
            max_side_effect_level: ToolSideEffectLevel::Write,
            allow_approval_required: true,
            required_tool_tags: vec!["generation".to_string()],
        },
        AgentTask::GhostWriting | AgentTask::ManualRequest | AgentTask::InlineRewrite => {
            ToolPolicyContract {
                max_side_effect_level: ToolSideEffectLevel::ProviderCall,
                allow_approval_required: false,
                required_tool_tags: vec!["project".to_string()],
            }
        }
        AgentTask::PlanningReview
        | AgentTask::ContinuityDiagnostic
        | AgentTask::CanonMaintenance
        | AgentTask::ProposalEvaluation => ToolPolicyContract {
            max_side_effect_level: ToolSideEffectLevel::Read,
            allow_approval_required: false,
            required_tool_tags: vec!["project".to_string()],
        },
    }
}

pub fn tool_filter_for_task(task: AgentTask) -> ToolFilter {
    let policy = tool_policy_for_task(&task);
    ToolFilter {
        intent: None,
        include_requires_approval: policy.allow_approval_required,
        include_disabled: false,
        max_side_effect_level: Some(policy.max_side_effect_level),
        required_tags: policy.required_tool_tags,
    }
}

pub(crate) fn approval_required_for_operation(operation: &WriterOperation) -> bool {
    matches!(
        operation,
        WriterOperation::TextInsert { .. }
            | WriterOperation::TextReplace { .. }
            | WriterOperation::CanonUpsertEntity { .. }
            | WriterOperation::CanonUpdateAttribute { .. }
            | WriterOperation::CanonUpsertRule { .. }
            | WriterOperation::PromiseAdd { .. }
            | WriterOperation::PromiseResolve { .. }
            | WriterOperation::PromiseDefer { .. }
            | WriterOperation::PromiseAbandon { .. }
            | WriterOperation::CharacterUpsert { .. }
            | WriterOperation::CharacterStateUpsert { .. }
            | WriterOperation::RelationshipUpsert { .. }
            | WriterOperation::PromiseBindSubject { .. }
            | WriterOperation::StyleUpdatePreference { .. }
            | WriterOperation::StoryContractUpsert { .. }
            | WriterOperation::ChapterMissionUpsert { .. }
            | WriterOperation::OutlineUpdate { .. }
            | WriterOperation::KnowledgeUpsert { .. }
            | WriterOperation::KnowledgeOwnershipUpsert { .. }
            | WriterOperation::IdentityLayerUpsert { .. }
            | WriterOperation::RevealEventRecord { .. }
            | WriterOperation::SceneUpsert { .. }
            | WriterOperation::SceneStateUpsert { .. }
            | WriterOperation::SceneObligationUpsert { .. }
            | WriterOperation::SceneResultRecord { .. }
            | WriterOperation::TimeSliceUpsert { .. }
            | WriterOperation::ChapterTimeMappingUpsert { .. }
            | WriterOperation::TimelineEventRecord { .. }
    )
}

pub(crate) fn operation_is_write_capable(operation: &WriterOperation) -> bool {
    approval_required_for_operation(operation)
}

pub(crate) fn operation_requires_durable_save(operation: &WriterOperation) -> bool {
    operation_is_write_capable(operation)
}

pub(crate) fn operation_has_kernel_durable_save(operation: &WriterOperation) -> bool {
    operation_is_write_capable(operation)
        && !matches!(
            operation,
            WriterOperation::TextInsert { .. }
                | WriterOperation::TextReplace { .. }
                | WriterOperation::OutlineUpdate { .. }
        )
}

pub(crate) fn save_result_is_success(save_result: &str) -> bool {
    let value = save_result.trim().to_ascii_lowercase();
    value.is_empty()
        || value == "saved"
        || value == "ok"
        || value == "success"
        || value.ends_with(":ok")
        || value.contains("save:")
        || value.contains("saved")
}

pub(crate) fn validate_story_contract_summary(contract: &StoryContractSummary) -> Option<String> {
    let quality = contract.quality();
    match quality {
        StoryContractQuality::Missing => {
            Some(
                "Story Contract is empty: fill in at minimum the genre, reader promise, main conflict, and tone contract.".to_string(),
            )
        }
        StoryContractQuality::Vague => {
            let gaps = contract.quality_gaps();
            Some(format!(
                "Story Contract is too vague to guide the writing agent. Key gaps:\n{}",
                gaps.iter()
                    .map(|gap| format!("  - {}", gap))
                    .collect::<Vec<_>>()
                    .join("\n")
            ))
        }
        StoryContractQuality::Usable | StoryContractQuality::Strong => None,
    }
}

pub(crate) fn validate_chapter_mission_summary(mission: &ChapterMissionSummary) -> Option<String> {
    let mut missing = Vec::new();
    for (label, value) in [
        ("chapter_title", mission.chapter_title.as_str()),
        ("mission", mission.mission.as_str()),
        ("must_include", mission.must_include.as_str()),
        ("must_not", mission.must_not.as_str()),
        ("expected_ending", mission.expected_ending.as_str()),
    ] {
        if value.trim().is_empty() {
            missing.push(label);
        }
    }

    if !missing.is_empty() {
        return Some(format!(
            "Chapter Mission is missing required fields: {}",
            missing.join(", ")
        ));
    }

    for (label, value, min_chars) in [
        ("mission", mission.mission.as_str(), 8),
        ("must_include", mission.must_include.as_str(), 6),
        ("must_not", mission.must_not.as_str(), 6),
        ("expected_ending", mission.expected_ending.as_str(), 8),
    ] {
        if value.trim().chars().count() < min_chars {
            return Some(format!(
                "Chapter Mission field '{}' is too vague; write a concrete, checkable statement.",
                label
            ));
        }
    }

    if !is_valid_chapter_mission_status(&mission.status) {
        return Some(format!(
            "Chapter Mission status '{}' is invalid.",
            mission.status
        ));
    }

    match normalize_chapter_mission_status(&mission.status).as_str() {
        "blocked" if mission.blocked_reason.trim().chars().count() < 8 => {
            return Some("Blocked Chapter Mission requires a concrete blocked_reason.".to_string());
        }
        "retired" if mission.retired_history.trim().chars().count() < 8 => {
            return Some(
                "Retired Chapter Mission requires a concrete retired_history.".to_string(),
            );
        }
        _ => {}
    }

    None
}

pub fn ghost_confidence(intent_confidence: f32, memory: &WriterMemory, project_id: &str) -> f64 {
    let base = (intent_confidence.max(0.65f32) as f64).min(0.9);
    let quality = memory
        .get_story_contract(project_id)
        .ok()
        .flatten()
        .map(|contract| contract.quality())
        .unwrap_or(StoryContractQuality::Missing);
    match quality {
        StoryContractQuality::Missing => base * 0.5,
        StoryContractQuality::Vague => base * 0.7,
        StoryContractQuality::Usable => base,
        StoryContractQuality::Strong => (base + 0.05).min(0.95),
    }
}

pub fn task_requires_story_grounding(task: &WriterAgentTask) -> bool {
    matches!(
        task,
        WriterAgentTask::ChapterGeneration
            | WriterAgentTask::GhostWriting
            | WriterAgentTask::InlineRewrite
            | WriterAgentTask::ContinuityDiagnostic
            | WriterAgentTask::PlanningReview
    )
}

pub fn ghost_quality_risks(memory: &WriterMemory, project_id: &str) -> Vec<String> {
    let quality = memory
        .get_story_contract(project_id)
        .ok()
        .flatten()
        .map(|contract| contract.quality())
        .unwrap_or(StoryContractQuality::Missing);
    match quality {
        StoryContractQuality::Missing => vec![
            "Story contract is missing — this continuation has no book-level grounding."
                .to_string(),
        ],
        StoryContractQuality::Vague => vec![
            "Story contract is vague — strengthen it in Settings for more reliable ghost writing."
                .to_string(),
        ],
        _ => vec!["LLM draft should be reviewed before keeping.".to_string()],
    }
}

pub(crate) fn normalize_chapter_mission_status(status: &str) -> String {
    match status.trim() {
        "" | "active" | "in_progress" => "active".to_string(),
        "draft" | "needs_review" | "completed" | "drifted" | "blocked" | "retired" => {
            status.trim().to_string()
        }
        other => other.to_string(),
    }
}

pub(crate) fn is_valid_chapter_mission_status(status: &str) -> bool {
    matches!(
        normalize_chapter_mission_status(status).as_str(),
        "draft" | "active" | "needs_review" | "completed" | "drifted" | "blocked" | "retired"
    )
}

pub(crate) fn operation_kind_label(operation: &WriterOperation) -> &'static str {
    match operation {
        WriterOperation::TextInsert { .. } => "text.insert",
        WriterOperation::TextReplace { .. } => "text.replace",
        WriterOperation::TextAnnotate { .. } => "text.annotate",
        WriterOperation::CanonUpsertEntity { .. } => "canon.upsert_entity",
        WriterOperation::CanonUpdateAttribute { .. } => "canon.update_attribute",
        WriterOperation::CanonUpsertRule { .. } => "canon.upsert_rule",
        WriterOperation::PromiseAdd { .. } => "promise.add",
        WriterOperation::PromiseResolve { .. } => "promise.resolve",
        WriterOperation::PromiseDefer { .. } => "promise.defer",
        WriterOperation::PromiseAbandon { .. } => "promise.abandon",
        WriterOperation::CharacterUpsert { .. } => "character.upsert",
        WriterOperation::CharacterStateUpsert { .. } => "character_state.upsert",
        WriterOperation::RelationshipUpsert { .. } => "relationship.upsert",
        WriterOperation::PromiseBindSubject { .. } => "promise.bind_subject",
        WriterOperation::StyleUpdatePreference { .. } => "style.update_preference",
        WriterOperation::StoryContractUpsert { .. } => "story_contract.upsert",
        WriterOperation::ChapterMissionUpsert { .. } => "chapter_mission.upsert",
        WriterOperation::OutlineUpdate { .. } => "outline.update",
        WriterOperation::KnowledgeUpsert { .. } => "knowledge.upsert",
        WriterOperation::KnowledgeOwnershipUpsert { .. } => "knowledge_ownership.upsert",
        WriterOperation::IdentityLayerUpsert { .. } => "identity_layer.upsert",
        WriterOperation::RevealEventRecord { .. } => "reveal_event.record",
        WriterOperation::SceneUpsert { .. } => "scene.upsert",
        WriterOperation::SceneStateUpsert { .. } => "scene_state.upsert",
        WriterOperation::SceneObligationUpsert { .. } => "scene_obligation.upsert",
        WriterOperation::SceneResultRecord { .. } => "scene_result.record",
        WriterOperation::TimeSliceUpsert { .. } => "time_slice.upsert",
        WriterOperation::ChapterTimeMappingUpsert { .. } => "chapter_time_mapping.upsert",
        WriterOperation::TimelineEventRecord { .. } => "timeline_event.record",
    }
}

pub(crate) fn operation_affected_scope(operation: &WriterOperation) -> Option<String> {
    match operation {
        WriterOperation::TextInsert { chapter, at, .. } => {
            Some(format!("chapter:{}@{}", chapter, at))
        }
        WriterOperation::TextReplace {
            chapter, from, to, ..
        } => Some(format!("chapter:{}:{}-{}", chapter, from, to)),
        WriterOperation::TextAnnotate {
            chapter, from, to, ..
        } => Some(format!("chapter:{}:{}-{}", chapter, from, to)),
        WriterOperation::CanonUpsertEntity { entity } => {
            Some(format!("canon:{}:{}", entity.kind, entity.name))
        }
        WriterOperation::CanonUpdateAttribute {
            entity, attribute, ..
        } => Some(format!("canon:{}:{}", entity, attribute)),
        WriterOperation::CanonUpsertRule { rule } => Some(format!("canon_rule:{}", rule.category)),
        WriterOperation::PromiseAdd { promise } => Some(format!("promise:new:{}", promise.title)),
        WriterOperation::PromiseResolve {
            promise_id,
            chapter,
        }
        | WriterOperation::PromiseDefer {
            promise_id,
            chapter,
            ..
        }
        | WriterOperation::PromiseAbandon {
            promise_id,
            chapter,
            ..
        } => Some(format!("promise:{}:{}", promise_id, chapter)),
        WriterOperation::CharacterUpsert { name, .. } => Some(format!("canon:character:{}", name)),
        WriterOperation::CharacterStateUpsert { character_id, .. } => {
            Some(format!("canon:character_state:{}", character_id))
        }
        WriterOperation::RelationshipUpsert {
            character_a_id,
            character_b_id,
            ..
        } => Some(format!(
            "canon:relationship:{}-{}",
            character_a_id, character_b_id
        )),
        WriterOperation::PromiseBindSubject { promise_id, .. } => {
            Some(format!("promise:bind_subject:{}", promise_id))
        }
        WriterOperation::StyleUpdatePreference { key, .. } => Some(format!("style:{}", key)),
        WriterOperation::StoryContractUpsert { contract } => {
            Some(format!("story_contract:{}", contract.project_id))
        }
        WriterOperation::ChapterMissionUpsert { mission } => Some(format!(
            "chapter_mission:{}:{}",
            mission.project_id, mission.chapter_title
        )),
        WriterOperation::OutlineUpdate { node_id, .. } => Some(format!("outline:{}", node_id)),
        WriterOperation::KnowledgeUpsert { topic, .. } => {
            Some(format!("canon:knowledge:{}", topic))
        }
        WriterOperation::KnowledgeOwnershipUpsert { knowledge_id, .. } => {
            Some(format!("canon:knowledge_ownership:{}", knowledge_id))
        }
        WriterOperation::IdentityLayerUpsert { character_id, .. } => {
            Some(format!("canon:identity_layer:{}", character_id))
        }
        WriterOperation::RevealEventRecord { subject_id, .. } => {
            Some(format!("canon:reveal_event:{}", subject_id))
        }
        WriterOperation::SceneUpsert {
            chapter_title,
            sequence,
            ..
        } => Some(format!("scene:{}:{}", chapter_title, sequence)),
        WriterOperation::SceneStateUpsert { scene_id, .. } => {
            Some(format!("scene:state:{}", scene_id))
        }
        WriterOperation::SceneObligationUpsert { scene_id, .. } => {
            Some(format!("scene:obligation:{}", scene_id))
        }
        WriterOperation::SceneResultRecord { scene_id, .. } => {
            Some(format!("scene:result:{}", scene_id))
        }
        WriterOperation::TimeSliceUpsert { label, .. } => Some(format!("timeline:{}", label)),
        WriterOperation::ChapterTimeMappingUpsert { chapter_title, .. } => {
            Some(format!("timeline:chapter_time_mapping:{}", chapter_title))
        }
        WriterOperation::TimelineEventRecord {
            event_type,
            time_slice_id,
            ..
        } => Some(format!("timeline:event:{}:{}", event_type, time_slice_id)),
    }
}

pub(crate) fn approval_sources(context: &super::operation::OperationApproval) -> Vec<String> {
    let mut sources = vec![format!("approval:{}", context.source)];
    if let Some(proposal_id) = context
        .proposal_id
        .as_ref()
        .filter(|id| !id.trim().is_empty())
    {
        sources.push(format!("proposal:{}", proposal_id));
    }
    sources
}

pub(crate) fn feedback_contract_for_task(task: &AgentTask) -> FeedbackContract {
    match task {
        AgentTask::GhostWriting => FeedbackContract {
            expected_signals: vec![
                "ghost accepted".to_string(),
                "ghost rejected".to_string(),
                "author typed past ghost".to_string(),
            ],
            checkpoints: vec![
                "record proposal trace".to_string(),
                "record context budget trace".to_string(),
            ],
            memory_writes: vec!["style preference from feedback".to_string()],
        },
        AgentTask::ManualRequest => FeedbackContract {
            expected_signals: vec![
                "manual response completed".to_string(),
                "author follow-up".to_string(),
            ],
            checkpoints: vec![
                "record manual turn".to_string(),
                "record creative decision".to_string(),
            ],
            memory_writes: vec![
                "manual_agent_turn".to_string(),
                "creative_decision".to_string(),
            ],
        },
        AgentTask::ChapterGeneration => FeedbackContract {
            expected_signals: vec![
                "chapter saved".to_string(),
                "save conflict".to_string(),
                "chapter result snapshot".to_string(),
            ],
            checkpoints: vec![
                "record context sources".to_string(),
                "record result feedback after save".to_string(),
            ],
            memory_writes: vec!["chapter_result_summary".to_string()],
        },
        AgentTask::PlanningReview => FeedbackContract {
            expected_signals: vec![
                "planning review completed".to_string(),
                "author confirmation requested".to_string(),
                "author selected next action".to_string(),
            ],
            checkpoints: vec![
                "record planning evidence in run trace".to_string(),
                "surface questions without mutating memory".to_string(),
            ],
            memory_writes: Vec::new(),
        },
        _ => FeedbackContract {
            expected_signals: vec!["proposal accepted/rejected".to_string()],
            checkpoints: vec!["record proposal trace".to_string()],
            memory_writes: vec!["creative_decision".to_string()],
        },
    }
}
