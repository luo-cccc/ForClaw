use agent_harness_core::execution_plan::AgentCheckpoint;
use crate::writer_agent::supervised_sprint::{LongTaskCheckpoint, SprintCheckpoint, SupervisedSprintPlan};
use rusqlite::types::Type;

fn sprint_to_json(plan: &SupervisedSprintPlan) -> rusqlite::Result<String> {
    serde_json::to_string(plan).map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))
}

fn checkpoint_to_json(checkpoint: &SprintCheckpoint) -> rusqlite::Result<String> {
    serde_json::to_string(checkpoint)
        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))
}

fn sprint_from_json(raw: String, column: usize) -> rusqlite::Result<SupervisedSprintPlan> {
    serde_json::from_str(&raw).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(column, Type::Text, Box::new(e))
    })
}

fn checkpoint_from_json(raw: String, column: usize) -> rusqlite::Result<SprintCheckpoint> {
    serde_json::from_str(&raw).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(column, Type::Text, Box::new(e))
    })
}

fn parse_checkpoint_row(row: &rusqlite::Row) -> rusqlite::Result<LongTaskCheckpoint> {
    let raw: String = row.get(0)?;
    serde_json::from_str(&raw).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(0, Type::Text, Box::new(e))
    })
}

impl WriterMemory {
    pub fn upsert_supervised_sprint(
        &self,
        project_id: &str,
        plan: &SupervisedSprintPlan,
    ) -> rusqlite::Result<()> {
        let now = crate::agent_runtime::now_ms() as i64;
        let plan_json = sprint_to_json(plan)?;
        self.conn.execute(
            "INSERT INTO supervised_sprints
             (project_id, sprint_id, status, plan_json, last_checkpoint_id, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)
             ON CONFLICT(project_id, sprint_id) DO UPDATE SET
                status=excluded.status,
                plan_json=excluded.plan_json,
                last_checkpoint_id=excluded.last_checkpoint_id,
                updated_at=excluded.updated_at",
            rusqlite::params![
                project_id,
                plan.sprint_id,
                plan.status,
                plan_json,
                plan.last_checkpoint_id.clone().unwrap_or_default(),
                now,
            ],
        )?;
        Ok(())
    }

    pub fn get_supervised_sprint(
        &self,
        project_id: &str,
        sprint_id: &str,
    ) -> rusqlite::Result<Option<SupervisedSprintPlan>> {
        self.conn
            .query_row(
                "SELECT plan_json
                 FROM supervised_sprints
                 WHERE project_id=?1 AND sprint_id=?2",
                rusqlite::params![project_id, sprint_id],
                |row| sprint_from_json(row.get(0)?, 0),
            )
            .optional()
    }

    pub fn get_latest_active_supervised_sprint(
        &self,
        project_id: &str,
    ) -> rusqlite::Result<Option<SupervisedSprintPlan>> {
        self.conn
            .query_row(
                "SELECT plan_json
                 FROM supervised_sprints
                 WHERE project_id=?1 AND status IN ('planned', 'running', 'paused')
                 ORDER BY updated_at DESC
                 LIMIT 1",
                rusqlite::params![project_id],
                |row| sprint_from_json(row.get(0)?, 0),
            )
            .optional()
    }

    pub fn insert_supervised_sprint_checkpoint(
        &self,
        project_id: &str,
        checkpoint: &SprintCheckpoint,
    ) -> rusqlite::Result<()> {
        let now = crate::agent_runtime::now_ms() as i64;
        let checkpoint_json = checkpoint_to_json(checkpoint)?;
        self.conn.execute(
            "INSERT OR REPLACE INTO supervised_sprint_checkpoints
             (project_id, checkpoint_id, sprint_id, checkpoint_json, source, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                project_id,
                checkpoint.checkpoint_id,
                checkpoint.sprint_id,
                checkpoint_json,
                checkpoint.source,
                now,
            ],
        )?;
        Ok(())
    }

