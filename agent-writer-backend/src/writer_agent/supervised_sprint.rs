//! Supervised Chapter Sprint — batch chapter advancement with guardrails.
//!
//! Allows authors to push through multiple chapters but enforces
//! preflight → receipt → draft → review → save → settlement per chapter.

use serde::{Deserialize, Serialize};

use crate::chapter_generation::ChapterQualityReport;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SupervisedSprintPlan {
    pub sprint_id: String,
    pub chapters: Vec<SprintChapterTarget>,
    pub total_chapters: usize,
    pub current_index: usize,
    pub status: String, // "planned" | "running" | "paused" | "completed"
    pub require_approval_per_chapter: bool,
    pub max_chapters_per_session: usize,
    pub spent_budget_micros: u64,
    pub budget_ceiling_micros: Option<u64>,
    pub checkpoint_count: usize,
    pub last_checkpoint_id: Option<String>,
    #[serde(default)]
    pub minimum_quality_score: f32,
    #[serde(default)]
    pub stop_on_fatal_issue: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quality_mode: Option<crate::chapter_generation::GenerationQualityMode>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SprintChapterTarget {
    pub chapter_title: String,
    pub chapter_number: usize,
    pub status: String, // "pending" | "preflight" | "drafting" | "review" | "saved" | "settled"
    pub receipt_id: Option<String>,
    pub preflight_readiness: Option<String>,
    pub requires_author_review: bool,
    pub last_error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quality_mode: Option<crate::chapter_generation::GenerationQualityMode>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SprintProgress {
    pub sprint_id: String,
    pub status: String,
    pub chapters_completed: usize,
    pub chapters_remaining: usize,
    pub current_chapter: Option<String>,
    pub receipts_recorded: usize,
    pub settlements_completed: usize,
    pub last_error: Option<String>,
    pub checkpoint_count: usize,
    pub spent_budget_micros: u64,
    pub budget_ceiling_micros: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SprintCheckpoint {
    pub checkpoint_id: String,
    pub sprint_id: String,
    pub status: String,
    pub current_index: usize,
    pub current_chapter: Option<String>,
    pub receipts_recorded: usize,
    pub settlements_completed: usize,
    pub spent_budget_micros: u64,
    pub budget_ceiling_micros: Option<u64>,
    pub source: String,
}

/// General-purpose checkpoint for long-running tasks (chapter generation,
/// batch sprint, Project Brain rebuild, etc.).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LongTaskCheckpoint {
    pub checkpoint_id: String,
    pub task_id: String,
    pub task_kind: String, // "chapter_generation" | "batch_sprint" | "project_brain_rebuild" | ...
    pub current_step: String,
    pub safe_resume_payload: serde_json::Value,
    pub budget_spent_micros: u64,
    pub artifact_refs: Vec<String>,
    pub source: String,
    pub created_at_ms: u64,
}

impl LongTaskCheckpoint {
    pub fn new(
        checkpoint_id: impl Into<String>,
        task_id: impl Into<String>,
        task_kind: impl Into<String>,
        current_step: impl Into<String>,
        safe_resume_payload: serde_json::Value,
    ) -> Self {
        Self {
            checkpoint_id: checkpoint_id.into(),
            task_id: task_id.into(),
            task_kind: task_kind.into(),
            current_step: current_step.into(),
            safe_resume_payload,
            budget_spent_micros: 0,
            artifact_refs: Vec::new(),
            source: String::new(),
            created_at_ms: crate::agent_runtime::now_ms(),
        }
    }

    pub fn with_budget(mut self, budget_spent_micros: u64) -> Self {
        self.budget_spent_micros = budget_spent_micros;
        self
    }

    pub fn with_artifacts(mut self, artifact_refs: Vec<String>) -> Self {
        self.artifact_refs = artifact_refs;
        self
    }

    pub fn with_source(mut self, source: impl Into<String>) -> Self {
        self.source = source.into();
        self
    }
}

// ── A3: Unified checkpoint conversions ──

impl From<&LongTaskCheckpoint> for agent_harness_core::execution_plan::AgentCheckpoint {
    fn from(cp: &LongTaskCheckpoint) -> Self {
        use agent_harness_core::execution_plan::ResumePolicy;
        let phase = checkpoint_phase_from_step(&cp.current_step);
        Self {
            checkpoint_id: cp.checkpoint_id.clone(),
            task_id: cp.task_id.clone(),
            plan_id: String::new(),
            step_id: cp.current_step.clone(),
            phase,
            input_hash: String::new(),
            context_hash: String::new(),
            artifact_refs: cp.artifact_refs.clone(),
            tool_effects: vec![],
            provider_usage: None,
            budget_spent: cp.budget_spent_micros,
            approval_refs: vec![],
            resume_policy: ResumePolicy::Rerun,
            task_kind: Some(cp.task_kind.clone()),
            safe_resume_payload: Some(cp.safe_resume_payload.clone()),
            source: Some(cp.source.clone()),
            created_at_ms: Some(cp.created_at_ms),
        }
    }
}

impl From<agent_harness_core::execution_plan::AgentCheckpoint> for LongTaskCheckpoint {
    fn from(cp: agent_harness_core::execution_plan::AgentCheckpoint) -> Self {
        let mut payload = cp
            .safe_resume_payload
            .unwrap_or_else(|| serde_json::json!({}));
        if let serde_json::Value::Object(ref mut map) = payload {
            map.insert(
                "chapter_title".to_string(),
                serde_json::json!(cp.task_id.clone()),
            );
            map.insert("request_id".to_string(), serde_json::json!(cp.task_id));
            map.insert("step".to_string(), serde_json::json!(cp.step_id));
        }
        Self {
            checkpoint_id: cp.checkpoint_id,
            task_id: cp.task_id,
            task_kind: cp
                .task_kind
                .unwrap_or_else(|| "agent_checkpoint".to_string()),
            current_step: cp.step_id,
            safe_resume_payload: payload,
            budget_spent_micros: cp.budget_spent,
            artifact_refs: cp.artifact_refs,
            source: cp.source.unwrap_or_default(),
            created_at_ms: cp
                .created_at_ms
                .unwrap_or_else(crate::agent_runtime::now_ms),
        }
    }
}

/// Map a legacy `LongTaskCheckpoint.current_step` string to a `CheckpointPhase`.
fn checkpoint_phase_from_step(step: &str) -> agent_harness_core::execution_plan::CheckpointPhase {
    use agent_harness_core::execution_plan::CheckpointPhase;
    match step {
        "context_built" | "preflight" => CheckpointPhase::StepStarted,
        "draft_produced" | "draft" => CheckpointPhase::StepCompleted,
        "quality_report_produced" | "revision" => CheckpointPhase::StepCompleted,
        "save_prepared" => CheckpointPhase::SavePrepared,
        "write_before" => CheckpointPhase::WriteBefore,
        "write_after" | "settled" | "saved" => CheckpointPhase::WriteAfter,
        _ => CheckpointPhase::StepStarted,
    }
}

/// Create a supervised sprint plan from a list of chapter titles.
pub fn create_sprint_plan(
    sprint_id: &str,
    chapter_titles: &[String],
    require_approval: bool,
) -> SupervisedSprintPlan {
    create_sprint_plan_with_limits(
        sprint_id,
        chapter_titles,
        require_approval,
        chapter_titles.len(),
        None,
    )
}

pub fn create_sprint_plan_with_limits(
    sprint_id: &str,
    chapter_titles: &[String],
    require_approval: bool,
    max_chapters_per_session: usize,
    budget_ceiling_micros: Option<u64>,
) -> SupervisedSprintPlan {
    let chapters: Vec<SprintChapterTarget> = chapter_titles
        .iter()
        .enumerate()
        .map(|(i, title)| SprintChapterTarget {
            chapter_title: title.clone(),
            chapter_number: i + 1,
            status: "pending".to_string(),
            receipt_id: None,
            preflight_readiness: None,
            requires_author_review: require_approval,
            last_error: None,
            quality_mode: None,
        })
        .collect();

    SupervisedSprintPlan {
        sprint_id: sprint_id.to_string(),
        total_chapters: chapters.len(),
        current_index: 0,
        chapters,
        status: "planned".to_string(),
        require_approval_per_chapter: require_approval,
        max_chapters_per_session: max_chapters_per_session.max(1),
        spent_budget_micros: 0,
        budget_ceiling_micros,
        checkpoint_count: 0,
        last_checkpoint_id: None,
        minimum_quality_score: 0.4,
        stop_on_fatal_issue: true,
        quality_mode: None,
    }
}

/// Check if the sprint can advance to the next chapter.
pub fn can_advance_to_next_chapter(sprint: &SupervisedSprintPlan) -> bool {
    if sprint.current_index >= sprint.total_chapters {
        return false;
    }
    if sprint.status == "paused" || sprint.status == "cancelled" {
        return false;
    }
    if sprint.current_index >= sprint.max_chapters_per_session {
        return false;
    }
    if budget_ceiling_reached(sprint) {
        return false;
    }

    let current = &sprint.chapters[sprint.current_index];

    // Must have receipt AND preflight passed AND (if approval required) author review done.
    let has_receipt = current.receipt_id.is_some();
    let preflight_ok = current
        .preflight_readiness
        .as_deref()
        .map(|r| r != "blocked")
        .unwrap_or(false);

    if sprint.require_approval_per_chapter {
        has_receipt && preflight_ok && current.status == "saved"
    } else {
        has_receipt && preflight_ok
    }
}

pub fn pause_sprint(sprint: &mut SupervisedSprintPlan) -> bool {
    if sprint.status == "completed" || sprint.status == "cancelled" {
        return false;
    }
    sprint.status = "paused".to_string();
    true
}

pub fn resume_sprint(sprint: &mut SupervisedSprintPlan) -> bool {
    if sprint.status != "paused" || budget_ceiling_reached(sprint) {
        return false;
    }
    sprint.status = "running".to_string();
    true
}

pub fn cancel_sprint(sprint: &mut SupervisedSprintPlan) {
    sprint.status = "cancelled".to_string();
}

pub fn budget_ceiling_reached(sprint: &SupervisedSprintPlan) -> bool {
    sprint
        .budget_ceiling_micros
        .is_some_and(|ceiling| sprint.spent_budget_micros >= ceiling)
}

pub fn record_budget_usage(sprint: &mut SupervisedSprintPlan, spent_micros: u64) -> bool {
    sprint.spent_budget_micros = sprint.spent_budget_micros.saturating_add(spent_micros);
    !budget_ceiling_reached(sprint)
}

pub fn checkpoint_sprint(sprint: &mut SupervisedSprintPlan, source: &str) -> SprintCheckpoint {
    sprint.checkpoint_count = sprint.checkpoint_count.saturating_add(1);
    let checkpoint_id = format!("{}-cp-{}", sprint.sprint_id, sprint.checkpoint_count);
    sprint.last_checkpoint_id = Some(checkpoint_id.clone());
    SprintCheckpoint {
        checkpoint_id,
        sprint_id: sprint.sprint_id.clone(),
        status: sprint.status.clone(),
        current_index: sprint.current_index,
        current_chapter: if sprint.current_index < sprint.total_chapters {
            Some(sprint.chapters[sprint.current_index].chapter_title.clone())
        } else {
            None
        },
        receipts_recorded: sprint
            .chapters
            .iter()
            .filter(|chapter| chapter.receipt_id.is_some())
            .count(),
        settlements_completed: sprint
            .chapters
            .iter()
            .filter(|chapter| chapter.status == "settled")
            .count(),
        spent_budget_micros: sprint.spent_budget_micros,
        budget_ceiling_micros: sprint.budget_ceiling_micros,
        source: source.to_string(),
    }
}

pub fn restore_from_checkpoint(
    sprint: &mut SupervisedSprintPlan,
    checkpoint: &SprintCheckpoint,
) -> bool {
    if sprint.sprint_id != checkpoint.sprint_id {
        return false;
    }
    sprint.status = checkpoint.status.clone();
    sprint.current_index = checkpoint.current_index.min(sprint.total_chapters);
    sprint.spent_budget_micros = checkpoint.spent_budget_micros;
    sprint.budget_ceiling_micros = checkpoint.budget_ceiling_micros;
    sprint.last_checkpoint_id = Some(checkpoint.checkpoint_id.clone());
    sprint.checkpoint_count = sprint.checkpoint_count.max(
        checkpoint
            .checkpoint_id
            .rsplit('-')
            .next()
            .and_then(|raw| raw.parse::<usize>().ok())
            .unwrap_or(sprint.checkpoint_count),
    );
    true
}

/// Skip chapters that are already saved/settled according to a
/// `LongTaskCheckpoint` safe-resume payload. The payload should contain
/// `saved_chapters: ["chapter_title", ...]`. Returns how many chapters
/// were skipped.
pub fn skip_saved_chapters_from_checkpoint(
    sprint: &mut SupervisedSprintPlan,
    checkpoint: &LongTaskCheckpoint,
) -> usize {
    let saved: Vec<String> = checkpoint
        .safe_resume_payload
        .get("saved_chapters")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    if saved.is_empty() {
        return 0;
    }
    let mut skipped = 0usize;
    for chapter in &mut sprint.chapters {
        if saved.contains(&chapter.chapter_title) && chapter.status != "settled" {
            chapter.status = "settled".to_string();
            skipped += 1;
        }
    }
    // Advance current_index past settled chapters
    while sprint.current_index < sprint.total_chapters
        && sprint.chapters[sprint.current_index].status == "settled"
    {
        sprint.current_index += 1;
    }
    skipped
}

pub fn update_current_chapter_state(
    sprint: &mut SupervisedSprintPlan,
    status: Option<&str>,
    receipt_id: Option<&str>,
    preflight_readiness: Option<&str>,
    last_error: Option<&str>,
) -> bool {
    if sprint.current_index >= sprint.total_chapters {
        return false;
    }
    let current = &mut sprint.chapters[sprint.current_index];
    if let Some(status) = status {
        current.status = status.to_string();
    }
    if let Some(receipt_id) = receipt_id {
        current.receipt_id = Some(receipt_id.to_string());
    }
    if let Some(preflight_readiness) = preflight_readiness {
        current.preflight_readiness = Some(preflight_readiness.to_string());
    }
    if let Some(last_error) = last_error {
        current.last_error = Some(last_error.to_string());
    }
    true
}

/// Advance sprint to the next chapter. Returns the new current chapter title.
pub fn advance_sprint(sprint: &mut SupervisedSprintPlan) -> Option<String> {
    if !can_advance_to_next_chapter(sprint) {
        return None;
    }

    sprint.current_index += 1;

    if sprint.current_index >= sprint.total_chapters {
        sprint.status = "completed".to_string();
        None
    } else {
        sprint.status = "running".to_string();
        sprint.chapters[sprint.current_index].status = "preflight".to_string();
        Some(sprint.chapters[sprint.current_index].chapter_title.clone())
    }
}

/// Quality gate check that blocks sprint advancement when chapter quality
/// drops below the configured threshold.
pub fn check_sprint_quality_gate(
    sprint: &SupervisedSprintPlan,
    quality_report: Option<&ChapterQualityReport>,
) -> Result<(), String> {
    let Some(qr) = quality_report else {
        return Ok(()); // No quality report available, allow progression
    };

    if qr.overall_score < sprint.minimum_quality_score {
        return Err(format!(
            "Sprint quality gate: overall score {:.2} below minimum {:.2}",
            qr.overall_score, sprint.minimum_quality_score
        ));
    }

    if sprint.stop_on_fatal_issue && !qr.no_fatal_issue {
        return Err(format!(
            "Sprint quality gate: fatal issue detected in chapter {}",
            qr.chapter_title
        ));
    }

    // strict 模式下，hard world consistency violations 也视为 fatal
    let has_hard_world_violation = qr.world_consistency_violations.iter().any(|v| {
        matches!(
            v.severity,
            crate::writer_agent::world_bible::ConstraintSeverity::Hard
        )
    });
    if sprint.stop_on_fatal_issue && has_hard_world_violation {
        return Err(format!(
            "Sprint quality gate: hard world consistency violation detected in chapter {}",
            qr.chapter_title
        ));
    }

    Ok(())
}

/// Configure quality gate thresholds on an active sprint plan.
pub fn set_sprint_quality_gate(
    sprint: &mut SupervisedSprintPlan,
    minimum_quality_score: Option<f32>,
    stop_on_fatal_issue: Option<bool>,
) {
    if let Some(score) = minimum_quality_score {
        sprint.minimum_quality_score = score.clamp(0.0, 1.0);
    }
    if let Some(stop) = stop_on_fatal_issue {
        sprint.stop_on_fatal_issue = stop;
    }
}

/// Build a progress report for the sprint.
pub fn sprint_progress(sprint: &SupervisedSprintPlan) -> SprintProgress {
    let completed = sprint.current_index;
    let remaining = sprint.total_chapters.saturating_sub(completed);
    let current = if sprint.current_index < sprint.total_chapters {
        Some(sprint.chapters[sprint.current_index].chapter_title.clone())
    } else {
        None
    };

    SprintProgress {
        sprint_id: sprint.sprint_id.clone(),
        status: sprint.status.clone(),
        chapters_completed: completed,
        chapters_remaining: remaining,
        current_chapter: current,
        receipts_recorded: sprint
            .chapters
            .iter()
            .filter(|c| c.receipt_id.is_some())
            .count(),
        settlements_completed: sprint
            .chapters
            .iter()
            .filter(|c| c.status == "settled")
            .count(),
        last_error: sprint
            .chapters
            .iter()
            .rev()
            .find_map(|chapter| chapter.last_error.clone()),
        checkpoint_count: sprint.checkpoint_count,
        spent_budget_micros: sprint.spent_budget_micros,
        budget_ceiling_micros: sprint.budget_ceiling_micros,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sprint_stops_before_unapproved_save() {
        let mut sprint = create_sprint_plan(
            "s1",
            &["Ch1".to_string(), "Ch2".to_string()],
            true, // require approval
        );

        // Set up Ch1 with receipt + preflight but NOT saved (no author approval yet)
        sprint.chapters[0].receipt_id = Some("receipt-1".to_string());
        sprint.chapters[0].preflight_readiness = Some("ready".to_string());
        sprint.chapters[0].status = "drafting".to_string();

        assert!(
            !can_advance_to_next_chapter(&sprint),
            "Should NOT advance without author save when approval required"
        );
    }

    #[test]
    fn sprint_carries_forward_settlement() {
        let mut sprint = create_sprint_plan("s2", &["Ch1".to_string(), "Ch2".to_string()], false);

        sprint.chapters[0].receipt_id = Some("r1".to_string());
        sprint.chapters[0].preflight_readiness = Some("ready".to_string());
        sprint.chapters[0].status = "settled".to_string();

        assert!(can_advance_to_next_chapter(&sprint));
        let next = advance_sprint(&mut sprint);
        assert_eq!(next, Some("Ch2".to_string()));
        assert_eq!(sprint.chapters[1].status, "preflight");
    }

    #[test]
    fn sprint_records_receipts_per_chapter() {
        let sprint = create_sprint_plan(
            "s3",
            &["Ch1".to_string(), "Ch2".to_string(), "Ch3".to_string()],
            false,
        );
        assert_eq!(sprint.total_chapters, 3);
        // Each chapter target should have space for receipt
        for chapter in &sprint.chapters {
            assert!(chapter.receipt_id.is_none(), "receipts start empty");
        }
    }

    #[test]
    fn sprint_can_pause_checkpoint_and_resume() {
        let mut sprint = create_sprint_plan_with_limits(
            "s4",
            &["Ch1".to_string(), "Ch2".to_string()],
            false,
            2,
            Some(10_000),
        );
        sprint.status = "running".to_string();
        sprint.chapters[0].receipt_id = Some("r1".to_string());
        sprint.chapters[0].preflight_readiness = Some("ready".to_string());
        sprint.chapters[0].status = "saved".to_string();
        assert!(pause_sprint(&mut sprint));
        let checkpoint = checkpoint_sprint(&mut sprint, "unit-test");
        assert!(restore_from_checkpoint(&mut sprint, &checkpoint));
        assert!(resume_sprint(&mut sprint));
        assert_eq!(sprint.status, "running");
    }

    #[test]
    fn quality_gate_allows_when_no_report() {
        let sprint = create_sprint_plan("sq1", &["Ch1".to_string()], false);
        assert!(check_sprint_quality_gate(&sprint, None).is_ok());
    }

    #[test]
    fn quality_gate_blocks_low_overall_score() {
        let mut sprint = create_sprint_plan("sq2", &["Ch1".to_string()], false);
        sprint.minimum_quality_score = 0.5;
        let report = ChapterQualityReport {
            chapter_title: "Ch1".to_string(),
            overall_score: 0.3,
            fatal_issues: vec![],
            major_issues: vec![],
            metric_results: vec![],
            top_revision_targets: vec![],
            no_fatal_issue: true,
            world_consistency_violations: Vec::new(),
            canon_constraint_violations: Vec::new(),
        };
        assert!(check_sprint_quality_gate(&sprint, Some(&report)).is_err());
    }

    #[test]
    fn quality_gate_allows_acceptable_score() {
        let mut sprint = create_sprint_plan("sq3", &["Ch1".to_string()], false);
        sprint.minimum_quality_score = 0.4;
        let report = ChapterQualityReport {
            chapter_title: "Ch1".to_string(),
            overall_score: 0.6,
            fatal_issues: vec![],
            major_issues: vec![],
            metric_results: vec![],
            top_revision_targets: vec![],
            no_fatal_issue: true,
            world_consistency_violations: Vec::new(),
            canon_constraint_violations: Vec::new(),
        };
        assert!(check_sprint_quality_gate(&sprint, Some(&report)).is_ok());
    }

    #[test]
    fn quality_gate_blocks_on_fatal_issue() {
        let mut sprint = create_sprint_plan("sq4", &["Ch1".to_string()], false);
        sprint.stop_on_fatal_issue = true;
        let report = ChapterQualityReport {
            chapter_title: "Ch1".to_string(),
            overall_score: 0.8,
            fatal_issues: vec![],
            major_issues: vec![],
            metric_results: vec![],
            top_revision_targets: vec![],
            no_fatal_issue: false,
            world_consistency_violations: Vec::new(),
            canon_constraint_violations: Vec::new(),
        };
        assert!(check_sprint_quality_gate(&sprint, Some(&report)).is_err());
    }

    #[test]
    fn set_sprint_quality_gate_clamps_score() {
        let mut sprint = create_sprint_plan("sq5", &["Ch1".to_string()], false);
        set_sprint_quality_gate(&mut sprint, Some(1.5), None);
        assert_eq!(sprint.minimum_quality_score, 1.0);
        set_sprint_quality_gate(&mut sprint, Some(-0.5), None);
        assert_eq!(sprint.minimum_quality_score, 0.0);
    }

    #[test]
    fn set_sprint_quality_gate_updates_stop_flag() {
        let mut sprint = create_sprint_plan("sq6", &["Ch1".to_string()], false);
        assert!(sprint.stop_on_fatal_issue, "default is true");
        set_sprint_quality_gate(&mut sprint, None, Some(false));
        assert!(!sprint.stop_on_fatal_issue);
    }

    #[test]
    fn sprint_budget_ceiling_blocks_advance() {
        let mut sprint = create_sprint_plan_with_limits(
            "s5",
            &["Ch1".to_string(), "Ch2".to_string()],
            false,
            2,
            Some(500),
        );
        sprint.chapters[0].receipt_id = Some("r1".to_string());
        sprint.chapters[0].preflight_readiness = Some("ready".to_string());
        sprint.chapters[0].status = "saved".to_string();
        assert!(!budget_ceiling_reached(&sprint));
        assert!(!record_budget_usage(&mut sprint, 600));
        assert!(!can_advance_to_next_chapter(&sprint));
    }

    #[test]
    fn skip_saved_chapters_advances_index() {
        let mut sprint = create_sprint_plan(
            "skip-test",
            &["Ch1".to_string(), "Ch2".to_string(), "Ch3".to_string()],
            false,
        );
        sprint.chapters[0].status = "saved".to_string();
        sprint.current_index = 0;

        let checkpoint = LongTaskCheckpoint::new(
            "cp-1",
            "task-1",
            "batch_sprint",
            "save_prepared",
            serde_json::json!({"saved_chapters": ["Ch1", "Ch2"]}),
        );
        let skipped = skip_saved_chapters_from_checkpoint(&mut sprint, &checkpoint);
        assert_eq!(skipped, 2); // Ch1 and Ch2 both matched saved_chapters
        assert_eq!(sprint.current_index, 2); // advanced past Ch1 and Ch2
        assert_eq!(sprint.chapters[0].status, "settled");
        assert_eq!(sprint.chapters[1].status, "settled");
    }

    #[test]
    fn long_task_checkpoint_builder() {
        let cp = LongTaskCheckpoint::new(
            "cp-1",
            "t1",
            "chapter_generation",
            "draft",
            serde_json::json!({}),
        )
        .with_budget(1_200_000)
        .with_artifacts(vec!["a.txt".to_string()])
        .with_source("test");
        assert_eq!(cp.budget_spent_micros, 1_200_000);
        assert_eq!(cp.artifact_refs, vec!["a.txt"]);
        assert_eq!(cp.source, "test");
    }

    // ── A3: Unified checkpoint conversion tests ──

    #[test]
    fn long_task_to_agent_checkpoint_conversion() {
        let lt = LongTaskCheckpoint::new(
            "cp-1",
            "task-1",
            "chapter_generation",
            "draft_produced",
            serde_json::json!({"chapter_title": "Ch1", "request_id": "req-1"}),
        )
        .with_budget(2_500_000)
        .with_artifacts(vec!["draft.txt".to_string()])
        .with_source("pipeline");

        let agent_cp: agent_harness_core::execution_plan::AgentCheckpoint = (&lt).into();
        assert_eq!(agent_cp.checkpoint_id, "cp-1");
        assert_eq!(agent_cp.task_id, "task-1");
        assert_eq!(agent_cp.step_id, "draft_produced");
        assert_eq!(agent_cp.budget_spent, 2_500_000);
        assert_eq!(agent_cp.artifact_refs, vec!["draft.txt"]);
        assert_eq!(agent_cp.task_kind, Some("chapter_generation".to_string()));
        assert_eq!(agent_cp.source, Some("pipeline".to_string()));
        assert_eq!(
            agent_cp.phase,
            agent_harness_core::execution_plan::CheckpointPhase::StepCompleted
        );
    }

    #[test]
    fn agent_to_long_task_checkpoint_conversion() {
        let agent_cp = agent_harness_core::execution_plan::AgentCheckpoint {
            checkpoint_id: "acp-1".to_string(),
            task_id: "task-1".to_string(),
            plan_id: "plan-1".to_string(),
            step_id: "save_prepared".to_string(),
            phase: agent_harness_core::execution_plan::CheckpointPhase::SavePrepared,
            input_hash: "hash-in".to_string(),
            context_hash: "hash-ctx".to_string(),
            artifact_refs: vec!["saved:Ch1/rev-1".to_string()],
            tool_effects: vec![],
            provider_usage: None,
            budget_spent: 3_000,
            approval_refs: vec!["approval-1".to_string()],
            resume_policy: agent_harness_core::execution_plan::ResumePolicy::RequireApproval,
            task_kind: Some("chapter_generation".to_string()),
            safe_resume_payload: Some(serde_json::json!({"extra": "data"})),
            source: Some("pipeline".to_string()),
            created_at_ms: Some(1_700_000_000_000),
        };

        let lt: LongTaskCheckpoint = agent_cp.into();
        assert_eq!(lt.checkpoint_id, "acp-1");
        assert_eq!(lt.task_id, "task-1");
        assert_eq!(lt.task_kind, "chapter_generation");
        assert_eq!(lt.current_step, "save_prepared");
        assert_eq!(lt.budget_spent_micros, 3_000);
        assert_eq!(lt.artifact_refs, vec!["saved:Ch1/rev-1"]);
        assert_eq!(lt.source, "pipeline");
        assert_eq!(lt.created_at_ms, 1_700_000_000_000);
        // safe_resume_payload should have merged fields
        assert_eq!(
            lt.safe_resume_payload
                .get("chapter_title")
                .and_then(|v| v.as_str()),
            Some("task-1")
        );
        assert_eq!(
            lt.safe_resume_payload.get("extra").and_then(|v| v.as_str()),
            Some("data")
        );
    }

    #[test]
    fn checkpoint_phase_mapping_from_step() {
        use agent_harness_core::execution_plan::CheckpointPhase;
        assert_eq!(
            checkpoint_phase_from_step("context_built"),
            CheckpointPhase::StepStarted
        );
        assert_eq!(
            checkpoint_phase_from_step("draft_produced"),
            CheckpointPhase::StepCompleted
        );
        assert_eq!(
            checkpoint_phase_from_step("save_prepared"),
            CheckpointPhase::SavePrepared
        );
        assert_eq!(
            checkpoint_phase_from_step("write_after"),
            CheckpointPhase::WriteAfter
        );
        assert_eq!(
            checkpoint_phase_from_step("unknown_step"),
            CheckpointPhase::StepStarted
        );
    }
}
