use std::fmt::{Display, Formatter};

use serde::{Deserialize, Serialize};

use crate::router::Intent;
use crate::tool_registry::{ToolFilter, ToolSideEffectLevel};

const MAX_OBJECTIVE_CHARS: usize = 600;
const MAX_LIST_ITEMS: usize = 24;
const MAX_ITEM_CHARS: usize = 500;

/// Machine-readable task contract for one agent action.
///
/// This is the foundation layer between a vague user request and concrete
/// planning, retrieval, tool exposure, and feedback capture.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskPacket {
    pub id: String,
    pub objective: String,
    pub scope: TaskScope,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope_ref: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub intent: Option<Intent>,
    pub constraints: Vec<String>,
    pub success_criteria: Vec<String>,
    pub beliefs: Vec<TaskBelief>,
    pub required_context: Vec<RequiredContext>,
    pub tool_policy: ToolPolicyContract,
    pub feedback: FeedbackContract,
    pub created_at_ms: u64,
}

impl TaskPacket {
    pub fn new(
        id: impl Into<String>,
        objective: impl Into<String>,
        scope: TaskScope,
        created_at_ms: u64,
    ) -> Self {
        Self {
            id: id.into(),
            objective: objective.into(),
            scope,
            scope_ref: None,
            intent: None,
            constraints: Vec::new(),
            success_criteria: Vec::new(),
            beliefs: Vec::new(),
            required_context: Vec::new(),
            tool_policy: ToolPolicyContract::default(),
            feedback: FeedbackContract::default(),
            created_at_ms,
        }
    }

    pub fn validate(&self) -> Result<(), TaskPacketValidationError> {
        let mut errors = Vec::new();

        validate_required("id", &self.id, &mut errors);
        validate_required("objective", &self.objective, &mut errors);
        validate_max_chars(
            "objective",
            &self.objective,
            MAX_OBJECTIVE_CHARS,
            &mut errors,
        );

        if self.scope.requires_ref()
            && self
                .scope_ref
                .as_deref()
                .is_none_or(|value| value.trim().is_empty())
        {
            errors.push(format!("scopeRef is required for {} scope", self.scope));
        }

        validate_string_list(
            "constraints",
            &self.constraints,
            MAX_LIST_ITEMS,
            MAX_ITEM_CHARS,
            &mut errors,
        );
        validate_string_list(
            "successCriteria",
            &self.success_criteria,
            MAX_LIST_ITEMS,
            MAX_ITEM_CHARS,
            &mut errors,
        );

        if self.success_criteria.is_empty() {
            errors.push("successCriteria must include at least one acceptance check".to_string());
        }

        validate_beliefs(&self.beliefs, &mut errors);
        validate_required_context(&self.required_context, &mut errors);
        self.feedback.validate(&mut errors);

        let coverage = self.foundation_coverage();
        for missing in coverage.missing {
            errors.push(format!("foundation gap: {missing}"));
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(TaskPacketValidationError::new(errors))
        }
    }

    pub fn foundation_coverage(&self) -> FoundationCoverage {
        let reasoning_and_planning =
            !self.objective.trim().is_empty() && !self.success_criteria.is_empty();
        let memory = !self.beliefs.is_empty()
            || self
                .required_context
                .iter()
                .any(|context| context.required && !context.source_type.trim().is_empty());
        let action_loop = self.tool_policy.has_explicit_boundary();
        let goal_and_belief = !self.objective.trim().is_empty()
            && (!self.constraints.is_empty() || !self.beliefs.is_empty());
        let environment_and_feedback = self.feedback.has_feedback_loop();

        let mut missing = Vec::new();
        if !reasoning_and_planning {
            missing.push("reasoning_and_planning requires objective plus success criteria".into());
        }
        if !memory {
            missing.push("memory requires beliefs or required context sources".into());
        }
        if !action_loop {
            missing.push("action_loop requires an explicit tool boundary".into());
        }
        if !goal_and_belief {
            missing.push("goal_and_belief requires constraints or current beliefs".into());
        }
        if !environment_and_feedback {
            missing
                .push("environment_and_feedback requires feedback signals and checkpoints".into());
        }

        FoundationCoverage {
            reasoning_and_planning,
            memory,
            action_loop,
            goal_and_belief,
            environment_and_feedback,
            missing,
        }
    }

    pub fn to_tool_filter(&self, fallback_intent: Option<Intent>) -> ToolFilter {
        ToolFilter {
            intent: self.intent.clone().or(fallback_intent),
            include_requires_approval: self.tool_policy.allow_approval_required,
            include_disabled: false,
            max_side_effect_level: Some(self.tool_policy.max_side_effect_level),
            required_tags: self.tool_policy.required_tool_tags.clone(),
        }
    }