    pub fn get_latest_supervised_sprint_checkpoint(
        &self,
        project_id: &str,
        sprint_id: &str,
    ) -> rusqlite::Result<Option<SprintCheckpoint>> {
        self.conn
            .query_row(
                "SELECT checkpoint_json
                 FROM supervised_sprint_checkpoints
                 WHERE project_id=?1 AND sprint_id=?2
                 ORDER BY created_at DESC
                 LIMIT 1",
                rusqlite::params![project_id, sprint_id],
                |row| checkpoint_from_json(row.get(0)?, 0),
            )
            .optional()
    }

    pub fn insert_long_task_checkpoint(
        &self,
        project_id: &str,
        checkpoint: &LongTaskCheckpoint,
    ) -> rusqlite::Result<()> {
        let now = crate::agent_runtime::now_ms() as i64;
        let checkpoint_json = serde_json::to_string(checkpoint)
            .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
        self.conn.execute(
            "INSERT OR REPLACE INTO long_task_checkpoints
             (project_id, checkpoint_id, task_id, task_kind, current_step, checkpoint_json, source, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            rusqlite::params![
                project_id,
                checkpoint.checkpoint_id,
                checkpoint.task_id,
                checkpoint.task_kind,
                checkpoint.current_step,
                checkpoint_json,
                checkpoint.source,
                now,
            ],
        )?;
        Ok(())
    }

    pub fn get_latest_long_task_checkpoint(
        &self,
        project_id: &str,
        task_id: &str,
    ) -> rusqlite::Result<Option<LongTaskCheckpoint>> {
        self.conn
            .query_row(
                "SELECT checkpoint_json
                 FROM long_task_checkpoints
                 WHERE project_id=?1 AND task_id=?2
                 ORDER BY created_at DESC
                 LIMIT 1",
                rusqlite::params![project_id, task_id],
                |row| {
                    let raw: String = row.get(0)?;
                    serde_json::from_str(&raw).map_err(|e| {
                        rusqlite::Error::FromSqlConversionFailure(0, Type::Text, Box::new(e))
                    })
                },
            )
            .optional()
    }

    pub fn get_long_task_checkpoint_by_id(
        &self,
        project_id: &str,
        checkpoint_id: &str,
    ) -> rusqlite::Result<Option<LongTaskCheckpoint>> {
        self.conn
            .query_row(
                "SELECT checkpoint_json
                 FROM long_task_checkpoints
                 WHERE project_id=?1 AND checkpoint_id=?2
                 ORDER BY created_at DESC
                 LIMIT 1",
                rusqlite::params![project_id, checkpoint_id],
                |row| {
                    let raw: String = row.get(0)?;
                    serde_json::from_str(&raw).map_err(|e| {
                        rusqlite::Error::FromSqlConversionFailure(0, Type::Text, Box::new(e))
                    })
                },
            )
            .optional()
    }

    pub fn list_long_task_checkpoints(
        &self,
        project_id: &str,
        task_kind: Option<&str>,
        limit: usize,
    ) -> rusqlite::Result<Vec<LongTaskCheckpoint>> {
        let sql = if task_kind.is_some() {
            "SELECT checkpoint_json
             FROM long_task_checkpoints
             WHERE project_id=?1 AND task_kind=?2
             ORDER BY created_at DESC
             LIMIT ?3"
        } else {
            "SELECT checkpoint_json
             FROM long_task_checkpoints
             WHERE project_id=?1
             ORDER BY created_at DESC
             LIMIT ?2"
        };
        let mut stmt = self.conn.prepare(sql)?;
        let rows: rusqlite::MappedRows<'_, _> = if let Some(kind) = task_kind {
            stmt.query_map(rusqlite::params![project_id, kind, limit], parse_checkpoint_row)?
        } else {
            stmt.query_map(rusqlite::params![project_id, limit], parse_checkpoint_row)?
        };
        rows.collect::<Result<Vec<_>, _>>()
    }
    /// Write (insert or replace) a long-task checkpoint. Convenience alias
    /// for `insert_long_task_checkpoint`.
    pub fn write_long_task_checkpoint(
        &self,
        project_id: &str,
        checkpoint: &LongTaskCheckpoint,
    ) -> rusqlite::Result<()> {
        self.insert_long_task_checkpoint(project_id, checkpoint)
    }

