use serde::{Deserialize, Serialize};

use crate::{router::Intent, tool_registry::ToolStage};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DomainCapability {
    pub id: String,
    pub label: String,
    pub description: String,
    pub stage: ToolStage,
    pub intents: Vec<Intent>,
    pub context_sources: Vec<String>,
    pub quality_checks: Vec<String>,
    pub priority: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextPriority {
    pub source_type: String,
    pub priority: u8,
    pub max_chars: usize,
    pub required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentDomainProfile {
    pub id: String,
    pub name: String,
    pub description: String,
    pub capabilities: Vec<DomainCapability>,
    pub context_priorities: Vec<ContextPriority>,
    pub quality_gates: Vec<String>,
}

impl AgentDomainProfile {
    pub fn capabilities_for_intent(&self, intent: &Intent) -> Vec<&DomainCapability> {
        self.capabilities
            .iter()
            .filter(|capability| capability.intents.contains(intent))
            .collect()
    }
}

pub fn writing_domain_profile() -> AgentDomainProfile {
    use Intent::{AnalyzeText, Chat, ExecutePlan, GenerateContent, Linter, RetrieveKnowledge};
    use ToolStage::{Context, Execute, Observe, Plan, Reflect};

    AgentDomainProfile {
        id: "longform_writing".to_string(),
        name: "Longform Writing Agent".to_string(),
        description: "Domain profile for a Cursor-style agent optimized around novel drafting, revision, continuity, and project-grounded context.".to_string(),
        capabilities: vec![
            DomainCapability {
                id: "intent_routing".to_string(),
                label: "Writing intent routing".to_string(),
                description: "Classify whether the user needs drafting, revision, analysis, retrieval, or plan execution before selecting tools.".to_string(),
                stage: Observe,
                intents: vec![Chat, RetrieveKnowledge, AnalyzeText, GenerateContent, ExecutePlan, Linter],
                context_sources: vec!["editor_state".to_string(), "selection".to_string()],
                quality_checks: vec!["avoid unnecessary provider calls".to_string()],
                priority: 10,
            },
            DomainCapability {
                id: "continuity_grounding".to_string(),
                label: "Continuity grounding".to_string(),
                description: "Ground suggestions in outline, lorebook, adjacent chapters, and project brain before inventing story facts.".to_string(),
                stage: Context,
                intents: vec![RetrieveKnowledge, AnalyzeText, GenerateContent, ExecutePlan, Linter],
                context_sources: vec![
                    "outline".to_string(),
                    "lorebook".to_string(),
                    "project_brain".to_string(),
                    "adjacent_chapters".to_string(),
                ],
                quality_checks: vec![
                    "named entities checked against lore".to_string(),
                    "chapter goal stays aligned with outline".to_string(),
                ],
                priority: 9,
            },
            DomainCapability {
                id: "revision_planning".to_string(),
                label: "Revision planning".to_string(),
                description: "Plan bounded edits with target ranges, intent, and conflict risk before replacing user prose.".to_string(),
                stage: Plan,
                intents: vec![AnalyzeText, GenerateContent],
                context_sources: vec!["selection".to_string(), "current_paragraph".to_string()],
                quality_checks: vec![
                    "selected text preserved unless replacement is explicit".to_string(),
                    "edit scope remains bounded".to_string(),
                ],
                priority: 8,
            },
            DomainCapability {
                id: "bounded_drafting".to_string(),
                label: "Bounded drafting".to_string(),
                description: "Generate continuations or chapter drafts under explicit context, output, and save-mode budgets.".to_string(),
                stage: Execute,
                intents: vec![GenerateContent, ExecutePlan],
                context_sources: vec![
                    "current_chapter".to_string(),
                    "outline".to_string(),
                    "lorebook".to_string(),
                    "user_profile".to_string(),
                ],
                quality_checks: vec![
                    "draft stays prose-only when requested".to_string(),
                    "writes require approval or conflict checks".to_string(),
                ],
                priority: 8,
            },
            DomainCapability {
                id: "ambient_editor_assist".to_string(),
                label: "Ambient editor assist".to_string(),
                description: "Provide low-latency suggestions and semantic warnings without interrupting active typing.".to_string(),
                stage: Execute,
                intents: vec![GenerateContent, Linter],
                context_sources: vec!["editor_window".to_string(), "lorebook".to_string()],
                quality_checks: vec![
                    "respect idle and snooze policy".to_string(),
                    "return short actionable suggestions".to_string(),
                ],
                priority: 7,
            },
            DomainCapability {
                id: "trajectory_feedback".to_string(),
                label: "Trajectory feedback".to_string(),
                description: "Record observable run events so future compaction and writing-quality feedback can be built on real traces.".to_string(),
                stage: Reflect,
                intents: vec![Chat, RetrieveKnowledge, AnalyzeText, GenerateContent, ExecutePlan, Linter],
                context_sources: vec!["run_trace".to_string()],
                quality_checks: vec!["tool and context decisions remain inspectable".to_string()],
                priority: 6,
            },
        ],
        context_priorities: vec![
            ContextPriority {
                source_type: "current_editor_window".to_string(),
                priority: 10,
                max_chars: 2_000,
                required: true,
            },
            ContextPriority {
                source_type: "selected_text".to_string(),
                priority: 10,
                max_chars: 2_000,
                required: false,
            },
            ContextPriority {
                source_type: "outline".to_string(),
                priority: 9,
                max_chars: 6_000,
                required: false,
            },
            ContextPriority {
                source_type: "lorebook".to_string(),
                priority: 9,
                max_chars: 5_000,
                required: false,
            },
            ContextPriority {
                source_type: "adjacent_chapters".to_string(),
                priority: 8,
                max_chars: 7_000,
                required: false,
            },
            ContextPriority {
                source_type: "project_brain".to_string(),
                priority: 7,
                max_chars: 4_000,
                required: false,
            },
            ContextPriority {
                source_type: "user_profile".to_string(),
                priority: 6,
                max_chars: 2_000,
                required: false,
            },
        ],
        quality_gates: vec![
            "Do not invent named character or setting facts before retrieval when project context exists.".to_string(),
            "Keep proactive suggestions short, interruptible, and tied to the current cursor context.".to_string(),
            "Treat write operations as approval-required or conflict-checked.".to_string(),
            "Prefer bounded edits over broad rewrites unless the user explicitly asks for a rewrite.".to_string(),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writing_profile_contains_core_vertical_capabilities() {
        let profile = writing_domain_profile();
        assert!(profile
            .capabilities
            .iter()
            .any(|capability| capability.id == "continuity_grounding"));
        assert!(profile
            .capabilities
            .iter()
            .any(|capability| capability.id == "bounded_drafting"));
        assert!(profile.context_priorities.iter().any(|priority| {
            priority.source_type == "current_editor_window" && priority.required
        }));
    }

    #[test]
    fn profile_filters_capabilities_by_intent() {
        let profile = writing_domain_profile();
        let capabilities = profile.capabilities_for_intent(&Intent::GenerateContent);
        assert!(capabilities
            .iter()
            .any(|capability| capability.id == "bounded_drafting"));
        assert!(capabilities
            .iter()
            .any(|capability| capability.id == "continuity_grounding"));
    }
}
