//! Supervised Chapter Sprint — batch chapter advancement with guardrails.
//!
//! Allows authors to push through multiple chapters but enforces
//! preflight → receipt → draft → review → save → settlement per chapter.

use serde::{Deserialize, Serialize};

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
}
