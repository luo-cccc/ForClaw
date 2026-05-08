//! Chapter Settlement Queue — post-save settlement workflow.
//!
//! After a chapter is saved, this queue groups reviewable updates by
//! Canon / Promise / Mission / Style / Continuity risk, keeping the
//! author in control via explicit approval.

use serde::{Deserialize, Serialize};

use super::diagnostics::{DiagnosticCategory, DiagnosticSeverity};
use super::memory::WriterMemory;
use super::post_write_diagnostics::WriterPostWriteDiagnosticReport;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ChapterSettlementQueue {
    pub chapter_title: String,
    pub chapter_revision: String,
    pub canon_updates: Vec<SettlementItem>,
    pub promise_updates: Vec<SettlementItem>,
    pub mission_suggestions: Vec<SettlementItem>,
    pub style_notes: Vec<SettlementItem>,
    pub continuity_risks: Vec<SettlementItem>,
    pub high_priority_count: usize,
    pub requires_author_approval: bool,
    pub evidence_refs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SettlementItem {
    pub id: String,
    pub category: String, // "canon" | "promise" | "mission" | "style" | "continuity"
    pub title: String,
    pub description: String,
    pub priority: String, // "high" | "medium" | "low"
    pub requires_approval: bool,
    pub evidence_source: String,
    pub suggested_action: String,
}

#[derive(Debug, Clone, Default)]
pub struct ChapterSettlementEvidence<'a> {
    pub saved_chapter_text: Option<&'a str>,
    pub post_write_diagnostics: Option<&'a WriterPostWriteDiagnosticReport>,
    pub story_impact_risk: Option<&'a str>,
    pub story_impact_sources: &'a [String],
}

/// Build a settlement queue from post-save memory state.
/// All items are proposals — nothing is written automatically.
pub fn build_chapter_settlement_queue(
    chapter_title: &str,
    chapter_revision: &str,
    memory: &WriterMemory,
    project_id: &str,
) -> ChapterSettlementQueue {
    build_chapter_settlement_queue_with_evidence(
        chapter_title,
        chapter_revision,
        memory,
        project_id,
        ChapterSettlementEvidence::default(),
    )
}