    /// Read the latest checkpoint for a given project and task kind,
    /// regardless of task_id.
    pub fn read_latest_checkpoint(
        &self,
        project_id: &str,
        task_kind: &str,
    ) -> rusqlite::Result<Option<LongTaskCheckpoint>> {
        self.conn
            .query_row(
                "SELECT checkpoint_json
                 FROM long_task_checkpoints
                 WHERE project_id=?1 AND task_kind=?2
                 ORDER BY created_at DESC
                 LIMIT 1",
                rusqlite::params![project_id, task_kind],
                |row| {
                    let raw: String = row.get(0)?;
                    serde_json::from_str(&raw).map_err(|e| {
                        rusqlite::Error::FromSqlConversionFailure(0, Type::Text, Box::new(e))
                    })
                },
            )
            .optional()
    }

    /// Delete all checkpoints for a given task. Returns the number of rows
    /// deleted.
    pub fn clear_checkpoints_for_task(
        &self,
        project_id: &str,
        task_id: &str,
    ) -> rusqlite::Result<usize> {
        self.conn.execute(
            "DELETE FROM long_task_checkpoints
             WHERE project_id=?1 AND task_id=?2",
            rusqlite::params![project_id, task_id],
        )
    }

    /// Insert a unified AgentCheckpoint into `long_task_checkpoints`.
    pub fn insert_agent_checkpoint(
        &self,
        project_id: &str,
        checkpoint: &AgentCheckpoint,
    ) -> rusqlite::Result<()> {
        let now = crate::agent_runtime::now_ms() as i64;
        let checkpoint_json = serde_json::to_string(checkpoint)
            .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
        self.conn.execute(
            "INSERT OR REPLACE INTO long_task_checkpoints
             (project_id, checkpoint_id, task_id, task_kind, current_step, checkpoint_json, source, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            rusqlite::params![
                project_id,
                checkpoint.checkpoint_id,
                checkpoint.task_id,
                "agent_checkpoint",
                format!("{:?}", checkpoint.phase),
                checkpoint_json,
                "agent_loop",
                now,
            ],
        )?;
        Ok(())
    }

    /// Get the latest unified AgentCheckpoint for a task.
    pub fn get_latest_agent_checkpoint(
        &self,
        project_id: &str,
        task_id: &str,
    ) -> rusqlite::Result<Option<AgentCheckpoint>> {
        self.conn
            .query_row(
                "SELECT checkpoint_json
                 FROM long_task_checkpoints
                 WHERE project_id=?1 AND task_id=?2 AND task_kind='agent_checkpoint'
                 ORDER BY created_at DESC
                 LIMIT 1",
                rusqlite::params![project_id, task_id],
                |row| {
                    let raw: String = row.get(0)?;
                    serde_json::from_str::<AgentCheckpoint>(&raw).map_err(|e| {
                        rusqlite::Error::FromSqlConversionFailure(0, Type::Text, Box::new(e))
                    })
                },
            )
            .optional()
    }

    /// Get resume candidate checkpoints for a task.
    /// Returns recent checkpoints ordered newest first, limited to `limit`.
    pub fn get_resume_candidate_checkpoints(
        &self,
        project_id: &str,
        task_id: &str,
        limit: usize,
    ) -> rusqlite::Result<Vec<AgentCheckpoint>> {
        let mut stmt = self.conn.prepare(
            "SELECT checkpoint_json
             FROM long_task_checkpoints
             WHERE project_id=?1 AND task_id=?2 AND task_kind='agent_checkpoint'
             ORDER BY created_at DESC
             LIMIT ?3",
        )?;
        let rows = stmt.query_map(
            rusqlite::params![project_id, task_id, limit],
            |row| {
                let raw: String = row.get(0)?;
                serde_json::from_str::<AgentCheckpoint>(&raw).map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(0, Type::Text, Box::new(e))
                })
            },
        )?;
        rows.collect::<Result<Vec<_>, _>>()
    }

}

