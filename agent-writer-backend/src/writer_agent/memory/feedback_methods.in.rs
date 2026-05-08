impl WriterMemory {
    pub fn record_decision(
        &self,
        scope: &str,
        title: &str,
        decision: &str,
        alternatives: &[String],
        rationale: &str,
        sources: &[String],
    ) -> rusqlite::Result<i64> {
        self.conn.execute(
            "INSERT INTO creative_decisions (scope, title, decision, alternatives_json, rationale, source_refs_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![scope, title, decision,
                serde_json::to_string(alternatives).unwrap(),
                rationale,
                serde_json::to_string(sources).unwrap()],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn list_recent_decisions(
        &self,
        limit: usize,
    ) -> rusqlite::Result<Vec<CreativeDecisionSummary>> {
        let mut stmt = self.conn.prepare(
            "SELECT scope, title, decision, rationale, created_at
             FROM creative_decisions ORDER BY id DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(rusqlite::params![limit as i64], |row| {
            Ok(CreativeDecisionSummary {
                scope: row.get(0)?,
                title: row.get(1)?,
                decision: row.get(2)?,
                rationale: row.get(3)?,
                created_at: row.get(4)?,
            })
        })?;
        rows.collect()
    }

    pub fn record_manual_agent_turn(&self, turn: &ManualAgentTurnSummary) -> rusqlite::Result<i64> {
        self.conn.execute(
            "INSERT INTO manual_agent_turns
             (project_id, observation_id, chapter_title, user_text, assistant_text, source_refs_json, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![
                turn.project_id,
                turn.observation_id,
                turn.chapter_title.clone().unwrap_or_default(),
                turn.user,
                turn.assistant,
                serde_json::to_string(&turn.source_refs).unwrap_or_else(|_| "[]".to_string()),
                turn.created_at as i64,
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn list_manual_agent_turns(
        &self,
        project_id: &str,
        limit: usize,
    ) -> rusqlite::Result<Vec<ManualAgentTurnSummary>> {
        let mut stmt = self.conn.prepare(
            "SELECT project_id, observation_id, chapter_title, user_text, assistant_text, source_refs_json, created_at
             FROM manual_agent_turns
             WHERE project_id = ?1
             ORDER BY created_at DESC, id DESC
             LIMIT ?2",
        )?;
        let rows = stmt.query_map(rusqlite::params![project_id, limit as i64], |row| {
            let chapter_title: String = row.get(2)?;
            let source_refs_json: String = row.get(5)?;
            let source_refs =
                serde_json::from_str::<Vec<String>>(&source_refs_json).unwrap_or_default();
            let created_at: i64 = row.get(6)?;
            Ok(ManualAgentTurnSummary {
                project_id: row.get(0)?,
                observation_id: row.get(1)?,
                chapter_title: if chapter_title.trim().is_empty() {
                    None
                } else {
                    Some(chapter_title)
                },
                user: row.get(3)?,
                assistant: row.get(4)?,
                source_refs,
                created_at: created_at.max(0) as u64,
            })
        })?;
        let mut turns = rows.collect::<rusqlite::Result<Vec<_>>>()?;
        turns.reverse();
        Ok(turns)
    }

    // -- Feedback --

    pub fn record_feedback(
        &self,
        proposal_id: &str,
        action: &str,
        reason: &str,
        final_text: &str,
    ) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT INTO proposal_feedback (proposal_id, action, reason, final_text) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![proposal_id, action, reason, final_text],
        )?;
        Ok(())
    }

    pub fn record_memory_audit(&self, entry: &MemoryAuditSummary) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT INTO memory_audit_events
             (proposal_id, kind, action, title, evidence, rationale, reason, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            rusqlite::params![
                entry.proposal_id,
                entry.kind,
                entry.action,
                entry.title,
                entry.evidence,
                entry.rationale,
                entry.reason.clone().unwrap_or_default(),
                entry.created_at as i64,
            ],
        )?;
        Ok(())
    }

    pub fn list_memory_audit(&self, limit: usize) -> rusqlite::Result<Vec<MemoryAuditSummary>> {
        let mut stmt = self.conn.prepare(
            "SELECT proposal_id, kind, action, title, evidence, rationale, reason, created_at
             FROM memory_audit_events ORDER BY id DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(rusqlite::params![limit as i64], |row| {
            let reason: String = row.get(6)?;
            let created_at: i64 = row.get(7)?;
            Ok(MemoryAuditSummary {
                proposal_id: row.get(0)?,
                kind: row.get(1)?,
                action: row.get(2)?,
                title: row.get(3)?,
                evidence: row.get(4)?,
                rationale: row.get(5)?,
                reason: if reason.trim().is_empty() {
                    None
                } else {
                    Some(reason)
                },
                created_at: created_at.max(0) as u64,
            })
        })?;
        rows.collect()
    }

    pub fn record_memory_feedback(&self, entry: &MemoryFeedbackSummary) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT INTO memory_feedback_events
             (slot, category, action, confidence_delta, source_error, proposal_id, reason, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            rusqlite::params![
                entry.slot,
                entry.category,
                entry.action,
                entry.confidence_delta,
                entry.source_error.clone().unwrap_or_default(),
                entry.proposal_id,
                entry.reason.clone().unwrap_or_default(),
                entry.created_at as i64,
            ],
        )?;
        Ok(())
    }

    pub fn list_memory_feedback(
        &self,
        limit: usize,
    ) -> rusqlite::Result<Vec<MemoryFeedbackSummary>> {
        let mut stmt = self.conn.prepare(
            "SELECT slot, category, action, confidence_delta, source_error, proposal_id, reason, created_at
             FROM memory_feedback_events ORDER BY id DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(rusqlite::params![limit as i64], memory_feedback_from_row)?;
        rows.collect()
    }

    pub fn list_memory_feedback_for_slot(
        &self,
        slot: &str,
        limit: usize,
    ) -> rusqlite::Result<Vec<MemoryFeedbackSummary>> {
        let mut stmt = self.conn.prepare(
            "SELECT slot, category, action, confidence_delta, source_error, proposal_id, reason, created_at
             FROM memory_feedback_events
             WHERE slot=?1
             ORDER BY id DESC
             LIMIT ?2",
        )?;
        let rows = stmt.query_map(
            rusqlite::params![slot, limit as i64],
            memory_feedback_from_row,
        )?;
        rows.collect()
    }

    // -- Writer Agent Trace --


}