    pub fn scope_label(&self) -> String {
        self.scope_ref
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .map(|scope_ref| format!("{}: {}", self.scope, scope_ref))
            .unwrap_or_else(|| self.scope.to_string())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskScope {
    Project,
    Book,
    Chapter,
    Scene,
    Selection,
    CursorWindow,
    Custom,
}

impl TaskScope {
    pub fn requires_ref(self) -> bool {
        matches!(
            self,
            Self::Chapter | Self::Scene | Self::Selection | Self::CursorWindow | Self::Custom
        )
    }
}

impl Display for TaskScope {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let label = match self {
            Self::Project => "project",
            Self::Book => "book",
            Self::Chapter => "chapter",
            Self::Scene => "scene",
            Self::Selection => "selection",
            Self::CursorWindow => "cursor-window",
            Self::Custom => "custom",
        };
        write!(f, "{label}")
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskBelief {
    pub subject: String,
    pub statement: String,
    pub confidence: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

impl TaskBelief {
    pub fn new(subject: impl Into<String>, statement: impl Into<String>, confidence: f32) -> Self {
        Self {
            subject: subject.into(),
            statement: statement.into(),
            confidence,
            source: None,
        }
    }

    pub fn with_source(mut self, source: impl Into<String>) -> Self {
        self.source = Some(source.into());
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequiredContext {
    pub source_type: String,
    pub purpose: String,
    pub max_chars: usize,
    pub required: bool,
}

impl RequiredContext {
    pub fn new(
        source_type: impl Into<String>,
        purpose: impl Into<String>,
        max_chars: usize,
        required: bool,
    ) -> Self {
        Self {
            source_type: source_type.into(),
            purpose: purpose.into(),
            max_chars,
            required,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolPolicyContract {
    pub max_side_effect_level: ToolSideEffectLevel,
    pub allow_approval_required: bool,
    pub required_tool_tags: Vec<String>,
}

impl ToolPolicyContract {
    pub fn has_explicit_boundary(&self) -> bool {
        self.max_side_effect_level <= ToolSideEffectLevel::External
            || !self.allow_approval_required
            || !self.required_tool_tags.is_empty()
    }
}

impl Default for ToolPolicyContract {
    fn default() -> Self {
        Self {
            max_side_effect_level: ToolSideEffectLevel::ProviderCall,
            allow_approval_required: false,
            required_tool_tags: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedbackContract {
    pub expected_signals: Vec<String>,
    pub checkpoints: Vec<String>,
    pub memory_writes: Vec<String>,
}

impl FeedbackContract {
    pub fn has_feedback_loop(&self) -> bool {
        !self.expected_signals.is_empty() && !self.checkpoints.is_empty()
    }

    fn validate(&self, errors: &mut Vec<String>) {
        validate_string_list(
            "feedback.expectedSignals",
            &self.expected_signals,
            MAX_LIST_ITEMS,
            MAX_ITEM_CHARS,
            errors,
        );
        validate_string_list(
            "feedback.checkpoints",
            &self.checkpoints,
            MAX_LIST_ITEMS,
            MAX_ITEM_CHARS,
            errors,
        );
        validate_string_list(
            "feedback.memoryWrites",
            &self.memory_writes,
            MAX_LIST_ITEMS,
            MAX_ITEM_CHARS,
            errors,
        );
        if self.expected_signals.is_empty() {
            errors.push("feedback.expectedSignals must include at least one signal".to_string());
        }
        if self.checkpoints.is_empty() {
            errors.push("feedback.checkpoints must include at least one checkpoint".to_string());
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FoundationCoverage {
    pub reasoning_and_planning: bool,
    pub memory: bool,
    pub action_loop: bool,
    pub goal_and_belief: bool,
    pub environment_and_feedback: bool,
    pub missing: Vec<String>,
}

impl FoundationCoverage {
    pub fn is_complete(&self) -> bool {
        self.reasoning_and_planning
            && self.memory
            && self.action_loop
            && self.goal_and_belief
            && self.environment_and_feedback
            && self.missing.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskPacketValidationError {
    errors: Vec<String>,
}

impl TaskPacketValidationError {
    pub fn new(errors: Vec<String>) -> Self {
        Self { errors }
    }

    pub fn errors(&self) -> &[String] {
        &self.errors
    }
}

impl Display for TaskPacketValidationError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.errors.join("; "))
    }
}

impl std::error::Error for TaskPacketValidationError {}

fn validate_required(field: &str, value: &str, errors: &mut Vec<String>) {
    if value.trim().is_empty() {
        errors.push(format!("{field} must not be empty"));
    }
}

fn validate_max_chars(field: &str, value: &str, max_chars: usize, errors: &mut Vec<String>) {
    let count = value.chars().count();
    if count > max_chars {
        errors.push(format!(
            "{field} exceeds max length: {count} > {max_chars} chars"
        ));
    }
}

fn validate_string_list(
    field: &str,
    values: &[String],
    max_items: usize,
    max_chars: usize,
    errors: &mut Vec<String>,
) {
    if values.len() > max_items {
        errors.push(format!(
            "{field} has too many items: {} > {max_items}",
            values.len()
        ));
    }
    for (index, value) in values.iter().enumerate() {
        if value.trim().is_empty() {
            errors.push(format!("{field} contains an empty value at index {index}"));
        }
        validate_max_chars(&format!("{field}[{index}]"), value, max_chars, errors);
    }
}

fn validate_beliefs(beliefs: &[TaskBelief], errors: &mut Vec<String>) {
    if beliefs.is_empty() {
        errors.push("beliefs must include at least one current belief".to_string());
    }
    if beliefs.len() > MAX_LIST_ITEMS {
        errors.push(format!(
            "beliefs has too many items: {} > {MAX_LIST_ITEMS}",
            beliefs.len()
        ));
    }
    for (index, belief) in beliefs.iter().enumerate() {
        validate_required(
            &format!("beliefs[{index}].subject"),
            &belief.subject,
            errors,
        );
        validate_required(
            &format!("beliefs[{index}].statement"),
            &belief.statement,
            errors,
        );
        if !belief.confidence.is_finite() || !(0.0..=1.0).contains(&belief.confidence) {
            errors.push(format!(
                "beliefs[{index}].confidence must be between 0.0 and 1.0"
            ));
        }
    }
}

fn validate_required_context(contexts: &[RequiredContext], errors: &mut Vec<String>) {
    if !contexts.iter().any(|context| context.required) {
        errors.push("requiredContext must include at least one required source".to_string());
    }
    if contexts.len() > MAX_LIST_ITEMS {
        errors.push(format!(
            "requiredContext has too many items: {} > {MAX_LIST_ITEMS}",
            contexts.len()
        ));
    }
    for (index, context) in contexts.iter().enumerate() {
        validate_required(
            &format!("requiredContext[{index}].sourceType"),
            &context.source_type,
            errors,
        );
        validate_required(
            &format!("requiredContext[{index}].purpose"),
            &context.purpose,
            errors,
        );
        if context.max_chars == 0 {
            errors.push(format!("requiredContext[{index}].maxChars must be > 0"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_packet() -> TaskPacket {
        let mut packet = TaskPacket::new(
            "task-1",
            "Continue the current chapter while preserving the chapter mission and open promises.",
            TaskScope::Chapter,
            1_000,
        );
        packet.scope_ref = Some("Chapter-7".to_string());
        packet.intent = Some(Intent::GenerateContent);
        packet.constraints = vec![
            "Do not reveal the jade pendant origin early.".to_string(),
            "Keep Lin Mo's dialogue restrained.".to_string(),
        ];
        packet.success_criteria = vec![
            "The continuation advances the interrogation scene.".to_string(),
            "No canon conflict is introduced.".to_string(),
        ];
        packet.beliefs =
            vec![TaskBelief::new("林墨", "林墨惯用寒影刀，不用长剑。", 0.95).with_source("canon")];
        packet.required_context = vec![
            RequiredContext::new(
                "chapter_mission",
                "Keep the active chapter aligned with its promise.",
                700,
                true,
            ),
            RequiredContext::new(
                "promise_ledger",
                "Avoid dropping open story debts.",
                600,
                true,
            ),
        ];
        packet.tool_policy.required_tool_tags = vec!["project".to_string()];
        packet.feedback = FeedbackContract {
            expected_signals: vec![
                "proposal accepted/rejected".to_string(),
                "canon warning emitted".to_string(),
            ],
            checkpoints: vec![
                "trace context sources".to_string(),
                "record result feedback after save".to_string(),
            ],
            memory_writes: vec!["chapter_result_summary".to_string()],
        };
        packet
    }

    #[test]
    fn valid_packet_covers_five_foundation_axes() {
        let packet = sample_packet();
        packet.validate().expect("sample packet should validate");

        let coverage = packet.foundation_coverage();
        assert!(coverage.is_complete());
        assert!(coverage.reasoning_and_planning);
        assert!(coverage.memory);
        assert!(coverage.action_loop);
        assert!(coverage.goal_and_belief);
        assert!(coverage.environment_and_feedback);
    }

    #[test]
    fn invalid_packet_reports_missing_contract_parts() {
        let packet = TaskPacket::new("", " ", TaskScope::Chapter, 1);
        let error = packet.validate().expect_err("packet should be rejected");

        assert!(error
            .errors()
            .iter()
            .any(|message| message.contains("id must not be empty")));
        assert!(error
            .errors()
            .iter()
            .any(|message| message.contains("objective must not be empty")));
        assert!(error
            .errors()
            .iter()
            .any(|message| message.contains("scopeRef is required")));
        assert!(error
            .errors()
            .iter()
            .any(|message| message.contains("successCriteria")));
        assert!(error
            .errors()
            .iter()
            .any(|message| message.contains("foundation gap")));
    }

    #[test]
    fn packet_builds_tool_filter_from_policy() {
        let packet = sample_packet();
        let filter = packet.to_tool_filter(None);

        assert_eq!(filter.intent, Some(Intent::GenerateContent));
        assert!(!filter.include_requires_approval);
        assert_eq!(
            filter.max_side_effect_level,
            Some(ToolSideEffectLevel::ProviderCall)
        );
        assert_eq!(filter.required_tags, vec!["project".to_string()]);
    }

    #[test]
    fn serialization_roundtrip_preserves_task_packet() {
        let packet = sample_packet();
        let json = serde_json::to_string(&packet).expect("packet should serialize");
        let decoded: TaskPacket = serde_json::from_str(&json).expect("packet should deserialize");
        assert_eq!(decoded, packet);
    }
}
