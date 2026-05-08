impl WriterMemory {
    #[allow(clippy::too_many_arguments)]
    pub fn add_emotional_debt(
        &self,
        project_id: &str,
        debt_kind: &str,
        title: &str,
        description: &str,
        introduced_chapter: &str,
        introduced_ref: &str,
        relationship_soil: &str,
        pressure_evidence: &str,
        interest_mechanism: &str,
        payoff_contract: &str,
        payoff_window: &str,
        payoff_path: &str,
        risk_level: &str,
        source_refs: &[String],
    ) -> rusqlite::Result<i64> {
        self.conn.execute(
            "INSERT INTO emotional_debt_ledger (
                project_id, debt_kind, title, description, introduced_at,
                introduced_chapter, introduced_ref, relationship_soil,
                pressure_evidence, interest_mechanism, payoff_contract,
                payoff_status, expected_payoff_window, payoff_path,
                overdue_risk, risk_level, source_refs_json
            ) VALUES (?1, ?2, ?3, ?4, datetime('now'), ?5, ?6, ?7, ?8, ?9, ?10,
                      'open', ?11, ?12, 'medium', ?13, ?14)",
            rusqlite::params![
                project_id,
                debt_kind,
                title,
                description,
                introduced_chapter,
                introduced_ref,
                relationship_soil,
                pressure_evidence,
                interest_mechanism,
                payoff_contract,
                payoff_window,
                payoff_path,
                risk_level,
                serde_json::to_string(source_refs).unwrap_or_default(),
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn get_open_emotional_debts(
        &self,
        project_id: &str,
    ) -> rusqlite::Result<Vec<EmotionalDebtSummary>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, project_id, debt_kind, title, description, introduced_at,
                    introduced_chapter, introduced_ref, relationship_soil,
                    pressure_evidence, interest_mechanism, payoff_contract,
                    payoff_status, expected_payoff_window, payoff_path,
                    overdue_risk, rollover_target, risk_level,
                    related_promise_ids_json, source_refs_json, updated_at
             FROM emotional_debt_ledger
             WHERE project_id = ?1 AND payoff_status = 'open'
             ORDER BY created_at DESC",
        )?;
        let rows = stmt.query_map(rusqlite::params![project_id], |row| {
            Ok(EmotionalDebtSummary {
                id: row.get(0)?,
                project_id: row.get(1)?,
                debt_kind: row.get(2)?,
                title: row.get(3)?,
                description: row.get(4)?,
                introduced_at: row.get(5)?,
                introduced_chapter: row.get(6)?,
                introduced_ref: row.get(7)?,
                relationship_soil: row.get(8)?,
                pressure_evidence: row.get(9)?,
                interest_mechanism: row.get(10)?,
                payoff_contract: row.get(11)?,
                payoff_status: row.get(12)?,
                expected_payoff_window: row.get(13)?,
                payoff_path: row.get(14)?,
                overdue_risk: row.get(15)?,
                rollover_target: row.get(16)?,
                risk_level: row.get(17)?,
                related_promise_ids: row
                    .get::<_, String>(18)
                    .ok()
                    .and_then(|j| serde_json::from_str(&j).ok())
                    .unwrap_or_default(),
                source_refs: row
                    .get::<_, String>(19)
                    .ok()
                    .and_then(|j| serde_json::from_str(&j).ok())
                    .unwrap_or_default(),
                updated_at: row.get(20)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
    }

    pub fn list_emotional_debts(
        &self,
        project_id: &str,
        limit: usize,
    ) -> rusqlite::Result<Vec<EmotionalDebtSummary>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, project_id, debt_kind, title, description, introduced_at,
                    introduced_chapter, introduced_ref, relationship_soil,
                    pressure_evidence, interest_mechanism, payoff_contract,
                    payoff_status, expected_payoff_window, payoff_path,
                    overdue_risk, rollover_target, risk_level,
                    related_promise_ids_json, source_refs_json, updated_at
             FROM emotional_debt_ledger
             WHERE project_id = ?1
             ORDER BY created_at DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(rusqlite::params![project_id, limit as i64], |row| {
            Ok(EmotionalDebtSummary {
                id: row.get(0)?,
                project_id: row.get(1)?,
                debt_kind: row.get(2)?,
                title: row.get(3)?,
                description: row.get(4)?,
                introduced_at: row.get(5)?,
                introduced_chapter: row.get(6)?,
                introduced_ref: row.get(7)?,
                relationship_soil: row.get(8)?,
                pressure_evidence: row.get(9)?,
                interest_mechanism: row.get(10)?,
                payoff_contract: row.get(11)?,
                payoff_status: row.get(12)?,
                expected_payoff_window: row.get(13)?,
                payoff_path: row.get(14)?,
                overdue_risk: row.get(15)?,
                rollover_target: row.get(16)?,
                risk_level: row.get(17)?,
                related_promise_ids: row
                    .get::<_, String>(18)
                    .ok()
                    .and_then(|j| serde_json::from_str(&j).ok())
                    .unwrap_or_default(),
                source_refs: row
                    .get::<_, String>(19)
                    .ok()
                    .and_then(|j| serde_json::from_str(&j).ok())
                    .unwrap_or_default(),
                updated_at: row.get(20)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
    }

    pub fn record_emotional_payoff(
        &self,
        debt_id: i64,
        payoff_evidence: &str,
        source_ref: &str,
    ) -> rusqlite::Result<bool> {
        self.conn.execute(
            "UPDATE emotional_debt_ledger SET
                payoff_status = 'paid',
                pressure_evidence = pressure_evidence || ?2,
                source_refs_json = json_insert(source_refs_json, '$[#]', ?3),
                updated_at = datetime('now')
             WHERE id = ?1",
            rusqlite::params![debt_id, payoff_evidence, source_ref],
        )?;
        Ok(self.conn.changes() > 0)
    }
}
