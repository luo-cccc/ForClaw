//! WriterMemory - structured creative ledgers.
//! Canon, promises, style preferences, creative decisions.
//! Ported from the plan's Creative Ledgers specification.

use rusqlite::{Connection, OptionalExtension, Result as SqlResult};
use serde::{Deserialize, Serialize};

include!("memory/schema.in.rs");

pub struct WriterMemory {
    conn: Connection,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CanonEntitySummary {
    pub kind: String,
    pub name: String,
    pub summary: String,
    pub attributes: serde_json::Value,
    pub confidence: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CanonRuleSummary {
    pub rule: String,
    pub category: String,
    pub priority: i32,
    pub status: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlotPromiseSummary {
    pub id: i64,
    pub kind: String,
    pub title: String,
    pub description: String,
    pub introduced_chapter: String,
    pub last_seen_chapter: String,
    pub last_seen_ref: String,
    pub expected_payoff: String,
    pub priority: i32,
    #[serde(default)]
    pub risk: String,
    #[serde(default)]
    pub blocked_reason: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub promoted: bool,
    #[serde(default)]
    pub core: bool,
    #[serde(default)]
    pub related_entities: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreativeDecisionSummary {
    pub scope: String,
    pub title: String,
    pub decision: String,
    pub rationale: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StylePreferenceSummary {
    pub key: String,
    pub value: String,
    pub accepted_count: i64,
    pub rejected_count: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum PromiseKind {
    #[default]
    PlotPromise,
    EmotionalDebt,
    ObjectWhereabouts,
    CharacterCommitment,
    MysteryClue,
    RelationshipTension,
    #[serde(other)]
    Other,
}

impl PromiseKind {
    pub fn from_kind_str(kind: &str) -> Self {
        match kind {
            "plot_promise" => PromiseKind::PlotPromise,
            "emotional_debt" => PromiseKind::EmotionalDebt,
            "object_whereabouts" => PromiseKind::ObjectWhereabouts,
            "character_commitment" => PromiseKind::CharacterCommitment,
            "mystery_clue" => PromiseKind::MysteryClue,
            "relationship_tension" => PromiseKind::RelationshipTension,
            _ => PromiseKind::Other,
        }
    }

    pub fn as_kind_str(&self) -> &'static str {
        match self {
            PromiseKind::PlotPromise => "plot_promise",
            PromiseKind::EmotionalDebt => "emotional_debt",
            PromiseKind::ObjectWhereabouts => "object_whereabouts",
            PromiseKind::CharacterCommitment => "character_commitment",
            PromiseKind::MysteryClue => "mystery_clue",
            PromiseKind::RelationshipTension => "relationship_tension",
            PromiseKind::Other => "other",
        }
    }

    pub fn default_risk(&self) -> &'static str {
        match self {
            PromiseKind::ObjectWhereabouts | PromiseKind::MysteryClue => "high",
            PromiseKind::RelationshipTension | PromiseKind::EmotionalDebt => "medium",
            PromiseKind::CharacterCommitment | PromiseKind::PlotPromise => "medium",
            PromiseKind::Other => "low",
        }
    }
}

pub fn canonicalize_promise_identity_segment(value: &str) -> String {
    value
        .chars()
        .filter(|ch| {
            !ch.is_whitespace() && !matches!(ch, '，' | '。' | '！' | '？' | ',' | '.' | '!' | '?')
        })
        .flat_map(|ch| ch.to_lowercase())
        .collect::<String>()
}

pub fn promise_identity_key(kind: &str, title: &str, description: &str) -> String {
    let title_key = canonicalize_promise_identity_segment(title);
    let description_key = canonicalize_promise_identity_segment(description)
        .chars()
        .take(24)
        .collect::<String>();
    format!("{}|{}|{}", kind.trim(), title_key, description_key)
}

pub fn promise_identity_matches(
    left_kind: &str,
    left_title: &str,
    left_description: &str,
    right_kind: &str,
    right_title: &str,
    right_description: &str,
) -> bool {
    if left_kind.trim() != right_kind.trim() {
        return false;
    }
    let left_title_key = canonicalize_promise_identity_segment(left_title);
    let right_title_key = canonicalize_promise_identity_segment(right_title);
    if left_title_key.is_empty() || left_title_key != right_title_key {
        return false;
    }
    let left_desc = canonicalize_promise_identity_segment(left_description);
    let right_desc = canonicalize_promise_identity_segment(right_description);
    if left_desc.is_empty() || right_desc.is_empty() {
        return false;
    }
    left_desc.contains(&right_desc)
        || right_desc.contains(&left_desc)
        || shared_prefix_len(&left_desc, &right_desc) >= 10
}

fn shared_prefix_len(left: &str, right: &str) -> usize {
    left.chars()
        .zip(right.chars())
        .take_while(|(l, r)| l == r)
        .count()
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct StoryContractSummary {
    pub project_id: String,
    pub title: String,
    pub genre: String,
    pub target_reader: String,
    pub reader_promise: String,
    pub first_30_chapter_promise: String,
    pub main_conflict: String,
    pub structural_boundary: String,
    pub tone_contract: String,
    pub updated_at: String,
    #[serde(default)]
    pub quality: String,
    #[serde(default)]
    pub quality_gaps: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ChapterMissionSummary {
    pub id: i64,
    pub project_id: String,
    pub chapter_title: String,
    pub mission: String,
    pub must_include: String,
    pub must_not: String,
    pub expected_ending: String,
    pub status: String,
    pub source_ref: String,
    pub updated_at: String,
    #[serde(default)]
    pub blocked_reason: String,
    #[serde(default)]
    pub retired_history: String,
    #[serde(default)]
    pub reader_lack_this_chapter: String,
    #[serde(default)]
    pub relationship_soil_this_chapter: String,
    #[serde(default)]
    pub pressure_scene: String,
    #[serde(default)]
    pub interest_mechanism: String,
    #[serde(default)]
    pub payoff_target: String,
    #[serde(default)]
    pub payoff_path: String,
    #[serde(default)]
    pub next_lack_opened: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ChapterResultSummary {
    pub id: i64,
    pub project_id: String,
    pub chapter_title: String,
    pub chapter_revision: String,
    pub summary: String,
    pub state_changes: Vec<String>,
    pub character_progress: Vec<String>,
    pub new_conflicts: Vec<String>,
    pub new_clues: Vec<String>,
    pub promise_updates: Vec<String>,
    pub canon_updates: Vec<String>,
    pub source_ref: String,
    pub created_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct NextBeatSummary {
    pub chapter_title: String,
    pub goal: String,
    pub carryovers: Vec<String>,
    pub blockers: Vec<String>,
    pub source_refs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct VolumeSummary {
    pub id: String,
    pub project_id: String,
    pub title: String,
    pub start_chapter: i64,
    pub end_chapter: i64,
    pub contract: serde_json::Value,
    pub mission: serde_json::Value,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct VolumeSnapshotSummary {
    pub project_id: String,
    pub volume_id: String,
    pub snapshot: serde_json::Value,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ArcSnapshotSummary {
    pub project_id: String,
    pub arc_id: String,
    pub volume_id: String,
    pub title: String,
    pub start_chapter: i64,
    pub end_chapter: i64,
    pub snapshot: serde_json::Value,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct BookStateSummary {
    pub project_id: String,
    pub title: String,
    pub long_term_constraints: Vec<String>,
    pub mega_promises: Vec<String>,
    pub irreversible_changes: Vec<String>,
    pub source_ref: String,
    pub updated_at: String,
}

impl ChapterMissionSummary {
    pub fn is_empty(&self) -> bool {
        [
            &self.mission,
            &self.must_include,
            &self.must_not,
            &self.expected_ending,
        ]
        .iter()
        .all(|value| value.trim().is_empty())
    }

    pub fn render_for_context(&self) -> String {
        let mut lines = Vec::new();
        push_contract_line(&mut lines, "章节", &self.chapter_title);
        push_contract_line(&mut lines, "本章任务", &self.mission);
        push_contract_line(&mut lines, "必保事项", &self.must_include);
        push_contract_line(&mut lines, "禁止事项", &self.must_not);
        push_contract_line(&mut lines, "预期收束", &self.expected_ending);
        push_contract_line(&mut lines, "任务状态", &self.status);
        if self.status == "blocked" && !self.blocked_reason.trim().is_empty() {
            push_contract_line(&mut lines, "阻塞原因", &self.blocked_reason);
        }
        if self.status == "retired" && !self.retired_history.trim().is_empty() {
            push_contract_line(&mut lines, "退役说明", &self.retired_history);
        }
        push_contract_line(&mut lines, "读者缺口", &self.reader_lack_this_chapter);
        push_contract_line(&mut lines, "关系土壤", &self.relationship_soil_this_chapter);
        push_contract_line(&mut lines, "压迫场景", &self.pressure_scene);
        push_contract_line(&mut lines, "计息机制", &self.interest_mechanism);
        push_contract_line(&mut lines, "补偿目标", &self.payoff_target);
        push_contract_line(&mut lines, "补偿路径", &self.payoff_path);
        push_contract_line(&mut lines, "下一层缺口", &self.next_lack_opened);
        lines.join("\n")
    }
}

impl ChapterResultSummary {
    pub fn is_empty(&self) -> bool {
        self.summary.trim().is_empty()
            && self.state_changes.is_empty()
            && self.character_progress.is_empty()
            && self.new_conflicts.is_empty()
            && self.new_clues.is_empty()
            && self.promise_updates.is_empty()
            && self.canon_updates.is_empty()
    }

    pub fn render_for_context(&self) -> String {
        let mut lines = Vec::new();
        push_contract_line(&mut lines, "章节结果", &self.chapter_title);
        push_contract_line(&mut lines, "结果摘要", &self.summary);
        push_list_line(&mut lines, "状态变化", &self.state_changes);
        push_list_line(&mut lines, "角色推进", &self.character_progress);
        push_list_line(&mut lines, "新冲突", &self.new_conflicts);
        push_list_line(&mut lines, "新线索", &self.new_clues);
        push_list_line(&mut lines, "伏笔变化", &self.promise_updates);
        push_list_line(&mut lines, "设定变化", &self.canon_updates);
        push_contract_line(&mut lines, "来源", &self.source_ref);
        lines.join("\n")
    }
}

impl NextBeatSummary {
    pub fn is_empty(&self) -> bool {
        self.goal.trim().is_empty() && self.carryovers.is_empty() && self.blockers.is_empty()
    }

    pub fn render_for_context(&self) -> String {
        let mut lines = Vec::new();
        push_contract_line(&mut lines, "接力章节", &self.chapter_title);
        push_contract_line(&mut lines, "下一拍目标", &self.goal);
        push_list_line(&mut lines, "需要接住", &self.carryovers);
        push_list_line(&mut lines, "阻塞/风险", &self.blockers);
        push_list_line(&mut lines, "依据", &self.source_refs);
        lines.join("\n")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StoryContractQuality {
    Missing,
    Vague,
    Usable,
    Strong,
}

impl StoryContractSummary {
    pub fn fill_quality(&mut self) {
        self.quality = match self.quality() {
            StoryContractQuality::Missing => "missing",
            StoryContractQuality::Vague => "vague",
            StoryContractQuality::Usable => "usable",
            StoryContractQuality::Strong => "strong",
        }
        .to_string();
        self.quality_gaps = self.quality_gaps();
    }

    pub fn is_empty(&self) -> bool {
        [
            &self.title,
            &self.genre,
            &self.target_reader,
            &self.reader_promise,
            &self.first_30_chapter_promise,
            &self.main_conflict,
            &self.structural_boundary,
            &self.tone_contract,
        ]
        .iter()
        .all(|value| value.trim().is_empty())
    }

    pub fn quality(&self) -> StoryContractQuality {
        let reader_promise_len = self.reader_promise.trim().chars().count();
        let main_conflict_len = self.main_conflict.trim().chars().count();
        let tone_contract_len = self.tone_contract.trim().chars().count();
        let structural_boundary_len = self.structural_boundary.trim().chars().count();
        let first_30_len = self.first_30_chapter_promise.trim().chars().count();

        if self.is_empty() {
            return StoryContractQuality::Missing;
        }

        let weak_fields = [
            ("title", self.title.trim().is_empty(), 0),
            ("genre", self.genre.trim().is_empty(), 0),
            ("reader_promise", reader_promise_len < 8, reader_promise_len),
            ("main_conflict", main_conflict_len < 8, main_conflict_len),
            ("tone_contract", tone_contract_len < 6, tone_contract_len),
            (
                "structural_boundary",
                structural_boundary_len < 8,
                structural_boundary_len,
            ),
            ("first_30_chapter_promise", first_30_len < 8, first_30_len),
        ];
        let vague_count = weak_fields.iter().filter(|(_, weak, _)| *weak).count();

        if vague_count >= 3 {
            return StoryContractQuality::Vague;
        }

        if vague_count >= 1
            || reader_promise_len < 20
            || main_conflict_len < 20
            || tone_contract_len < 12
        {
            return StoryContractQuality::Usable;
        }

        StoryContractQuality::Strong
    }

    pub fn quality_gaps(&self) -> Vec<String> {
        let mut gaps = Vec::new();
        let quality = self.quality();

        if quality == StoryContractQuality::Strong {
            return gaps;
        }

        let checks: Vec<(&str, &str, usize, usize)> = vec![
            ("title", &self.title, 0, 1),
            ("genre", &self.genre, 0, 1),
            ("target_reader", &self.target_reader, 0, 1),
            ("reader_promise", &self.reader_promise, 8, 20),
            (
                "first_30_chapter_promise",
                &self.first_30_chapter_promise,
                8,
                12,
            ),
            ("main_conflict", &self.main_conflict, 8, 20),
            ("structural_boundary", &self.structural_boundary, 8, 12),
            ("tone_contract", &self.tone_contract, 6, 12),
        ];

        let labels: std::collections::HashMap<&str, &str> = [
            ("title", "标题"),
            ("genre", "题材定位"),
            ("target_reader", "目标读者"),
            ("reader_promise", "读者承诺"),
            ("first_30_chapter_promise", "前30章承诺"),
            ("main_conflict", "核心冲突"),
            ("structural_boundary", "结构边界"),
            ("tone_contract", "语气/风格合同"),
        ]
        .into();

        for (key, value, min_chars, strong_chars) in checks {
            let char_count = value.trim().chars().count();
            if char_count == 0 {
                gaps.push(format!("{}: 缺失", labels[key]));
            } else if char_count < min_chars {
                gaps.push(format!(
                    "{}: 过于简略 ({}字，至少需要{}字)",
                    labels[key], char_count, min_chars
                ));
            } else if quality == StoryContractQuality::Usable && char_count < strong_chars {
                gaps.push(format!(
                    "{}: 可以更具体 ({}字，建议{}字以上)",
                    labels[key], char_count, strong_chars
                ));
            }
        }

        gaps
    }

    pub fn render_for_context(&self) -> String {
        let mut lines = Vec::new();
        let quality = self.quality();
        lines.push(format!(
            "合同质量: {}",
            match quality {
                StoryContractQuality::Missing => "缺失 — 请在设置中填写故事合同",
                StoryContractQuality::Vague => "模糊 — 关键字段不够具体，会影响AI写作质量",
                StoryContractQuality::Usable => "可用",
                StoryContractQuality::Strong => "完整 — 所有关键约束清晰",
            }
        ));
        push_contract_line(&mut lines, "标题", &self.title);
        push_contract_line(&mut lines, "题材定位", &self.genre);
        push_contract_line(&mut lines, "目标读者", &self.target_reader);
        push_contract_line(&mut lines, "读者承诺", &self.reader_promise);
        push_contract_line(&mut lines, "前30章承诺", &self.first_30_chapter_promise);
        push_contract_line(&mut lines, "核心冲突", &self.main_conflict);
        push_contract_line(&mut lines, "结构边界", &self.structural_boundary);
        push_contract_line(&mut lines, "语气/风格合同", &self.tone_contract);
        lines.join("\n")
    }
}

pub(crate) fn push_contract_line(lines: &mut Vec<String>, label: &str, value: &str) {
    let value = value.trim();
    if !value.is_empty() {
        lines.push(format!("{}: {}", label, value));
    }
}

fn push_list_line(lines: &mut Vec<String>, label: &str, values: &[String]) {
    let values = values
        .iter()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .take(5)
        .collect::<Vec<_>>();
    if !values.is_empty() {
        lines.push(format!("{}: {}", label, values.join("；")));
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryAuditSummary {
    pub proposal_id: String,
    pub kind: String,
    pub action: String,
    pub title: String,
    pub evidence: String,
    pub rationale: String,
    pub reason: Option<String>,
    pub created_at: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryFeedbackSummary {
    pub slot: String,
    pub category: String,
    pub action: String,
    pub confidence_delta: f64,
    pub source_error: Option<String>,
    pub proposal_id: String,
    pub reason: Option<String>,
    pub created_at: u64,
}

#[derive(Debug, Clone)]
pub struct ObservationTraceSummary {
    pub id: String,
    pub created_at: u64,
    pub reason: String,
    pub chapter_title: Option<String>,
    pub paragraph_snippet: String,
}

#[derive(Debug, Clone)]
pub struct ProposalTraceSummary {
    pub id: String,
    pub observation_id: String,
    pub kind: String,
    pub priority: String,
    pub state: String,
    pub confidence: f64,
    pub preview_snippet: String,
    pub evidence: Vec<super::proposal::EvidenceRef>,
    pub context_budget: Option<ContextBudgetTrace>,
    pub expires_at: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ContextBudgetTrace {
    pub task: String,
    pub used: usize,
    pub total_budget: usize,
    pub wasted: usize,
    pub source_reports: Vec<ContextSourceBudgetTrace>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ContextSourceBudgetTrace {
    pub source: String,
    pub requested: usize,
    pub provided: usize,
    pub truncated: bool,
    pub reason: String,
    pub truncation_reason: Option<String>,
}

#[derive(Debug, Clone)]
pub struct FeedbackTraceSummary {
    pub proposal_id: String,
    pub action: String,
    pub reason: Option<String>,
    pub created_at: u64,
}

#[derive(Debug, Clone)]
pub struct RunEventSummary {
    pub seq: u64,
    pub project_id: String,
    pub session_id: String,
    pub task_id: Option<String>,
    pub event_type: String,
    pub source_refs: Vec<String>,
    pub data: serde_json::Value,
    pub ts_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextRecallSummary {
    pub source: String,
    pub reference: String,
    pub snippet: String,
    pub recall_count: u64,
    pub first_recalled_at: u64,
    pub last_recalled_at: u64,
    pub last_observation_id: String,
    pub last_proposal_id: String,
}

#[derive(Debug, Clone)]
pub struct ManualAgentTurnSummary {
    pub project_id: String,
    pub observation_id: String,
    pub chapter_title: Option<String>,
    pub user: String,
    pub assistant: String,
    pub source_refs: Vec<String>,
    pub created_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ReaderCompensationProfile {
    pub target_reader: String,
    pub primary_lack: String,
    pub secondary_lacks: Vec<String>,
    pub protagonist_proxy_state: String,
    pub dominant_relationship_soil: String,
    pub pressure_mode: String,
    pub payoff_mode: String,
    pub payoff_path: String,
    pub escalation_ladder: String,
    pub forbidden_shortcuts: Vec<String>,
    pub confidence: f64,
    pub source_refs: Vec<String>,
    pub pending_approval: bool,
    pub approved_by: String,
    pub approved_at: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct EmotionalDebtLifecycle {
    pub debt_id: String,
    pub debt_kind: String,
    pub relationship_soil: String,
    pub introduced_by_scene: String,
    pub interest_mechanism: String,
    pub payoff_contract: String,
    pub payoff_window: String,
    pub current_state: String,
    pub overdue_risk: String,
    pub rollover_target: String,
    pub source_refs: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct EmotionalDebtSummary {
    pub id: i64,
    pub project_id: String,
    pub debt_kind: String,
    pub title: String,
    pub description: String,
    pub introduced_at: String,
    pub introduced_chapter: String,
    pub introduced_ref: String,
    pub relationship_soil: String,
    pub pressure_evidence: String,
    pub interest_mechanism: String,
    pub payoff_contract: String,
    pub payoff_status: String,
    pub expected_payoff_window: String,
    pub payoff_path: String,
    pub overdue_risk: String,
    pub rollover_target: String,
    pub risk_level: String,
    pub related_promise_ids: Vec<String>,
    pub source_refs: Vec<String>,
    pub updated_at: String,
}

impl WriterMemory {
    pub fn open(path: &std::path::Path) -> rusqlite::Result<Self> {
        let conn = Connection::open(path)?;
        initialize_schema(&conn)?;
        Ok(Self { conn })
    }
}

include!("memory/canon_methods.in.rs");
include!("memory/character_methods.in.rs");
include!("memory/character_state_methods.in.rs");
include!("memory/relationship_methods.in.rs");
include!("memory/knowledge_methods.in.rs");
include!("memory/ownership_methods.in.rs");
include!("memory/identity_methods.in.rs");
include!("memory/reveal_methods.in.rs");
include!("memory/promises_methods.in.rs");
include!("memory/style_contract_methods.in.rs");
include!("memory/mission_result_methods.in.rs");
include!("memory/feedback_methods.in.rs");
include!("memory/tracing_impl.in.rs");
include!("memory/tracing_migrate.in.rs");
include!("memory/tracing_helpers.in.rs");
include!("memory/tracing_tests.in.rs");
include!("memory/reader_compensation_methods.in.rs");
include!("memory/emotional_debt_lifecycle_methods.in.rs");
include!("memory/emotional_debt_ledger_methods.in.rs");
include!("memory/volume_arc_book_methods.in.rs");
include!("memory/sprint_methods.in.rs");
include!("memory/scene_methods.in.rs");
include!("memory/scene_state_methods.in.rs");
include!("memory/scene_obligation_methods.in.rs");
include!("memory/scene_result_methods.in.rs");
include!("memory/time_slice_methods.in.rs");
include!("memory/chapter_time_methods.in.rs");
include!("memory/timeline_event_methods.in.rs");
