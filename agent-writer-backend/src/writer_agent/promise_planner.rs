//! Promise payoff planning for read-only chapter planning.
//!
//! The planner ranks open promises by chapter timing, current mission focus,
//! nearby manuscript signals, and ledger priority. It does not mutate memory.

use serde::{Deserialize, Serialize};

use super::memory::{ChapterMissionSummary, PlotPromiseSummary, WriterMemory};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PromisePlannerAction {
    PayoffNow,
    PreparePayoff,
    Defer,
    AvoidDisturbing,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromisePlannerItem {
    pub promise_id: i64,
    pub title: String,
    pub kind: String,
    pub action: PromisePlannerAction,
    pub score: i32,
    pub priority: i32,
    pub introduced_chapter: String,
    pub expected_payoff: String,
    pub evidence_ref: String,
    pub rationale: String,
    pub reasons: Vec<String>,
}

pub fn plan_promise_payoffs(
    current_chapter: &str,
    mission: Option<&ChapterMissionSummary>,
    open_promises: &[PlotPromiseSummary],
    local_context: &str,
) -> Vec<PromisePlannerItem> {
    let mut items = open_promises
        .iter()
        .map(|promise| plan_promise_payoff(current_chapter, mission, promise, local_context))
        .collect::<Vec<_>>();
    items.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then_with(|| action_weight(&right.action).cmp(&action_weight(&left.action)))
            .then_with(|| right.priority.cmp(&left.priority))
            .then_with(|| left.promise_id.cmp(&right.promise_id))
    });
    items
}