/// Build a settlement queue from real post-save evidence.
/// All items are proposals — nothing is written automatically.
pub fn build_chapter_settlement_queue_with_evidence(
    chapter_title: &str,
    chapter_revision: &str,
    memory: &WriterMemory,
    project_id: &str,
    evidence: ChapterSettlementEvidence<'_>,
) -> ChapterSettlementQueue {
    let mut canon_updates = Vec::new();
    let mut promise_updates = Vec::new();
    let mut mission_suggestions = Vec::new();
    let mut style_notes = Vec::new();
    let mut continuity_risks = Vec::new();
    let mut evidence_refs = Vec::new();

    evidence_refs.push(format!("chapter:{}", chapter_title));
    evidence_refs.push(format!("revision:{}", chapter_revision));
    if let Some(report) = evidence.post_write_diagnostics {
        evidence_refs.extend(report.source_refs.iter().cloned());
        for diagnostic in &report.diagnostics {
            let priority = priority_for_diagnostic(&diagnostic.severity);
            let item = SettlementItem {
                id: format!("diagnostic:{}", diagnostic.diagnostic_id),
                category: settlement_category_for_diagnostic(&diagnostic.category).to_string(),
                title: diagnostic.message.clone(),
                description: diagnostic.fix_suggestion.clone().unwrap_or_else(|| {
                    "Review this post-write diagnostic before continuing.".to_string()
                }),
                priority: priority.to_string(),
                requires_approval: requires_approval_for_diagnostic(&diagnostic.category),
                evidence_source: diagnostic
                    .evidence_refs
                    .first()
                    .cloned()
                    .unwrap_or_else(|| format!("post_write_diagnostics:{}", report.observation_id)),
                suggested_action: suggested_action_for_diagnostic(&diagnostic.category).to_string(),
            };
            match diagnostic.category {
                DiagnosticCategory::CanonConflict => canon_updates.push(item),
                DiagnosticCategory::UnresolvedPromise | DiagnosticCategory::PayoffGap => {
                    promise_updates.push(item)
                }
                DiagnosticCategory::StoryContractViolation
                | DiagnosticCategory::ChapterMissionViolation => mission_suggestions.push(item),
                DiagnosticCategory::CharacterVoiceInconsistency
                | DiagnosticCategory::PacingNote => style_notes.push(item),
                DiagnosticCategory::TimelineIssue => continuity_risks.push(item),
            }
        }
        if report.error_count > 0 {
            continuity_risks.push(SettlementItem {
                id: format!("post_write_blockers:{}", report.observation_id),
                category: "continuity".to_string(),
                title: format!("保存后诊断发现 {} 个阻断错误", report.error_count),
                description: report.remediation.join(" "),
                priority: "high".to_string(),
                requires_approval: false,
                evidence_source: format!("post_write_diagnostics:{}", report.observation_id),
                suggested_action: "先处理阻断诊断，再继续下一次写入".to_string(),
            });
        }
    }

    if let Some(text) = evidence.saved_chapter_text {
        if text.trim().is_empty() {
            continuity_risks.push(SettlementItem {
                id: format!("empty_save:{}", chapter_revision),
                category: "continuity".to_string(),
                title: "保存正文为空".to_string(),
                description: "Saved chapter text is empty for this revision.".to_string(),
                priority: "high".to_string(),
                requires_approval: false,
                evidence_source: format!("revision:{}", chapter_revision),
                suggested_action: "确认保存状态或恢复上一版正文".to_string(),
            });
        }
        evidence_refs.push(format!(
            "chapter_text:{}:chars={}",
            chapter_revision,
            text.chars().count()
        ));
    }

    if let Some(risk) = evidence.story_impact_risk {
        evidence_refs.extend(evidence.story_impact_sources.iter().cloned());
        if risk.eq_ignore_ascii_case("high") || risk.eq_ignore_ascii_case("blocked") {
            continuity_risks.push(SettlementItem {
                id: format!("story_impact:{}", chapter_revision),
                category: "continuity".to_string(),
                title: format!("Story Impact 风险: {}", risk),
                description: format!(
                    "This saved revision touches {} story impact sources.",
                    evidence.story_impact_sources.len()
                ),
                priority: "high".to_string(),
                requires_approval: false,
                evidence_source: evidence
                    .story_impact_sources
                    .first()
                    .cloned()
                    .unwrap_or_else(|| format!("story_impact:{}", chapter_revision)),
                suggested_action: "在继续写作前审查受影响的 canon / promise / mission".to_string(),
            });
        }
    }

    // Canon: flag entities with low confidence.
    if let Ok(entities) = memory.list_canon_entities() {
        for entity in &entities {
            if entity.confidence < 0.6 {
                canon_updates.push(SettlementItem {
                    id: format!("canon:{}", entity.name),
                    category: "canon".to_string(),
                    title: format!("低置信度实体: {}", entity.name),
                    description: format!(
                        "Entity '{}' (kind={}) has confidence {}",
                        entity.name, entity.kind, entity.confidence
                    ),
                    priority: "high".to_string(),
                    requires_approval: true,
                    evidence_source: format!("canon_fact:{}", entity.name),
                    suggested_action: "审查并确认或修正此实体属性".to_string(),
                });
                evidence_refs.push(format!("canon:{}", entity.name));
            }
        }
    }

    // Promise: flag at-risk or stale promises.
    if let Ok(promises) = memory.get_open_promise_summaries() {
        for p in &promises {
            if p.risk == "high" {
                promise_updates.push(SettlementItem {
                    id: format!("promise:{}", p.id),
                    category: "promise".to_string(),
                    title: format!("高风险伏笔: {}", p.title),
                    description: format!(
                        "Promise '{}' (kind={}) is at high risk; expected payoff: {}",
                        p.title, p.kind, p.expected_payoff
                    ),
                    priority: "high".to_string(),
                    requires_approval: true,
                    evidence_source: format!("promise:{}", p.id),
                    suggested_action: "审查伏笔状态，确认是否延期、兑现或放弃".to_string(),
                });
                evidence_refs.push(format!("promise:{}", p.id));
            }
        }
    }

    // Mission: check current chapter mission status.
    if let Ok(Some(mission)) = memory.get_chapter_mission(project_id, chapter_title) {
        if mission.status == "draft" || mission.status == "needs_review" {
            mission_suggestions.push(SettlementItem {
                id: format!("mission:{}", mission.chapter_title),
                category: "mission".to_string(),
                title: format!("章节任务需审查: {}", mission.chapter_title),
                description: format!(
                    "Mission status is '{}'; must_include: {}, must_not: {}",
                    mission.status, mission.must_include, mission.must_not
                ),
                priority: "medium".to_string(),
                requires_approval: true,
                evidence_source: format!("chapter_mission:{}", mission.chapter_title),
                suggested_action: "标记为 completed / drifted / needs_review".to_string(),
            });
            evidence_refs.push(format!("mission:{}", mission.chapter_title));
        }
    }

    // Style notes from recent feedback.
    if let Ok(feedback_entries) = memory.list_memory_feedback(5) {
        for entry in &feedback_entries {
            if entry.action == "correction" {
                style_notes.push(SettlementItem {
                    id: format!("style:{}", entry.slot),
                    category: "style".to_string(),
                    title: format!("风格纠错: {}", entry.slot),
                    description: entry.reason.clone().unwrap_or_default(),
                    priority: "medium".to_string(),
                    requires_approval: false,
                    evidence_source: format!("memory_feedback:{}", entry.slot),
                    suggested_action: "确认风格约束已更新".to_string(),
                });
            }
        }
    }

    let high_priority = canon_updates
        .iter()
        .filter(|i| i.priority == "high")
        .count()
        + promise_updates
            .iter()
            .filter(|i| i.priority == "high")
            .count()
        + mission_suggestions
            .iter()
            .filter(|i| i.priority == "high")
            .count()
        + style_notes.iter().filter(|i| i.priority == "high").count()
        + continuity_risks
            .iter()
            .filter(|i| i.priority == "high")
            .count();

    ChapterSettlementQueue {
        chapter_title: chapter_title.to_string(),
        chapter_revision: chapter_revision.to_string(),
        canon_updates,
        promise_updates,
        mission_suggestions,
        style_notes,
        continuity_risks,
        high_priority_count: high_priority,
        requires_author_approval: high_priority > 0,
        evidence_refs: normalize_refs(evidence_refs),
    }
}

