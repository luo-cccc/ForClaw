use crate::writer_agent::supervised_sprint::{SprintCheckpoint, SupervisedSprintPlan};
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
}

#[cfg(test)]
mod sprint_persistence_tests {
    use super::*;
    use crate::writer_agent::supervised_sprint::{
        checkpoint_sprint, create_sprint_plan_with_limits, record_budget_usage,
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
}