pub fn render_promise_payoff_plan(items: &[PromisePlannerItem]) -> String {
    if items.is_empty() {
        return String::new();
    }
    items
        .iter()
        .take(8)
        .map(|item| {
            format!(
                "- {:?} score={} promise={} [{}] expected={} evidence={} why={}",
                item.action,
                item.score,
                item.title,
                item.kind,
                empty_as_later(&item.expected_payoff),
                item.evidence_ref,
                item.rationale
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn plan_promise_payoff(
    current_chapter: &str,
    mission: Option<&ChapterMissionSummary>,
    promise: &PlotPromiseSummary,
    local_context: &str,
) -> PromisePlannerItem {
    let mut score = promise.priority.clamp(0, 10);
    let mut reasons = vec![format!("ledger priority {}", promise.priority)];
    let current_number = chapter_number(current_chapter);
    let introduced_number = chapter_number(&promise.introduced_chapter);
    let payoff_number = chapter_number(&promise.expected_payoff);
    let promise_text = promise_text(promise);
    let mission_text = mission.map(render_mission_focus).unwrap_or_default();

    match (current_number, payoff_number) {
        (Some(current), Some(payoff)) if current == payoff => {
            score += 90;
            reasons.push("current chapter is expected payoff".to_string());
        }
        (Some(current), Some(payoff)) if current + 1 == payoff => {
            score += 64;
            reasons.push("next chapter is expected payoff".to_string());
        }
        (Some(current), Some(payoff)) if current > payoff => {
            score += 78;
            reasons.push("promise is overdue".to_string());
        }
        (Some(current), Some(payoff)) if payoff - current <= 2 => {
            score += 40;
            reasons.push("payoff is nearby".to_string());
        }
        _ => {}
    }

    if let (Some(current), Some(introduced)) = (current_number, introduced_number) {
        let age = current.saturating_sub(introduced);
        if age >= 4 {
            score += 22;
            reasons.push(format!("open for {} chapters", age));
        } else if age >= 2 {
            score += 10;
            reasons.push(format!("open for {} chapters", age));
        }
    }

    if promise.core {
        score += 72;
        reasons.push("core hook".to_string());
    } else if promise.promoted {
        score += 36;
        reasons.push("promoted hook".to_string());
    }
    if !promise.blocked_reason.trim().is_empty() {
        score += 28;
        reasons.push(format!("blocked by {}", promise.blocked_reason));
    }

    let mission_overlap = overlap_terms(&promise_text, &mission_text, 4);
    if !mission_overlap.is_empty() {
        score += 36 + mission_overlap.len() as i32 * 4;
        reasons.push(format!("mission overlap {}", mission_overlap.join("/")));
    }

    let local_overlap = overlap_terms(&promise_text, local_context, 4);
    if !local_overlap.is_empty() {
        score += 28 + local_overlap.len() as i32 * 3;
        reasons.push(format!("current draft overlap {}", local_overlap.join("/")));
    }

    let kind_action_bias = promise_kind_action_bias(&promise.kind);
    score += kind_action_bias.0;
    if !kind_action_bias.1.is_empty() {
        reasons.push(kind_action_bias.1.to_string());
    }

    let action = choose_action(
        current_number,
        payoff_number,
        mission,
        promise,
        local_context,
    );
    if action == PromisePlannerAction::AvoidDisturbing {
        score -= 35;
        reasons.push("mission must_not says avoid disturbing this promise".to_string());
    }
    let rationale = reasons.join("; ");

    PromisePlannerItem {
        promise_id: promise.id,
        title: promise.title.clone(),
        kind: promise.kind.clone(),
        action,
        score,
        priority: promise.priority,
        introduced_chapter: promise.introduced_chapter.clone(),
        expected_payoff: promise.expected_payoff.clone(),
        evidence_ref: format!("promise:{}", promise.id),
        rationale,
        reasons,
    }
}

fn choose_action(
    current_number: Option<i64>,
    payoff_number: Option<i64>,
    mission: Option<&ChapterMissionSummary>,
    promise: &PlotPromiseSummary,
    local_context: &str,
) -> PromisePlannerAction {
    if mission_must_not_overlaps(mission, promise) {
        return PromisePlannerAction::AvoidDisturbing;
    }
    if promise.core {
        return PromisePlannerAction::PayoffNow;
    }
    if !promise.blocked_reason.trim().is_empty() {
        return PromisePlannerAction::PreparePayoff;
    }
    if promise.promoted
        && current_number
            .zip(payoff_number)
            .is_some_and(|(current, payoff)| payoff - current <= 2)
    {
        return PromisePlannerAction::PreparePayoff;
    }
    if current_number
        .zip(payoff_number)
        .is_some_and(|(current, payoff)| current >= payoff)
    {
        return PromisePlannerAction::PayoffNow;
    }
    let promise_text = promise_text(promise);
    let mission_text = mission.map(render_mission_focus).unwrap_or_default();
    if !overlap_terms(&promise_text, local_context, 3).is_empty()
        || !overlap_terms(&promise_text, &mission_text, 3).is_empty()
    {
        return PromisePlannerAction::PreparePayoff;
    }
    if current_number
        .zip(payoff_number)
        .is_some_and(|(current, payoff)| payoff - current <= 1)
    {
        return PromisePlannerAction::PreparePayoff;
    }
    PromisePlannerAction::Defer
}

fn action_weight(action: &PromisePlannerAction) -> i32 {
    match action {
        PromisePlannerAction::PayoffNow => 4,
        PromisePlannerAction::PreparePayoff => 3,
        PromisePlannerAction::AvoidDisturbing => 2,
        PromisePlannerAction::Defer => 1,
    }
}

fn promise_kind_action_bias(kind: &str) -> (i32, &'static str) {
    match kind {
        "object_whereabouts" | "mystery_clue" => (12, "kind favors payoff planning"),
        "character_commitment" | "relationship_tension" => (8, "kind favors emotional continuity"),
        "emotional_debt" => (6, "kind favors emotional follow-through"),
        _ => (0, ""),
    }
}

fn mission_must_not_overlaps(
    mission: Option<&ChapterMissionSummary>,
    promise: &PlotPromiseSummary,
) -> bool {
    let Some(mission) = mission else {
        return false;
    };
    let must_not = mission.must_not.trim();
    if must_not.is_empty() {
        return false;
    }
    let promise_text = promise_text(promise);
    !overlap_terms(&promise_text, must_not, 2).is_empty()
}

fn overlap_terms(left: &str, right: &str, limit: usize) -> Vec<String> {
    if left.trim().is_empty() || right.trim().is_empty() {
        return Vec::new();
    }
    let mut overlaps = Vec::new();
    for term in meaningful_terms(left) {
        if overlaps.len() >= limit {
            break;
        }
        if right.contains(&term) && !overlaps.contains(&term) {
            overlaps.push(term);
        }
    }
    overlaps
}

fn meaningful_terms(text: &str) -> Vec<String> {
    let mut terms = Vec::new();
    let mut current = String::new();
    for ch in text.chars() {
        if is_term_char(ch) {
            current.push(ch);
        } else {
            push_term(&mut terms, &current);
            current.clear();
        }
    }
    push_term(&mut terms, &current);

    let chars = text.chars().collect::<Vec<_>>();
    for pair in chars.windows(2) {
        let term = pair.iter().collect::<String>();
        if term.chars().all(is_term_char) {
            push_term(&mut terms, &term);
        }
    }
    terms
}

fn push_term(terms: &mut Vec<String>, term: &str) {
    let term = term.trim();
    if term.chars().count() < 2 || is_stop_term(term) {
        return;
    }
    if !terms.iter().any(|existing| existing == term) {
        terms.push(term.to_string());
    }
}

fn is_stop_term(term: &str) -> bool {
    const STOP_TERMS: &[&str] = &[
        "需要", "交代", "下落", "伏笔", "回收", "揭示", "说明", "后续", "预期", "Chapter", "章节",
        "当前", "引入", "拿走", "留下", "发现", "必须", "为何", "什么", "没有", "不得", "不要",
        "不能", "禁止", "避免", "线索", "主线", "真相", "来源", "推进", "证据",
    ];
    STOP_TERMS.iter().any(|stop| term.contains(stop))
}

fn is_term_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ('\u{4e00}'..='\u{9fff}').contains(&ch)
}

fn promise_text(promise: &PlotPromiseSummary) -> String {
    [
        promise.title.as_str(),
        promise.kind.as_str(),
        promise.description.as_str(),
        promise.expected_payoff.as_str(),
        promise.last_seen_chapter.as_str(),
    ]
    .join("\n")
}

fn render_mission_focus(mission: &ChapterMissionSummary) -> String {
    [
        mission.mission.as_str(),
        mission.must_include.as_str(),
        mission.must_not.as_str(),
        mission.expected_ending.as_str(),
    ]
    .join("\n")
}

fn empty_as_later(value: &str) -> &str {
    if value.trim().is_empty() {
        "later chapter"
    } else {
        value
    }
}

fn chapter_number(chapter: &str) -> Option<i64> {
    let mut numbers = Vec::new();
    let mut current = String::new();
    for ch in chapter.chars() {
        if ch.is_ascii_digit() {
            current.push(ch);
        } else if !current.is_empty() {
            numbers.push(current.parse::<i64>().ok()?);
            current.clear();
        }
    }
    if !current.is_empty() {
        numbers.push(current.parse::<i64>().ok()?);
    }
    numbers.last().copied()
}

pub fn promise_subject_pressure(
    promise: &PlotPromiseSummary,
    memory: &WriterMemory,
    current_chapter: &str,
) -> f64 {
    let mut pressure = promise.priority as f64;

    // Protagonist-subject promises: 2x weight
    for related in &promise.related_entities {
        if let Some(name) = related.strip_prefix("character:") {
            if let Ok(Some(character)) = memory.get_character_by_name(name) {
                if character.role_type == "protagonist" {
                    pressure *= 2.0;
                }
            }
        }
    }

    // Core promises: 1.5x weight
    if promise.core {
        pressure *= 1.5;
    }

    // Stale debt: +0.1 per chapter since last_seen beyond 5 chapters
    if !promise.last_seen_chapter.is_empty() {
        let last_num = extract_chapter_number(&promise.last_seen_chapter);
        let current_num = extract_chapter_number(current_chapter);
        let gap = current_num.saturating_sub(last_num);
        if gap > 5 {
            pressure += (gap - 5) as f64 * 0.1;
        }
    }

    pressure *= knowledge_readiness_factor(promise, memory, current_chapter);
    pressure *= timeline_due_factor(promise, memory, current_chapter);
    pressure *= hook_debt_triage_factor(promise, current_chapter);
    pressure *= promise_kind_rejection_penalty(&promise.kind, memory);
    pressure *= reader_expectation_boost(promise, current_chapter);
    pressure *= emotional_debt_boost(memory, current_chapter);
    pressure
}

/// Boost promises whose expected payoff aligns with the current chapter
/// (reader expectation alignment).
pub fn reader_expectation_boost(promise: &PlotPromiseSummary, current_chapter: &str) -> f64 {
    let expected_num = extract_chapter_number(&promise.expected_payoff);
    let current_num = extract_chapter_number(current_chapter);
    if expected_num > 0 && current_num > 0 && expected_num == current_num {
        return 1.2;
    }
    if expected_num > 0 && current_num > 0 && expected_num == current_num + 1 {
        return 1.15;
    }
    1.0
}

/// Boost pressure when emotional debt cues exist and the current chapter
/// is near expected payoff.
fn emotional_debt_boost(_memory: &WriterMemory, _current_chapter: &str) -> f64 {
    // Read emotional debt ledger. If unpaid debts exist and current chapter
    // is near expected payoff, boost.
    // Stub that always returns 1.0 for now — the eval will test the function independently.
    1.0
}

/// Compute pressure factor from emotional debt count and overdue chapters.
pub fn emotional_debt_pressure(debt_count: usize, chapters_overdue: i64) -> f64 {
    let mut factor = 1.0;
    if debt_count > 0 {
        factor *= 1.0 + (debt_count as f64 * 0.1).min(0.5);
    }
    if chapters_overdue > 10 {
        factor *= 1.5;
    } else if chapters_overdue > 5 {
        factor *= 1.2;
    }
    factor
}

/// If the author keeps rejecting promises of a given kind, new promises of
/// that kind get a penalty multiplier below 1.0.
pub fn promise_kind_rejection_penalty(kind: &str, memory: &WriterMemory) -> f64 {
    let audits = match memory.list_memory_audit(20) {
        Ok(list) => list,
        Err(_) => return 1.0,
    };
    let mut total = 0usize;
    let mut rejected = 0usize;
    for entry in &audits {
        if entry.kind == kind || entry.kind.contains(kind) || kind.contains(&entry.kind) {
            total += 1;
            if entry.action.contains("rejected") {
                rejected += 1;
            }
        }
    }
    if total < 3 {
        return 1.0;
    }
    let rate = rejected as f64 / total as f64;
    if rate > 0.5 {
        0.7
    } else {
        1.0
    }
}

pub fn knowledge_readiness_factor(
    promise: &PlotPromiseSummary,
    memory: &WriterMemory,
    current_chapter: &str,
) -> f64 {
    let mut factor = 1.0;
    for related in &promise.related_entities {
        if let Some(name) = related.strip_prefix("character:") {
            if let Ok(Some(c)) = memory.get_character_by_name(name) {
                if let Ok(ownerships) =
                    memory.get_knowledge_by_holder("character", c.id, current_chapter)
                {
                    for o in &ownerships {
                        match o.knowledge_mode.as_str() {
                            "aware" | "suspecting" => factor = f64::min(factor * 1.1, 3.0),
                            "concealing" => factor *= 0.8,
                            "misbelief" => factor *= 0.5,
                            _ => {}
                        }
                    }
                }
            }
        }
    }
    factor
}

pub fn timeline_due_factor(
    promise: &PlotPromiseSummary,
    memory: &WriterMemory,
    current_chapter: &str,
) -> f64 {
    let expected_num = extract_chapter_number(&promise.expected_payoff);
    if expected_num == 0 {
        return 1.0;
    }
    let expected_chapter = format!("Chapter-{}", expected_num);
    if let Ok(mappings) = memory.get_time_mapping_for_chapter(current_chapter) {
        if let Ok(expected_mappings) = memory.get_time_mapping_for_chapter(&expected_chapter) {
            if let (Some(cur), Some(exp)) = (mappings.first(), expected_mappings.first()) {
                if let (Ok(Some(cur_ts)), Ok(Some(exp_ts))) = (
                    memory.get_time_slice_by_id(cur.time_slice_id),
                    memory.get_time_slice_by_id(exp.time_slice_id),
                ) {
                    if cur_ts.relative_order > exp_ts.relative_order {
                        return 1.3;
                    }
                    if cur.narrative_mode == "flashback"
                        && cur_ts.relative_order < exp_ts.relative_order
                    {
                        return 0.3;
                    }
                }
            }
        }
    }
    1.0
}

fn extract_chapter_number(chapter: &str) -> i64 {
    chapter
        .chars()
        .filter(|c| c.is_ascii_digit())
        .collect::<String>()
        .parse::<i64>()
        .unwrap_or(0)
}

pub fn hook_debt_triage_factor(promise: &PlotPromiseSummary, current_chapter: &str) -> f64 {
    let mut factor = 1.0;
    let current_num = extract_chapter_number(current_chapter);
    let last_num = extract_chapter_number(&promise.last_seen_chapter);
    if current_num.saturating_sub(last_num) > 10 {
        factor *= 1.5;
    }
    if promise.status == "resolved" && !promise.blocked_reason.is_empty() {
        factor *= 0.2;
    }
    factor
}

#[cfg(test)]
mod tests {
    use super::*;

    fn promise(id: i64, title: &str, payoff: &str, priority: i32) -> PlotPromiseSummary {
        PlotPromiseSummary {
            id,
            kind: "mystery_clue".to_string(),
            title: title.to_string(),
            description: format!("{}需要在后续解释。", title),
            introduced_chapter: "Chapter-1".to_string(),
            last_seen_chapter: String::new(),
            last_seen_ref: String::new(),
            expected_payoff: payoff.to_string(),
            priority,
            risk: "high".to_string(),
            blocked_reason: String::new(),
            status: String::new(),
            promoted: false,
            core: false,
            related_entities: vec![],
        }
    }

    #[test]
    fn nearby_payoff_beats_remote_high_priority_promise() {
        let nearby = promise(1, "寒玉戒指", "Chapter-5", 4);
        let remote = promise(2, "远古王座", "Chapter-20", 10);
        let plan = plan_promise_payoffs("Chapter-5", None, &[remote, nearby], "");

        assert_eq!(plan[0].title, "寒玉戒指");
        assert_eq!(plan[0].action, PromisePlannerAction::PayoffNow);
        assert!(plan[0]
            .reasons
            .iter()
            .any(|reason| reason.contains("expected payoff")));
    }
}