#[cfg(test)]
mod sprint_persistence_tests {
    use super::*;
    use crate::writer_agent::supervised_sprint::{
        checkpoint_sprint, create_sprint_plan_with_limits, record_budget_usage, LongTaskCheckpoint,
    };

    #[test]
    fn supervised_sprint_plan_and_checkpoint_persist() {
        let memory = WriterMemory::open(std::path::Path::new(":memory:")).unwrap();
        let mut plan = create_sprint_plan_with_limits(
            "sprint-1",
            &["Chapter-1".to_string(), "Chapter-2".to_string()],
            true,
            2,
            Some(10_000),
        );
        plan.status = "running".to_string();
        record_budget_usage(&mut plan, 1_500);
        let checkpoint = checkpoint_sprint(&mut plan, "unit-test");

        memory.upsert_supervised_sprint("eval", &plan).unwrap();
        memory
            .insert_supervised_sprint_checkpoint("eval", &checkpoint)
            .unwrap();

        let restored = memory
            .get_latest_active_supervised_sprint("eval")
            .unwrap()
            .unwrap();
        assert_eq!(restored.sprint_id, plan.sprint_id);
        assert_eq!(restored.spent_budget_micros, 1_500);
        assert_eq!(restored.last_checkpoint_id, Some(checkpoint.checkpoint_id.clone()));

        let restored_checkpoint = memory
            .get_latest_supervised_sprint_checkpoint("eval", "sprint-1")
            .unwrap()
            .unwrap();
        assert_eq!(restored_checkpoint.checkpoint_id, checkpoint.checkpoint_id);
    }

    #[test]
    fn long_task_checkpoint_persists_and_restores() {
        let memory = WriterMemory::open(std::path::Path::new(":memory:")).unwrap();
        let checkpoint = LongTaskCheckpoint::new(
            "cp-1",
            "task-1",
            "chapter_generation",
            "draft_produced",
            serde_json::json!({"chapter": "第三章", "revision": "rev-2"}),
        )
        .with_budget(2_500_000)
        .with_artifacts(vec!["draft-1.txt".to_string()])
        .with_source("test");

        memory
            .insert_long_task_checkpoint("proj-1", &checkpoint)
            .unwrap();

        let restored = memory
            .get_latest_long_task_checkpoint("proj-1", "task-1")
            .unwrap()
            .unwrap();
        assert_eq!(restored.checkpoint_id, checkpoint.checkpoint_id);
        assert_eq!(restored.task_kind, "chapter_generation");
        assert_eq!(restored.current_step, "draft_produced");
        assert_eq!(restored.budget_spent_micros, 2_500_000);
        assert_eq!(restored.artifact_refs, vec!["draft-1.txt"]);
    }

    #[test]
    fn long_task_checkpoint_wrong_project_returns_none() {
        let memory = WriterMemory::open(std::path::Path::new(":memory:")).unwrap();
        let checkpoint = LongTaskCheckpoint::new(
            "cp-1",
            "task-1",
            "chapter_generation",
            "save_prepared",
            serde_json::json!({}),
        );
        memory
            .insert_long_task_checkpoint("proj-a", &checkpoint)
            .unwrap();

        let wrong = memory
            .get_latest_long_task_checkpoint("proj-b", "task-1")
            .unwrap();
        assert!(wrong.is_none());
    }

