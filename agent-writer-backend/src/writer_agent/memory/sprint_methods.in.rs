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
                checkpoint.task_kind.as_deref().unwrap_or("agent_checkpoint"),
                format!("{:?}", checkpoint.phase),
                checkpoint_json,
                checkpoint.source.as_deref().unwrap_or("agent_loop"),
                checkpoint.created_at_ms.unwrap_or(now as u64) as i64,
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
                 WHERE project_id=?1 AND task_id=?2
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
             WHERE project_id=?1 AND task_id=?2
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

    /// Get a unified AgentCheckpoint by its checkpoint_id.
    pub fn get_agent_checkpoint_by_id(
        &self,
        project_id: &str,
        checkpoint_id: &str,
    ) -> rusqlite::Result<Option<AgentCheckpoint>> {
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
                    serde_json::from_str::<AgentCheckpoint>(&raw).map_err(|e| {
                        rusqlite::Error::FromSqlConversionFailure(0, Type::Text, Box::new(e))
                    })
                },
            )
            .optional()
    }

    // ── P19: StateLedger persistence ──

    /// Save all deltas from a StateLedger entry to the database.
    pub fn save_state_ledger(
        &self,
        project_id: &str,
        entry: &crate::writer_agent::world_bible::StateLedgerEntry,
    ) -> rusqlite::Result<usize> {
        let mut inserted = 0;
        for delta in &entry.deltas {
            self.conn.execute(
                "INSERT INTO state_ledger_deltas
                 (project_id, chapter_id, delta_type, entity_id, before_state, after_state, source_constraint_id, evidence_excerpt, created_at_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                rusqlite::params![
                    project_id,
                    entry.chapter_id,
                    delta.delta_type,
                    delta.entity_id,
                    delta.before_state,
                    delta.after_state,
                    delta.source_constraint_id.as_deref().unwrap_or(""),
                    delta.evidence_excerpt,
                    entry.timestamp_ms as i64,
                ],
            )?;
            inserted += 1;
        }
        Ok(inserted)
    }

    /// Load all deltas for a project and reconstruct StateLedger entries grouped by chapter.
    pub fn load_state_ledger(
        &self,
        project_id: &str,
    ) -> rusqlite::Result<crate::writer_agent::world_bible::StateLedger> {
        let mut stmt = self.conn.prepare(
            "SELECT chapter_id, delta_type, entity_id, before_state, after_state, source_constraint_id, evidence_excerpt, created_at_ms
             FROM state_ledger_deltas
             WHERE project_id = ?1
             ORDER BY created_at_ms ASC, id ASC",
        )?;
        let rows = stmt.query_map(rusqlite::params![project_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, i64>(7)?,
            ))
        })?;

        use std::collections::BTreeMap;
        let mut chapters: BTreeMap<String, (u64, Vec<crate::writer_agent::world_bible::StateLedgerDelta>)> = BTreeMap::new();
        for row in rows {
            let (chapter_id, delta_type, entity_id, before_state, after_state, source_constraint_id, evidence_excerpt, created_at_ms) = row?;
            let entry = chapters.entry(chapter_id.clone()).or_insert_with(|| {
                (created_at_ms as u64, Vec::new())
            });
            entry.1.push(crate::writer_agent::world_bible::StateLedgerDelta {
                delta_type,
                entity_id,
                before_state,
                after_state,
                source_constraint_id: if source_constraint_id.is_empty() { None } else { Some(source_constraint_id) },
                evidence_excerpt,
            });
        }

        let entries: Vec<crate::writer_agent::world_bible::StateLedgerEntry> = chapters
            .into_iter()
            .map(|(chapter_id, (timestamp_ms, deltas))| {
                crate::writer_agent::world_bible::StateLedgerEntry {
                    chapter_id,
                    timestamp_ms,
                    deltas,
                }
            })
            .collect();

        Ok(crate::writer_agent::world_bible::StateLedger {
            project_id: project_id.to_string(),
            entries,
        })
    }

    /// List all state deltas for a project ordered by chapter_id.
    pub fn list_state_deltas_for_project(
        &self,
        project_id: &str,
    ) -> rusqlite::Result<Vec<crate::writer_agent::world_bible::StateLedgerDelta>> {
        let mut stmt = self.conn.prepare(
            "SELECT delta_type, entity_id, before_state, after_state, source_constraint_id, evidence_excerpt
             FROM state_ledger_deltas
             WHERE project_id = ?1
             ORDER BY chapter_id ASC, id ASC",
        )?;
        let rows = stmt.query_map(rusqlite::params![project_id], |row| {
            let source_constraint_id: String = row.get(4)?;
            Ok(crate::writer_agent::world_bible::StateLedgerDelta {
                delta_type: row.get(0)?,
                entity_id: row.get(1)?,
                before_state: row.get(2)?,
                after_state: row.get(3)?,
                source_constraint_id: if source_constraint_id.is_empty() { None } else { Some(source_constraint_id) },
                evidence_excerpt: row.get(5)?,
            })
        })?;
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
            task_kind: Some("chapter_generation".to_string()),
            safe_resume_payload: None,
            source: Some("pipeline".to_string()),
            created_at_ms: None,
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
            task_kind: None,
            safe_resume_payload: None,
            source: None,
            created_at_ms: None,
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
            task_kind: None,
            safe_resume_payload: None,
            source: None,
            created_at_ms: None,
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
            task_kind: None,
            safe_resume_payload: None,
            source: None,
            created_at_ms: None,
        };
        memory.insert_agent_checkpoint("proj-4", &cp_before).unwrap();

        let candidates = memory
            .get_resume_candidate_checkpoints("proj-4", "task-4", 10)
            .unwrap();
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].resume_policy, ResumePolicy::Rerun);
        assert_eq!(candidates[0].phase, CheckpointPhase::ProviderCallBefore);
    }

    // ── A3: Checkpoint roundtrip and resume tests ──

    #[test]
    fn checkpoint_roundtrip_all_fields() {
        use agent_harness_core::execution_plan::{
            AgentCheckpoint, CheckpointPhase, ProviderUsageSummary, ResumePolicy,
        };
        let memory = WriterMemory::open(std::path::Path::new(":memory:")).unwrap();
        let cp = AgentCheckpoint {
            checkpoint_id: "acp-full".to_string(),
            task_id: "task-full".to_string(),
            plan_id: "plan-full".to_string(),
            step_id: "save_prepared".to_string(),
            phase: CheckpointPhase::SavePrepared,
            input_hash: "hash-input".to_string(),
            context_hash: "hash-ctx".to_string(),
            artifact_refs: vec!["artifact-1.txt".to_string(), "artifact-2.txt".to_string()],
            tool_effects: vec!["tool:read".to_string(), "tool:write".to_string()],
            provider_usage: Some(ProviderUsageSummary {
                model: "gpt-4".to_string(),
                prompt_tokens: 1000,
                completion_tokens: 500,
                total_tokens: 1500,
                duration_ms: 3000,
            }),
            budget_spent: 5_000_000,
            approval_refs: vec!["approval-1".to_string(), "approval-2".to_string()],
            resume_policy: ResumePolicy::RequireApproval,
            task_kind: Some("chapter_generation".to_string()),
            safe_resume_payload: Some(serde_json::json!({"chapter_title": "Ch1", "step": "save"})),
            source: Some("pipeline".to_string()),
            created_at_ms: Some(1_700_000_000_000),
        };

        memory.insert_agent_checkpoint("proj-full", &cp).unwrap();

        let restored = memory
            .get_agent_checkpoint_by_id("proj-full", "acp-full")
            .unwrap()
            .unwrap();
        assert_eq!(restored.checkpoint_id, cp.checkpoint_id);
        assert_eq!(restored.task_id, cp.task_id);
        assert_eq!(restored.plan_id, cp.plan_id);
        assert_eq!(restored.step_id, cp.step_id);
        assert_eq!(restored.phase, CheckpointPhase::SavePrepared);
        assert_eq!(restored.input_hash, "hash-input");
        assert_eq!(restored.context_hash, "hash-ctx");
        assert_eq!(restored.artifact_refs, vec!["artifact-1.txt", "artifact-2.txt"]);
        assert_eq!(restored.tool_effects, vec!["tool:read", "tool:write"]);
        assert_eq!(restored.budget_spent, 5_000_000);
        assert_eq!(restored.approval_refs, vec!["approval-1", "approval-2"]);
        assert_eq!(restored.resume_policy, ResumePolicy::RequireApproval);
        assert_eq!(restored.task_kind, Some("chapter_generation".to_string()));
        assert_eq!(
            restored.safe_resume_payload.as_ref().and_then(|v| v.get("chapter_title")).and_then(|v| v.as_str()),
            Some("Ch1")
        );
        assert_eq!(restored.source, Some("pipeline".to_string()));
        assert_eq!(restored.created_at_ms, Some(1_700_000_000_000));
        let usage = restored.provider_usage.unwrap();
        assert_eq!(usage.model, "gpt-4");
        assert_eq!(usage.total_tokens, 1500);
        assert_eq!(usage.duration_ms, 3000);
    }

    #[test]
    fn resume_skips_completed_step() {
        use agent_harness_core::execution_plan::{
            AgentCheckpoint, CheckpointPhase, ResumePolicy,
        };
        let memory = WriterMemory::open(std::path::Path::new(":memory:")).unwrap();
        // Completed step with Skip policy
        let cp = AgentCheckpoint {
            checkpoint_id: "acp-skip".to_string(),
            task_id: "task-skip".to_string(),
            plan_id: "plan-skip".to_string(),
            step_id: "step-0".to_string(),
            phase: CheckpointPhase::StepCompleted,
            input_hash: String::new(),
            context_hash: String::new(),
            artifact_refs: vec!["result.txt".to_string()],
            tool_effects: vec![],
            provider_usage: None,
            budget_spent: 1000,
            approval_refs: vec![],
            resume_policy: ResumePolicy::Skip,
            task_kind: None,
            safe_resume_payload: None,
            source: None,
            created_at_ms: None,
        };
        memory.insert_agent_checkpoint("proj-skip", &cp).unwrap();

        let candidates = memory
            .get_resume_candidate_checkpoints("proj-skip", "task-skip", 10)
            .unwrap();
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].resume_policy, ResumePolicy::Skip);
        assert_eq!(candidates[0].phase, CheckpointPhase::StepCompleted);
    }

    #[test]
    fn save_prepared_resume_rechecks_conflict() {
        use agent_harness_core::execution_plan::{
            AgentCheckpoint, CheckpointPhase, ResumePolicy,
        };
        let memory = WriterMemory::open(std::path::Path::new(":memory:")).unwrap();
        // save_prepared with Rerun policy should re-do conflict check
        let cp = AgentCheckpoint {
            checkpoint_id: "acp-save-rerun".to_string(),
            task_id: "task-save".to_string(),
            plan_id: "plan-save".to_string(),
            step_id: "save_prepared".to_string(),
            phase: CheckpointPhase::SavePrepared,
            input_hash: String::new(),
            context_hash: String::new(),
            artifact_refs: vec!["saved:Ch1/rev-1".to_string()],
            tool_effects: vec![],
            provider_usage: None,
            budget_spent: 3000,
            approval_refs: vec!["auto_approved".to_string()],
            resume_policy: ResumePolicy::Rerun,
            task_kind: Some("chapter_generation".to_string()),
            safe_resume_payload: Some(serde_json::json!({"conflict_check": "passed", "output_chars": 5000})),
            source: Some("pipeline".to_string()),
            created_at_ms: None,
        };
        memory.insert_agent_checkpoint("proj-save", &cp).unwrap();

        let restored = memory
            .get_agent_checkpoint_by_id("proj-save", "acp-save-rerun")
            .unwrap()
            .unwrap();
        assert_eq!(restored.phase, CheckpointPhase::SavePrepared);
        assert_eq!(restored.resume_policy, ResumePolicy::Rerun);
        assert_eq!(restored.approval_refs, vec!["auto_approved"]);
        let payload = restored.safe_resume_payload.unwrap();
        assert_eq!(payload.get("conflict_check").and_then(|v| v.as_str()), Some("passed"));
        assert_eq!(payload.get("output_chars").and_then(|v| v.as_u64()), Some(5000));
    }

    #[test]
    fn provider_interrupt_recovery_checkpoint() {
        use agent_harness_core::execution_plan::{
            AgentCheckpoint, CheckpointPhase, ResumePolicy,
        };
        let memory = WriterMemory::open(std::path::Path::new(":memory:")).unwrap();
        // Provider call interrupted before completion
        let cp = AgentCheckpoint {
            checkpoint_id: "acp-interrupt".to_string(),
            task_id: "task-interrupt".to_string(),
            plan_id: "plan-interrupt".to_string(),
            step_id: "draft".to_string(),
            phase: CheckpointPhase::ProviderCallBefore,
            input_hash: "hash-before".to_string(),
            context_hash: "hash-ctx".to_string(),
            artifact_refs: vec![],
            tool_effects: vec![],
            provider_usage: None,
            budget_spent: 500,
            approval_refs: vec![],
            resume_policy: ResumePolicy::Rerun,
            task_kind: Some("chapter_generation".to_string()),
            safe_resume_payload: None,
            source: Some("pipeline".to_string()),
            created_at_ms: None,
        };
        memory.insert_agent_checkpoint("proj-interrupt", &cp).unwrap();

        let restored = memory
            .get_latest_agent_checkpoint("proj-interrupt", "task-interrupt")
            .unwrap()
            .unwrap();
        assert_eq!(restored.phase, CheckpointPhase::ProviderCallBefore);
        assert_eq!(restored.resume_policy, ResumePolicy::Rerun);
        assert_eq!(restored.input_hash, "hash-before");
        assert_eq!(restored.context_hash, "hash-ctx");
    }

    // ── P19: StateLedger persistence tests ──

    #[test]
    fn state_ledger_persists_and_restores() {
        use crate::writer_agent::world_bible::{StateLedgerDelta, StateLedgerEntry};
        let memory = WriterMemory::open(std::path::Path::new(":memory:")).unwrap();
        let entry = StateLedgerEntry {
            chapter_id: "ch1".to_string(),
            timestamp_ms: 1_700_000_000_000,
            deltas: vec![
                StateLedgerDelta {
                    delta_type: "knowledge".to_string(),
                    entity_id: "hero".to_string(),
                    before_state: "ignorant".to_string(),
                    after_state: "knows truth".to_string(),
                    source_constraint_id: Some("c1".to_string()),
                    evidence_excerpt: "he learned".to_string(),
                },
                StateLedgerDelta {
                    delta_type: "relationship".to_string(),
                    entity_id: "alice_bob".to_string(),
                    before_state: "strangers".to_string(),
                    after_state: "friends".to_string(),
                    source_constraint_id: None,
                    evidence_excerpt: "they shook hands".to_string(),
                },
            ],
        };

        let inserted = memory.save_state_ledger("proj-a", &entry).unwrap();
        assert_eq!(inserted, 2);

        let restored = memory.load_state_ledger("proj-a").unwrap();
        assert_eq!(restored.project_id, "proj-a");
        assert_eq!(restored.entries.len(), 1);
        assert_eq!(restored.entries[0].chapter_id, "ch1");
        assert_eq!(restored.entries[0].deltas.len(), 2);
        assert_eq!(restored.entries[0].deltas[0].entity_id, "hero");
        assert_eq!(restored.entries[0].deltas[0].after_state, "knows truth");
        assert_eq!(restored.entries[0].deltas[0].source_constraint_id, Some("c1".to_string()));
        assert_eq!(restored.entries[0].deltas[1].entity_id, "alice_bob");
        assert_eq!(restored.entries[0].deltas[1].source_constraint_id, None);
    }

    #[test]
    fn state_ledger_query_by_project() {
        use crate::writer_agent::world_bible::{StateLedgerDelta, StateLedgerEntry};
        let memory = WriterMemory::open(std::path::Path::new(":memory:")).unwrap();

        // Insert for proj-a
        let entry_a = StateLedgerEntry {
            chapter_id: "ch1".to_string(),
            timestamp_ms: 1,
            deltas: vec![StateLedgerDelta {
                delta_type: "knowledge".to_string(),
                entity_id: "hero".to_string(),
                before_state: "a".to_string(),
                after_state: "b".to_string(),
                source_constraint_id: None,
                evidence_excerpt: "ex".to_string(),
            }],
        };
        memory.save_state_ledger("proj-a", &entry_a).unwrap();

        // Insert for proj-b
        let entry_b = StateLedgerEntry {
            chapter_id: "ch2".to_string(),
            timestamp_ms: 2,
            deltas: vec![StateLedgerDelta {
                delta_type: "possession".to_string(),
                entity_id: "item".to_string(),
                before_state: "none".to_string(),
                after_state: "has".to_string(),
                source_constraint_id: Some("cost".to_string()),
                evidence_excerpt: "found".to_string(),
            }],
        };
        memory.save_state_ledger("proj-b", &entry_b).unwrap();

        // Query proj-a should only return proj-a deltas
        let deltas_a = memory.list_state_deltas_for_project("proj-a").unwrap();
        assert_eq!(deltas_a.len(), 1);
        assert_eq!(deltas_a[0].entity_id, "hero");

        // Query proj-b should only return proj-b deltas
        let deltas_b = memory.list_state_deltas_for_project("proj-b").unwrap();
        assert_eq!(deltas_b.len(), 1);
        assert_eq!(deltas_b[0].entity_id, "item");
        assert_eq!(deltas_b[0].source_constraint_id, Some("cost".to_string()));

        // Wrong project should return empty
        let deltas_wrong = memory.list_state_deltas_for_project("proj-c").unwrap();
        assert!(deltas_wrong.is_empty());
    }
}