fn priority_for_diagnostic(severity: &DiagnosticSeverity) -> &'static str {
    match severity {
        DiagnosticSeverity::Error => "high",
        DiagnosticSeverity::Warning => "medium",
        DiagnosticSeverity::Info => "low",
    }
}

fn settlement_category_for_diagnostic(category: &DiagnosticCategory) -> &'static str {
    match category {
        DiagnosticCategory::CanonConflict => "canon",
        DiagnosticCategory::UnresolvedPromise | DiagnosticCategory::PayoffGap => "promise",
        DiagnosticCategory::StoryContractViolation
        | DiagnosticCategory::ChapterMissionViolation => "mission",
        DiagnosticCategory::CharacterVoiceInconsistency | DiagnosticCategory::PacingNote => "style",
        DiagnosticCategory::TimelineIssue => "continuity",
    }
}

fn requires_approval_for_diagnostic(category: &DiagnosticCategory) -> bool {
    matches!(
        category,
        DiagnosticCategory::CanonConflict
            | DiagnosticCategory::UnresolvedPromise
            | DiagnosticCategory::PayoffGap
            | DiagnosticCategory::StoryContractViolation
            | DiagnosticCategory::ChapterMissionViolation
    )
}

fn suggested_action_for_diagnostic(category: &DiagnosticCategory) -> &'static str {
    match category {
        DiagnosticCategory::CanonConflict => "审查并确认是否更新 Canon",
        DiagnosticCategory::UnresolvedPromise | DiagnosticCategory::PayoffGap => {
            "审查伏笔/情绪债务状态，确认兑现、延期或放弃"
        }
        DiagnosticCategory::StoryContractViolation => "审查 Story Contract 偏离",
        DiagnosticCategory::ChapterMissionViolation => "审查 Chapter Mission 状态",
        DiagnosticCategory::TimelineIssue => "审查时间线连续性风险",
        DiagnosticCategory::CharacterVoiceInconsistency | DiagnosticCategory::PacingNote => {
            "审查作者声音或节奏漂移"
        }
    }
}

fn normalize_refs(refs: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::BTreeSet::new();
    refs.into_iter()
        .filter(|value| !value.trim().is_empty())
        .filter(|value| seen.insert(value.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::writer_agent::memory::WriterMemory;
    use std::path::Path;

    #[test]
    fn settlement_requires_approval_for_ledger() {
        let memory = WriterMemory::open(Path::new(":memory:")).unwrap();
        memory
            .ensure_story_contract_seed("eval", "T", "fantasy", "p", "j", "")
            .unwrap();
        memory
            .upsert_canon_entity(
                "location",
                "林墨",
                &[],
                "主角",
                &serde_json::json!({"weapon":"sword"}),
                0.3,
            )
            .ok();
        let queue = build_chapter_settlement_queue("Ch3", "rev-1", &memory, "eval");
        assert!(queue.requires_author_approval);
        assert!(queue.high_priority_count > 0);
    }

    #[test]
    fn settlement_groups_by_category() {
        let memory = WriterMemory::open(Path::new(":memory:")).unwrap();
        memory
            .ensure_story_contract_seed("eval", "T", "fantasy", "p", "j", "")
            .unwrap();
        memory
            .add_promise("plot_promise", "戒指", "遗物", "Ch1", "Ch5", 4)
            .unwrap();
        memory
            .upsert_canon_entity(
                "location",
                "林墨故居",
                &[],
                "x",
                &serde_json::json!({}),
                0.3,
            )
            .ok();
        let queue = build_chapter_settlement_queue("Ch3", "rev-1", &memory, "eval");
        assert!(
            !queue.canon_updates.is_empty() || !queue.promise_updates.is_empty(),
            "should have at least one category populated"
        );
    }
}
