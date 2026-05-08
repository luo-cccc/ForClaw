use agent_harness_core::{
    default_writing_tool_registry, EffectiveToolInventory, PermissionMode, PermissionPolicy,
    ToolDescriptor, ToolFilter, ToolSideEffectLevel,
};
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AgentMode {
    Off,
    Passive,
    Proactive,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AgentObservationReason {
    UserTyped,
    SelectionChange,
    ChapterSwitch,
    IdleTick,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AgentTextRange {
    pub from: usize,
    pub to: usize,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSelection {
    pub from: usize,
    pub to: usize,
    pub text: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentObservation {
    pub id: String,
    pub mode: AgentMode,
    pub reason: AgentObservationReason,
    pub created_at: u64,
    pub chapter_title: Option<String>,
    pub chapter_revision: Option<String>,
    pub dirty: bool,
    pub cursor_position: usize,
    pub selection: Option<AgentSelection>,
    pub current_paragraph: String,
    pub nearby_text: String,
    pub recent_edit_summary: String,
    pub idle_ms: u64,
    pub snoozed_until: Option<u64>,
    pub outline_chapter_title: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AgentSuggestionKind {
    Continue,
    Revise,
    Continuity,
    Lore,
    Structure,
    Question,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentSuggestionAction {
    Accept,
    Reject,
    Snooze,
    Explain,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSourceSummary {
    pub source_type: String,
    pub label: String,
    pub summary: String,
    pub original_chars: usize,
    pub included_chars: usize,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSuggestion {
    pub id: String,
    pub request_id: String,
    pub observation_id: String,
    pub kind: AgentSuggestionKind,
    pub target_range: Option<AgentTextRange>,
    pub anchor_position: Option<usize>,
    pub confidence: f32,
    pub reason: String,
    pub source_summaries: Vec<AgentSourceSummary>,
    pub preview_text: String,
    pub actions: Vec<AgentSuggestionAction>,
    pub created_at: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentObserveResult {
    pub request_id: String,
    pub observation_id: String,
    pub decision: String,
    pub reason: String,
    pub suggestion_id: Option<String>,
}

pub type AgentToolDescriptor = ToolDescriptor;

#[derive(Debug, Clone)]
pub struct AttentionDecision {
    pub should_suggest: bool,
    pub kind: AgentSuggestionKind,
    pub reason: String,
    pub confidence: f32,
}

const MIN_IDLE_MS: u64 = 900;
const MIN_MEANINGFUL_PARAGRAPH_CHARS: usize = 32;
const MIN_SELECTION_CHARS: usize = 6;
const CONTINUITY_MARKERS: [&str; 7] = ["但是", "可是", "明明", "忽然", "突然", "不可能", "矛盾"];

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn char_count(text: &str) -> usize {
    text.chars().count()
}

pub fn attention_policy(observation: &AgentObservation, now: u64) -> AttentionDecision {
    if observation.mode == AgentMode::Off {
        return noop("proactive agent disabled");
    }

    if observation.mode == AgentMode::Passive {
        return noop("agent is in passive trace mode");
    }

    if observation
        .snoozed_until
        .is_some_and(|snoozed_until| snoozed_until > now)
    {
        return noop("suggestions are snoozed");
    }

    if observation.idle_ms < MIN_IDLE_MS {
        return noop("user is still typing");
    }

    let paragraph = observation.current_paragraph.trim();

    if observation.reason == AgentObservationReason::ChapterSwitch
        && observation
            .outline_chapter_title
            .as_deref()
            .is_some_and(|title| !title.trim().is_empty())
    {
        return AttentionDecision {
            should_suggest: true,
            kind: AgentSuggestionKind::Structure,
            reason: "chapter switch can use outline alignment".to_string(),
            confidence: 0.58,
        };
    }

    if paragraph.ends_with('？') || paragraph.ends_with('?') {
        return AttentionDecision {
            should_suggest: true,
            kind: AgentSuggestionKind::Question,
            reason: "current paragraph ends with a question cue".to_string(),
            confidence: 0.56,
        };
    }

    if let Some(selection) = &observation.selection {
        if selection.from < selection.to && char_count(&selection.text) >= MIN_SELECTION_CHARS {
            return AttentionDecision {
                should_suggest: true,
                kind: AgentSuggestionKind::Revise,
                reason: "selected text is stable after a pause".to_string(),
                confidence: 0.72,
            };
        }
    }

    let has_project_context =
        observation.dirty || observation.chapter_revision.is_some() || observation.created_at > 0;
    let lore_cue_present = ["设定", "毒", "密道", "破庙", "传说", "规则"]
        .iter()
        .any(|cue| observation.current_paragraph.contains(cue));
    if has_project_context && lore_cue_present {
        return AttentionDecision {
            should_suggest: true,
            kind: AgentSuggestionKind::Lore,
            reason: format!(
                "paragraph contains a likely lore lookup cue after {}",
                observation.recent_edit_summary
            ),
            confidence: 0.61,
        };
    }

    if CONTINUITY_MARKERS
        .iter()
        .any(|marker| observation.current_paragraph.contains(marker))
    {
        return AttentionDecision {
            should_suggest: true,
            kind: AgentSuggestionKind::Continuity,
            reason: "paragraph contains a possible continuity-risk marker".to_string(),
            confidence: 0.64,
        };
    }

    let paragraph_chars = char_count(paragraph);
    if matches!(
        observation.reason,
        AgentObservationReason::UserTyped | AgentObservationReason::IdleTick
    ) && paragraph_chars >= MIN_MEANINGFUL_PARAGRAPH_CHARS
    {
        return AttentionDecision {
            should_suggest: true,
            kind: AgentSuggestionKind::Continue,
            reason: "meaningful paragraph pause detected".to_string(),
            confidence: 0.68,
        };
    }

    noop("observation below attention threshold")
}

fn noop(reason: &str) -> AttentionDecision {
    AttentionDecision {
        should_suggest: false,
        kind: AgentSuggestionKind::Continue,
        reason: reason.to_string(),
        confidence: 0.0,
    }
}

pub fn build_suggestion(
    observation: &AgentObservation,
    request_id: String,
    decision: &AttentionDecision,
    source_summaries: Vec<AgentSourceSummary>,
) -> AgentSuggestion {
    let target_range = observation.selection.as_ref().and_then(|selection| {
        if selection.from < selection.to {
            Some(AgentTextRange {
                from: selection.from,
                to: selection.to,
            })
        } else {
            None
        }
    });
    let preview_text = match decision.kind {
        AgentSuggestionKind::Revise => "这里可以收紧句子，让动作、情绪和目的更清楚。".to_string(),
        AgentSuggestionKind::Continuity => {
            "建议先核对前文因果：这一句可能需要补一个触发点或解释。".to_string()
        }
        AgentSuggestionKind::Lore => "这里可能需要引用设定库中的既有规则。".to_string(),
        AgentSuggestionKind::Structure => {
            "这一段可以对齐当前章节节拍，提前埋下下一场冲突。".to_string()
        }
        AgentSuggestionKind::Question => "这里需要你确认角色真正想隐瞒什么。".to_string(),
        AgentSuggestionKind::Continue => {
            "可以顺着当前情绪推进一小段，让人物先做出一个无法回避的动作。".to_string()
        }
    };

    AgentSuggestion {
        id: format!("sug-{}-{}", observation.id, now_ms()),
        request_id,
        observation_id: observation.id.clone(),
        kind: decision.kind.clone(),
        target_range,
        anchor_position: Some(observation.cursor_position),
        confidence: decision.confidence,
        reason: decision.reason.clone(),
        source_summaries,
        preview_text,
        actions: vec![
            AgentSuggestionAction::Accept,
            AgentSuggestionAction::Reject,
            AgentSuggestionAction::Snooze,
            AgentSuggestionAction::Explain,
        ],
        created_at: now_ms(),
    }
}

pub fn build_source_summaries(
    observation: &AgentObservation,
    outline_summary: Option<String>,
    lore_hits: Vec<(String, String)>,
    profile_count: usize,
) -> Vec<AgentSourceSummary> {
    let mut sources = Vec::new();
    sources.push(summary_source(
        "editor_window",
        observation
            .chapter_title
            .as_deref()
            .unwrap_or("current chapter"),
        &observation.nearby_text,
        220,
    ));

    if let Some(summary) = outline_summary {
        sources.push(summary_source(
            "outline",
            "current outline node",
            &summary,
            220,
        ));
    }

    for (keyword, content) in lore_hits.into_iter().take(3) {
        sources.push(summary_source("lorebook", &keyword, &content, 220));
    }

    if profile_count > 0 {
        sources.push(AgentSourceSummary {
            source_type: "user_drift_profile".to_string(),
            label: "learned preferences".to_string(),
            summary: format!("{} active preference entries available", profile_count),
            original_chars: 0,
            included_chars: 0,
            truncated: false,
        });
    }

    sources
}

fn summary_source(
    source_type: &str,
    label: &str,
    content: &str,
    max_chars: usize,
) -> AgentSourceSummary {
    let original_chars = char_count(content);
    let summary: String = content.chars().take(max_chars).collect();
    let included_chars = char_count(&summary);
    AgentSourceSummary {
        source_type: source_type.to_string(),
        label: label.to_string(),
        summary,
        original_chars,
        included_chars,
        truncated: original_chars > included_chars,
    }
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

    fn observation(mode: AgentMode, paragraph: &str) -> AgentObservation {
        AgentObservation {
            id: "obs-1".to_string(),
            mode,
            reason: AgentObservationReason::UserTyped,
            created_at: 100,
            chapter_title: Some("Chapter-1".to_string()),
            chapter_revision: Some("rev".to_string()),
            dirty: true,
            cursor_position: 10,
            selection: None,
            current_paragraph: paragraph.to_string(),
            nearby_text: paragraph.to_string(),
            recent_edit_summary: "typed".to_string(),
            idle_ms: 1_100,
            snoozed_until: None,
            outline_chapter_title: Some("Chapter-1".to_string()),
        }
    }

    #[test]
    fn policy_noops_when_disabled() {
        let obs = observation(
            AgentMode::Off,
            "这是一个足够长的段落，用来验证关闭状态不会触发主动建议。",
        );
        let decision = attention_policy(&obs, 1_000);
        assert!(!decision.should_suggest);
        assert_eq!(decision.reason, "proactive agent disabled");
    }

    #[test]
    fn policy_noops_when_snoozed() {
        let mut obs = observation(
            AgentMode::Proactive,
            "这是一个足够长的段落，用来验证暂停状态不会触发主动建议。",
        );
        obs.snoozed_until = Some(2_000);
        let decision = attention_policy(&obs, 1_000);
        assert!(!decision.should_suggest);
        assert_eq!(decision.reason, "suggestions are snoozed");
    }

    #[test]
    fn policy_triggers_on_meaningful_pause() {
        let obs = observation(AgentMode::Proactive, "林墨停在旧门前，风从裂开的门缝里钻出来，带着一股潮湿的冷意。他没有立刻进去，只把手按在刀柄上。");
        let decision = attention_policy(&obs, 1_000);
        assert!(decision.should_suggest);
        assert_eq!(decision.kind, AgentSuggestionKind::Continue);
    }

    #[test]
    fn policy_triggers_lore_when_paragraph_has_setting_cue() {
        let obs = observation(
            AgentMode::Proactive,
            "林墨停在破庙门前，密道里的毒雾正从裂缝里渗出来。",
        );
        let decision = attention_policy(&obs, 1_000);
        assert!(decision.should_suggest);
        assert_eq!(decision.kind, AgentSuggestionKind::Lore);
    }

    #[test]
    fn policy_triggers_on_selection_pause() {
        let mut obs = observation(AgentMode::Proactive, "短句");
        obs.reason = AgentObservationReason::SelectionChange;
        obs.selection = Some(AgentSelection {
            from: 2,
            to: 20,
            text: "这是一段被选中的文字".to_string(),
        });
        let decision = attention_policy(&obs, 1_000);
        assert!(decision.should_suggest);
        assert_eq!(decision.kind, AgentSuggestionKind::Revise);
    }

    #[test]
    fn source_summary_counts_unicode_chars() {
        let source = summary_source("editor", "段落", "林墨推门而入", 3);
        assert_eq!(source.original_chars, 6);
        assert_eq!(source.included_chars, 3);
        assert_eq!(source.summary, "林墨推");
        assert!(source.truncated);
    }

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