    #[test]
    fn long_task_checkpoint_list_by_kind() {
        let memory = WriterMemory::open(std::path::Path::new(":memory:")).unwrap();
        for i in 0..3 {
            let cp = LongTaskCheckpoint::new(
                format!("cp-{}", i),
                "task-1",
                "chapter_generation",
                "draft",
                serde_json::json!({}),
            );
            memory.insert_long_task_checkpoint("proj-1", &cp).unwrap();
        }
        let cp_other = LongTaskCheckpoint::new(
            "cp-other",
            "task-2",
            "project_brain_rebuild",
            "analyze",
            serde_json::json!({}),
        );
        memory.insert_long_task_checkpoint("proj-1", &cp_other).unwrap();

        let all = memory.list_long_task_checkpoints("proj-1", None, 10).unwrap();
        assert_eq!(all.len(), 4);

        let filtered = memory
            .list_long_task_checkpoints("proj-1", Some("chapter_generation"), 10)
            .unwrap();
        assert_eq!(filtered.len(), 3);
    }

    #[test]
    fn chapter_generation_checkpoint_persist_and_restore() {
        let memory = WriterMemory::open(std::path::Path::new(":memory:")).unwrap();
        let request_id = "chapter-req-1";

        // Simulate context_built checkpoint
        let cp1 = LongTaskCheckpoint::new(
            format!("{}-cp-{}", request_id, 1),
            request_id,
            "chapter_generation",
            "context_built",
            serde_json::json!({"chapter_title": "第一章", "request_id": request_id, "step": "context_built"}),
        )
        .with_budget(0)
        .with_artifacts(vec![])
        .with_source("pipeline");
        memory.insert_long_task_checkpoint("proj-a", &cp1).unwrap();

        // Simulate draft_produced checkpoint
        let cp2 = LongTaskCheckpoint::new(
            format!("{}-cp-{}", request_id, 2),
            request_id,
            "chapter_generation",
            "draft_produced",
            serde_json::json!({"chapter_title": "第一章", "request_id": request_id, "step": "draft_produced"}),
        )
        .with_budget(1500)
        .with_artifacts(vec![format!("draft:{}", request_id)])
        .with_source("pipeline");
        memory.insert_long_task_checkpoint("proj-a", &cp2).unwrap();

        // Simulate save_prepared checkpoint
        let cp3 = LongTaskCheckpoint::new(
            format!("{}-cp-{}", request_id, 3),
            request_id,
            "chapter_generation",
            "save_prepared",
            serde_json::json!({"chapter_title": "第一章", "request_id": request_id, "step": "save_prepared"}),
        )
        .with_budget(3000)
        .with_artifacts(vec!["saved:第一章/rev-1".to_string()])
        .with_source("pipeline");
        memory.insert_long_task_checkpoint("proj-a", &cp3).unwrap();

        // Latest checkpoint should be save_prepared
        let latest = memory
            .get_latest_long_task_checkpoint("proj-a", request_id)
            .unwrap()
            .unwrap();
        assert_eq!(latest.current_step, "save_prepared");
        assert_eq!(latest.budget_spent_micros, 3000);
        assert_eq!(latest.artifact_refs, vec!["saved:第一章/rev-1"]);

        // List by kind should return all 3
        let candidates = memory
            .list_long_task_checkpoints("proj-a", Some("chapter_generation"), 10)
            .unwrap();
        assert_eq!(candidates.len(), 3);
        let steps: std::collections::BTreeSet<String> = candidates
            .iter()
            .map(|c| c.current_step.clone())
            .collect();
        assert!(steps.contains("save_prepared"));
        assert!(steps.contains("draft_produced"));
        assert!(steps.contains("context_built"));
    }

