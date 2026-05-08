impl WriterMemory {
    pub fn upsert_emotional_debt_lifecycle(
        &self,
        lifecycle: &EmotionalDebtLifecycle,
    ) -> rusqlite::Result<bool> {
        self.conn.execute(
            "INSERT INTO emotional_debt_lifecycles (
                debt_id, project_id, debt_kind, relationship_soil,
                introduced_by_scene, interest_mechanism, payoff_contract,
                payoff_window, current_state, overdue_risk, rollover_target,
                source_refs_json, created_at, updated_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
            ON CONFLICT(debt_id) DO UPDATE SET
                debt_kind = excluded.debt_kind,
                relationship_soil = excluded.relationship_soil,
                introduced_by_scene = excluded.introduced_by_scene,
                interest_mechanism = excluded.interest_mechanism,
                payoff_contract = excluded.payoff_contract,
                payoff_window = excluded.payoff_window,
                current_state = excluded.current_state,
                overdue_risk = excluded.overdue_risk,
                rollover_target = excluded.rollover_target,
                source_refs_json = excluded.source_refs_json,
                updated_at = excluded.updated_at",
            rusqlite::params![
                lifecycle.debt_id,
                "project", // placeholder — set via caller context
                lifecycle.debt_kind,
                lifecycle.relationship_soil,
                lifecycle.introduced_by_scene,
                lifecycle.interest_mechanism,
                lifecycle.payoff_contract,
                lifecycle.payoff_window,
                lifecycle.current_state,
                lifecycle.overdue_risk,
                lifecycle.rollover_target,
                serde_json::to_string(&lifecycle.source_refs).unwrap_or_default(),
                lifecycle.created_at,
                lifecycle.updated_at,
            ],
        )?;
        Ok(self.conn.changes() > 0)
    }

    pub fn get_emotional_debt_lifecycle(
        &self,
        debt_id: &str,
    ) -> rusqlite::Result<Option<EmotionalDebtLifecycle>> {
        self.conn
            .query_row(
                "SELECT debt_id, debt_kind, relationship_soil, introduced_by_scene,
                        interest_mechanism, payoff_contract, payoff_window, current_state,
                        overdue_risk, rollover_target, source_refs_json, created_at, updated_at
                 FROM emotional_debt_lifecycles WHERE debt_id = ?1",
                rusqlite::params![debt_id],
                |row| {
                    Ok(EmotionalDebtLifecycle {
                        debt_id: row.get(0)?,
                        debt_kind: row.get(1)?,
                        relationship_soil: row.get(2)?,
                        introduced_by_scene: row.get(3)?,
                        interest_mechanism: row.get(4)?,
                        payoff_contract: row.get(5)?,
                        payoff_window: row.get(6)?,
                        current_state: row.get(7)?,
                        overdue_risk: row.get(8)?,
                        rollover_target: row.get(9)?,
                        source_refs: row
                            .get::<_, String>(10)
                            .ok()
                            .and_then(|j| serde_json::from_str(&j).ok())
                            .unwrap_or_default(),
                        created_at: row.get(11)?,
                        updated_at: row.get(12)?,
                    })
                },
            )
            .optional()
    }

    pub fn list_emotional_debt_lifecycles(
        &self,
        _project_id: &str,
        limit: usize,
    ) -> rusqlite::Result<Vec<EmotionalDebtLifecycle>> {
        let mut stmt = self.conn.prepare(
            "SELECT debt_id, debt_kind, relationship_soil, introduced_by_scene,
                    interest_mechanism, payoff_contract, payoff_window, current_state,
                    overdue_risk, rollover_target, source_refs_json, created_at, updated_at
             FROM emotional_debt_lifecycles ORDER BY updated_at DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(rusqlite::params![limit as i64], |row| {
            Ok(EmotionalDebtLifecycle {
                debt_id: row.get(0)?,
                debt_kind: row.get(1)?,
                relationship_soil: row.get(2)?,
                introduced_by_scene: row.get(3)?,
                interest_mechanism: row.get(4)?,
                payoff_contract: row.get(5)?,
                payoff_window: row.get(6)?,
                current_state: row.get(7)?,
                overdue_risk: row.get(8)?,
                rollover_target: row.get(9)?,
                source_refs: row
                    .get::<_, String>(10)
                    .ok()
                    .and_then(|j| serde_json::from_str(&j).ok())
                    .unwrap_or_default(),
                created_at: row.get(11)?,
                updated_at: row.get(12)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
    }

    pub fn advance_emotional_debt_state(
        &self,
        debt_id: &str,
        new_state: &str,
        source_ref: &str,
    ) -> rusqlite::Result<bool> {
        self.conn.execute(
            "UPDATE emotional_debt_lifecycles SET current_state = ?2, source_refs_json = json_insert(source_refs_json, '$[#]', ?3), updated_at = datetime('now') WHERE debt_id = ?1",
            rusqlite::params![debt_id, new_state, source_ref],
        )?;
        Ok(self.conn.changes() > 0)
    }
}
