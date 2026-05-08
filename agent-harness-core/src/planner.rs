use serde::{Deserialize, Serialize};

use crate::task_packet::TaskPacket;

/// A single step in a plan
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanStep {
    pub step: u32,
    pub action: String,
    pub description: String,
    pub query: Option<String>,
    pub focus: Option<String>,
    pub style: Option<String>,
}

/// Complete execution plan
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionPlan {
    pub steps: Vec<PlanStep>,
    pub goal: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_packet: Option<TaskPacket>,
}

impl ExecutionPlan {
    /// Parse LLM JSON output, with retry-friendly error messages
    pub fn from_json(json: &str) -> Result<Self, String> {
        serde_json::from_str::<ExecutionPlan>(json).map_err(|e| format!("Plan parse error: {}", e))
    }

    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }

    pub fn total_steps(&self) -> usize {
        self.steps.len()
    }

    pub fn from_task_packet(packet: TaskPacket) -> Result<Self, String> {
        packet
            .validate()
            .map_err(|error| format!("Task packet validation error: {}", error))?;

        let mut steps = Vec::new();
        steps.push(PlanStep {
            step: 1,
            action: "observe_contract".to_string(),
            description: format!(
                "Read task objective, scope, constraints, and success criteria for {}.",
                packet.scope_label()
            ),
            query: None,
            focus: Some(packet.objective.clone()),
            style: None,
        });

        for context in packet
            .required_context
            .iter()
            .filter(|context| context.required)
        {
            steps.push(PlanStep {
                step: steps.len() as u32 + 1,
                action: "load_required_context".to_string(),
                description: format!(
                    "Load {} for {} within {} chars.",
                    context.source_type, context.purpose, context.max_chars
                ),
                query: Some(context.source_type.clone()),
                focus: Some(context.purpose.clone()),
                style: None,
            });
        }

        steps.push(PlanStep {
            step: steps.len() as u32 + 1,
            action: "execute_with_tool_boundary".to_string(),
            description: format!(
                "Execute under {:?} side-effect ceiling with tags [{}].",
                packet.tool_policy.max_side_effect_level,
                packet.tool_policy.required_tool_tags.join(", ")
            ),
            query: None,
            focus: Some(packet.objective.clone()),
            style: None,
        });

        steps.push(PlanStep {
            step: steps.len() as u32 + 1,
            action: "capture_feedback".to_string(),
            description: format!(
                "Check signals [{}] and checkpoints [{}].",
                packet.feedback.expected_signals.join(", "),
                packet.feedback.checkpoints.join(", ")
            ),
            query: None,
            focus: Some(packet.success_criteria.join(" | ")),
            style: None,
        });

        Ok(Self {
            goal: packet.objective.clone(),
            steps,
            task_packet: Some(packet),
        })
    }
}

/// State machine for plan execution
#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum PlanState {
    Idle,
    Planning,
    Executing {
        current: u32,
        total: u32,
        description: String,
    },
    Completed,
    Failed {
        error: String,
    },
}