    #[test]
    fn chapter_generation_checkpoint_wrong_project_rejected() {
        let memory = WriterMemory::open(std::path::Path::new(":memory:")).unwrap();
        let cp = LongTaskCheckpoint::new(
            "cp-1",
            "task-1",
            "chapter_generation",
            "draft_produced",
            serde_json::json!({"chapter_title": "第一章"}),
        )
        .with_budget(1500)
        .with_source("pipeline");
        memory.insert_long_task_checkpoint("proj-a", &cp).unwrap();

        // Wrong project should return none
        let wrong = memory
            .get_latest_long_task_checkpoint("proj-b", "task-1")
            .unwrap();
        assert!(wrong.is_none());
    }

    #[test]
    fn chapter_generation_checkpoint_budget_tracked() {
        let memory = WriterMemory::open(std::path::Path::new(":memory:")).unwrap();
        let request_id = "chapter-budget-test";

        // Budget grows across checkpoints
        let cp1 = LongTaskCheckpoint::new(
            format!("{}-cp-1", request_id),
            request_id,
            "chapter_generation",
            "context_built",
            serde_json::json!({}),
        )
        .with_budget(0);
        memory.insert_long_task_checkpoint("proj-b", &cp1).unwrap();

        let cp2 = LongTaskCheckpoint::new(
            format!("{}-cp-2", request_id),
            request_id,
            "chapter_generation",
            "draft_produced",
            serde_json::json!({}),
        )
        .with_budget(2500);
        memory.insert_long_task_checkpoint("proj-b", &cp2).unwrap();

        let latest = memory
            .get_latest_long_task_checkpoint("proj-b", request_id)
            .unwrap()
            .unwrap();
        assert_eq!(latest.budget_spent_micros, 2500);
    }
    #[test]
    fn write_read_latest_by_kind_roundtrip() {
        let memory = WriterMemory::open(std::path::Path::new(":memory:")).unwrap();
        let cp = LongTaskCheckpoint::new(
            "cp-roundtrip",
            "task-abc",
            "batch_sprint",
            "draft_produced",
            serde_json::json!({"step": "draft", "data": 42}),
        )
        .with_budget(5_000)
        .with_artifacts(vec!["artifact-1.txt".to_string()])
        .with_source("unit-test");

        // Write via the new write_long_task_checkpoint alias
        memory.write_long_task_checkpoint("proj-1", &cp).unwrap();

        // Read back via read_latest_checkpoint by task_kind
        let restored = memory
            .read_latest_checkpoint("proj-1", "batch_sprint")
            .unwrap()
            .unwrap();
        assert_eq!(restored.checkpoint_id, "cp-roundtrip");
        assert_eq!(restored.task_id, "task-abc");
        assert_eq!(restored.task_kind, "batch_sprint");
        assert_eq!(restored.current_step, "draft_produced");
        assert_eq!(restored.budget_spent_micros, 5_000);
        assert_eq!(restored.artifact_refs, vec!["artifact-1.txt"]);
    }

    #[test]
    fn clear_checkpoints_after_task_completion() {
        let memory = WriterMemory::open(std::path::Path::new(":memory:")).unwrap();

        // Write two checkpoints for same task
        for i in 0..3 {
            let cp = LongTaskCheckpoint::new(
                format!("cp-{}", i),
                "task-clear",
                "chapter_generation",
                format!("step-{}", i),
                serde_json::json!({}),
            );
            memory.write_long_task_checkpoint("proj-1", &cp).unwrap();
        }

        // Verify they exist
        let before = memory
            .get_latest_long_task_checkpoint("proj-1", "task-clear")
            .unwrap();
        assert!(before.is_some());

        // Clear checkpoints for the completed task
        let deleted = memory
            .clear_checkpoints_for_task("proj-1", "task-clear")
            .unwrap();
        assert_eq!(deleted, 3);

        // Verify they are gone
        let after = memory
            .get_latest_long_task_checkpoint("proj-1", "task-clear")
            .unwrap();
        assert!(after.is_none());
    }

