use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CraftRule {
    pub id: String,
    pub category: String,
    pub name: String,
    pub applies_when: Vec<String>,
    pub instruction: String,
    pub anti_patterns: Vec<String>,
    pub diagnostic_signals: Vec<String>,
    pub revision_hint: String,
    pub token_cost_hint: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CraftRuleSelection {
    pub rule_id: String,
    pub reason: String,
    pub evidence_refs: Vec<String>,
    pub priority: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmpowermentPromptPacket {
    pub craft_rules: Vec<CraftRuleSelection>,
    pub chapter_discipline: Vec<String>,
    pub must_avoid: Vec<String>,
    pub self_checklist: Vec<String>,
    #[serde(default)]
    pub memory_examples: Vec<CraftMemoryPromptExample>,
    #[serde(default)]
    pub memory_bad_patterns: Vec<CraftMemoryPromptBadPattern>,
    pub total_token_estimate: usize,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub scene_contract_prompt: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CraftMemoryPromptSamples {
    pub rule_id: String,
    pub examples: Vec<CraftMemoryPromptExample>,
    pub bad_patterns: Vec<CraftMemoryPromptBadPattern>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CraftMemoryPromptExample {
    pub rule_id: String,
    pub excerpt_ref: String,
    pub excerpt: String,
    pub reason: String,
    pub score_delta: f32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CraftMemoryPromptBadPattern {
    pub rule_id: String,
    pub evidence_ref: String,
    pub evidence_excerpt: String,
    pub correction: String,
    pub rejected_count: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SceneCraftPlan {
    pub scene_id: String,
    pub chapter_title: String,
    pub objective: String,
    pub participants: Vec<String>,
    pub conflict_pressure: ConflictPressure,
    pub character_choice: CharacterChoice,
    pub information_release: Vec<String>,
    pub withheld_information: Vec<String>,
    pub emotional_curve: Vec<EmotionalBeat>,
    pub promise_or_anchor_payoff: Vec<String>,
    pub ending_hook: EndingHook,
    pub selected_craft_rules: Vec<String>,
    pub must_avoid: Vec<String>,
    pub evidence_refs: Vec<String>,
    #[serde(default)]
    pub required_state_deltas: Vec<StateDelta>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StateDelta {
    pub delta_type: String,
    pub description: String,
    pub source: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConflictPressure {
    pub source: String,
    pub escalation: bool,
    pub cost_or_consequence: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CharacterChoice {
    pub character: String,
    pub options: Vec<String>,
    pub cost: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmotionalBeat {
    pub position: String,
    pub emotion: String,
    pub trigger: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EndingHook {
    pub consequence_delivered: String,
    pub question_left_open: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IssueSeverity {
    None,
    Minor,
    Major,
    Fatal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QualityMetricResult {
    pub metric: String,
    pub score: f32,
    pub severity: IssueSeverity,
    pub evidence_excerpt: String,
    pub rule_source: String,
    pub reason: String,
    pub revision_hint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QualityIssue {
    pub metric: String,
    pub severity: IssueSeverity,
    pub evidence: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[derive(Default)]
pub struct ChapterQualityReport {
    pub chapter_title: String,
    pub overall_score: f32,
    pub fatal_issues: Vec<QualityIssue>,
    pub major_issues: Vec<QualityIssue>,
    pub metric_results: Vec<QualityMetricResult>,
    pub top_revision_targets: Vec<String>,
    pub no_fatal_issue: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub world_consistency_violations: Vec<crate::writer_agent::world_bible::WorldConsistencyViolation>,
    /// P15: Structured canon constraint violations with evidence excerpt,
    /// violated rule, and suggested revision direction.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub canon_constraint_violations: Vec<crate::writer_agent::world_bible::CanonConstraintViolation>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RevisionTargetChangeStatus {
    NotAttempted,
    BudgetSkipped,
    NotObserved,
    Improved,
    Unchanged,
    Regressed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SentenceChangeKind {
    Modified,
    Inserted,
    Deleted,
    Moved,
    Unaligned,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SentenceChangeConfidence {
    High,
    Medium,
    Low,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SentenceChange {
    pub before_sentence: String,
    pub after_sentence: String,
    pub change_kind: SentenceChangeKind,
    pub target_metric: String,
    pub confidence: SentenceChangeConfidence,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RevisionTargetChange {
    pub metric: String,
    pub revision_hint: String,
    pub score_before: f32,
    pub score_after: Option<f32>,
    pub delta: Option<f32>,
    pub status: RevisionTargetChangeStatus,
    pub evidence_before: String,
    pub evidence_after: Option<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub changed_excerpt_before: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub changed_excerpt_after: String,
    pub text_change_summary: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sentence_changes: Vec<SentenceChange>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CraftMemoryUpdate {
    pub rule_id: String,
    pub scope: String,
    pub decision: String,
    pub diagnostic_signals: Vec<String>,
    pub matched_metrics: Vec<String>,
    pub score_before: f32,
    pub score_after: f32,
    pub evidence_ref: String,
    pub reason: String,
    #[serde(default)]
    pub example_refs: Vec<String>,
    #[serde(default)]
    pub bad_pattern_refs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RevisionReport {
    pub chapter_title: String,
    pub request_id: String,
    pub triggered: bool,
    pub budget_skipped: bool,
    pub top_issues_before: Vec<String>,
    pub score_before: f32,
    pub score_after: Option<f32>,
    pub accepted: bool,
    pub reason: String,
    #[serde(default)]
    pub target_changes: Vec<RevisionTargetChange>,
    #[serde(default)]
    pub craft_memory_updates: Vec<CraftMemoryUpdate>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManualCraftEditFeedbackRequest {
    pub chapter_title: String,
    pub before_text: String,
    pub after_text: String,
    #[serde(default)]
    pub metrics: Vec<String>,
    #[serde(default)]
    pub anchor_keywords: Vec<String>,
    #[serde(default)]
    pub open_promise_keywords: Vec<String>,
    #[serde(default)]
    pub author_voice: Option<crate::writer_agent::author_voice::AuthorVoiceSnapshot>,
    #[serde(default)]
    pub target_min_chars: Option<usize>,
    #[serde(default)]
    pub target_max_chars: Option<usize>,
    #[serde(default)]
    pub source_ref: Option<String>,
    #[serde(default)]
    pub author_approved: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManualCraftEditFeedbackResult {
    pub chapter_title: String,
    pub source_ref: String,
    pub score_before: f32,
    pub score_after: f32,
    pub target_changes: Vec<RevisionTargetChange>,
    pub craft_memory_updates: Vec<CraftMemoryUpdate>,
    pub example_refs: Vec<String>,
    pub bad_pattern_refs: Vec<String>,
    pub quality_before: ChapterQualityReport,
    pub quality_after: ChapterQualityReport,
}

#[cfg(test)]
mod craft_types_tests {
    use super::*;

    #[test]
    fn deserializes_craft_rule() {
        let json = r#"{
            "id": "test_rule",
            "category": "prose",
            "name": "测试规则",
            "appliesWhen": ["chapter_draft"],
            "instruction": "测试指令",
            "antiPatterns": ["反模式1"],
            "diagnosticSignals": ["signal1"],
            "revisionHint": "修改建议",
            "tokenCostHint": 100
        }"#;
        let rule: CraftRule = serde_json::from_str(json).expect("should deserialize");
        assert_eq!(rule.id, "test_rule");
        assert_eq!(rule.token_cost_hint, 100);
    }

    #[test]
    fn craft_library_json_is_valid() {
        let json = include_str!("../../../config/craft-library.json");
        let rules: Vec<CraftRule> =
            serde_json::from_str(json).expect("craft-library.json must be valid");
        assert_eq!(rules.len(), 8, "expected 8 craft rules");
        let ids: Vec<&str> = rules.iter().map(|r| r.id.as_str()).collect();
        assert!(ids.contains(&"scene_objective"));
        assert!(ids.contains(&"ending_hook"));
    }
}