    #[test]
    fn read_latest_by_kind_project_mismatch_returns_none() {
        let memory = WriterMemory::open(std::path::Path::new(":memory:")).unwrap();
        let cp = LongTaskCheckpoint::new(
            "cp-mismatch",
            "task-1",
            "project_brain_rebuild",
            "analyze",
            serde_json::json!({}),
        );
        memory
            .write_long_task_checkpoint("proj-a", &cp)
            .unwrap();

        // Wrong project -> None
        let wrong_proj = memory
            .read_latest_checkpoint("proj-b", "project_brain_rebuild")
            .unwrap();
        assert!(wrong_proj.is_none());

        // Wrong task_kind -> None
        let wrong_kind = memory
            .read_latest_checkpoint("proj-a", "chapter_generation")
            .unwrap();
        assert!(wrong_kind.is_none());
    }

    #[test]
    fn clear_only_target_task_leaves_others_intact() {
        let memory = WriterMemory::open(std::path::Path::new(":memory:")).unwrap();

        // Task A checkpoints
        for i in 0..2 {
            let cp = LongTaskCheckpoint::new(
                format!("cp-a-{}", i),
                "task-a",
                "chapter_generation",
                format!("step-{}", i),
                serde_json::json!({}),
            );
            memory.write_long_task_checkpoint("proj-1", &cp).unwrap();
        }

        // Task B checkpoints
        for i in 0..3 {
            let cp = LongTaskCheckpoint::new(
                format!("cp-b-{}", i),
                "task-b",
                "batch_sprint",
                format!("step-{}", i),
                serde_json::json!({}),
            );
            memory.write_long_task_checkpoint("proj-1", &cp).unwrap();
        }

        // Clear only task A
        let deleted = memory
            .clear_checkpoints_for_task("proj-1", "task-a")
            .unwrap();
        assert_eq!(deleted, 2);

        // Task A gone
        assert!(memory
            .get_latest_long_task_checkpoint("proj-1", "task-a")
            .unwrap()
            .is_none());

        // Task B still has 3
        let b_checkpoints = memory
            .list_long_task_checkpoints("proj-1", Some("batch_sprint"), 10)
            .unwrap();
        assert_eq!(b_checkpoints.len(), 3);
    }

    #[test]
    fn agent_checkpoint_roundtrip() {
        use agent_harness_core::execution_plan::{
            AgentCheckpoint, CheckpointPhase, ProviderUsageSummary, ResumePolicy,
        };
        let memory = WriterMemory::open(std::path::Path::new(":memory:")).unwrap();
        let cp = AgentCheckpoint {
            checkpoint_id: "acp-1".to_string(),
            task_id: "task-1".to_string(),
            plan_id: "plan-1".to_string(),
            step_id: "step-0".to_string(),
            phase: CheckpointPhase::StepCompleted,
            input_hash: "hash-in".to_string(),
            context_hash: "hash-ctx".to_string(),
            artifact_refs: vec!["draft.txt".to_string()],
            tool_effects: vec!["tool:read".to_string()],
            provider_usage: Some(ProviderUsageSummary {
                model: "gpt-4".to_string(),
                prompt_tokens: 100,
                completion_tokens: 50,
                total_tokens: 150,
                duration_ms: 1200,
            }),
            budget_spent: 2500,
            approval_refs: vec!["approval-1".to_string()],
            resume_policy: ResumePolicy::Skip,
        };

        memory.insert_agent_checkpoint("proj-1", &cp).unwrap();

        let restored = memory
            .get_latest_agent_checkpoint("proj-1", "task-1")
            .unwrap()
            .unwrap();
        assert_eq!(restored.checkpoint_id, cp.checkpoint_id);
        assert_eq!(restored.phase, CheckpointPhase::StepCompleted);
        assert_eq!(restored.resume_policy, ResumePolicy::Skip);
        assert_eq!(restored.budget_spent, 2500);
        assert_eq!(restored.artifact_refs, vec!["draft.txt"]);
        assert_eq!(
            restored.provider_usage.as_ref().unwrap().total_tokens,
            150
        );
    }

    #[test]
    fn agent_checkpoint_completed_step_skips_on_resume() {
        use agent_harness_core::execution_plan::{
            AgentCheckpoint, CheckpointPhase, ResumePolicy,
        };
        let memory = WriterMemory::open(std::path::Path::new(":memory:")).unwrap();
        // Simulate a completed step checkpoint
        let cp = AgentCheckpoint {
            checkpoint_id: "acp-done".to_string(),
            task_id: "task-2".to_string(),
            plan_id: "plan-2".to_string(),
            step_id: "step-0".to_string(),
            phase: CheckpointPhase::StepCompleted,
            input_hash: String::new(),
            context_hash: String::new(),
            artifact_refs: vec!["artifact-1".to_string()],
            tool_effects: vec![],
            provider_usage: None,
            budget_spent: 1000,
            approval_refs: vec![],
            resume_policy: ResumePolicy::Skip,
        };
        memory.insert_agent_checkpoint("proj-2", &cp).unwrap();

        let candidates = memory
            .get_resume_candidate_checkpoints("proj-2", "task-2", 10)
            .unwrap();
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].resume_policy, ResumePolicy::Skip);
    }

    #[test]
    fn agent_checkpoint_save_prepared_requires_approval() {
        use agent_harness_core::execution_plan::{
            AgentCheckpoint, CheckpointPhase, ResumePolicy,
        };
        let memory = WriterMemory::open(std::path::Path::new(":memory:")).unwrap();
        let cp = AgentCheckpoint {
            checkpoint_id: "acp-save".to_string(),
            task_id: "task-3".to_string(),
            plan_id: "plan-3".to_string(),
            step_id: "save".to_string(),
            phase: CheckpointPhase::SavePrepared,
            input_hash: String::new(),
            context_hash: String::new(),
            artifact_refs: vec!["saved:ch1/rev-1".to_string()],
            tool_effects: vec![],
            provider_usage: None,
            budget_spent: 3000,
            approval_refs: vec![],
            resume_policy: ResumePolicy::RequireApproval,
        };
        memory.insert_agent_checkpoint("proj-3", &cp).unwrap();

        let latest = memory
            .get_latest_agent_checkpoint("proj-3", "task-3")
            .unwrap()
            .unwrap();
        assert_eq!(latest.phase, CheckpointPhase::SavePrepared);
        assert_eq!(latest.resume_policy, ResumePolicy::RequireApproval);
    }

    #[test]
    fn agent_checkpoint_provider_interrupt_rerun_policy() {
        use agent_harness_core::execution_plan::{
            AgentCheckpoint, CheckpointPhase, ResumePolicy,
        };
        let memory = WriterMemory::open(std::path::Path::new(":memory:")).unwrap();
        // Simulate provider call before checkpoint (interrupted during provider call)
        let cp_before = AgentCheckpoint {
            checkpoint_id: "acp-provider-before".to_string(),
            task_id: "task-4".to_string(),
            plan_id: "plan-4".to_string(),
            step_id: "step-1".to_string(),
            phase: CheckpointPhase::ProviderCallBefore,
            input_hash: String::new(),
            context_hash: String::new(),
            artifact_refs: vec![],
            tool_effects: vec![],
            provider_usage: None,
            budget_spent: 500,
            approval_refs: vec![],
            resume_policy: ResumePolicy::Rerun,
        };
        memory.insert_agent_checkpoint("proj-4", &cp_before).unwrap();

        let candidates = memory
            .get_resume_candidate_checkpoints("proj-4", "task-4", 10)
            .unwrap();
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].resume_policy, ResumePolicy::Rerun);
        assert_eq!(candidates[0].phase, CheckpointPhase::ProviderCallBefore);
    }
}
