impl WriterMemory {
    pub fn record_observation_trace(
        &self,
        id: &str,
        created_at: u64,
        reason: &str,
        chapter_title: Option<&str>,
        paragraph_snippet: &str,
    ) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT INTO writer_observation_trace
             (observation_id, created_at, reason, chapter_title, paragraph_snippet)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![
                id,
                created_at as i64,
                reason,
                chapter_title.unwrap_or(""),
                paragraph_snippet,
            ],
        )?;
        Ok(())
    }

    pub fn record_proposal_trace(
        &self,
        proposal: &ProposalTraceSummary,
        created_at: u64,
    ) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT INTO writer_proposal_trace
             (proposal_id, observation_id, kind, priority, state, confidence, preview_snippet, evidence_json, context_budget_json, created_at, expires_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
             ON CONFLICT(proposal_id) DO UPDATE SET
                observation_id=excluded.observation_id,
                kind=excluded.kind,
                priority=excluded.priority,
                state=excluded.state,
                confidence=excluded.confidence,
                preview_snippet=excluded.preview_snippet,
                evidence_json=excluded.evidence_json,
                context_budget_json=excluded.context_budget_json,
                created_at=excluded.created_at,
                expires_at=excluded.expires_at",
            rusqlite::params![
                proposal.id,
                proposal.observation_id,
                proposal.kind,
                proposal.priority,
                proposal.state,
                proposal.confidence,
                proposal.preview_snippet,
                serde_json::to_string(&proposal.evidence).unwrap_or_else(|_| "[]".to_string()),
                proposal
                    .context_budget
                    .as_ref()
                    .and_then(|budget| serde_json::to_string(budget).ok())
                    .unwrap_or_default(),
                created_at as i64,
                proposal.expires_at.map(|value| value as i64),
            ],
        )?;
        Ok(())
    }

    pub fn update_proposal_trace_state(
        &self,
        proposal_id: &str,
        state: &str,
    ) -> rusqlite::Result<()> {
        self.conn.execute(
            "UPDATE writer_proposal_trace SET state=?1 WHERE proposal_id=?2",
            rusqlite::params![state, proposal_id],
        )?;
        Ok(())
    }

    pub fn record_feedback_trace(&self, feedback: &FeedbackTraceSummary) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT INTO writer_feedback_trace (proposal_id, action, reason, created_at)
             VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![
                feedback.proposal_id,
                feedback.action,
                feedback.reason.clone().unwrap_or_default(),
                feedback.created_at as i64,
            ],
        )?;
        Ok(())
    }

    pub fn record_run_event(&self, event: &RunEventSummary) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT INTO writer_run_events
             (seq, project_id, session_id, task_id, event_type, source_refs_json, data_json, ts_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            rusqlite::params![
                event.seq as i64,
                event.project_id,
                event.session_id,
                event.task_id.clone().unwrap_or_default(),
                event.event_type,
                string_vec_json(&event.source_refs),
                serde_json::to_string(&event.data).unwrap_or_else(|_| "{}".to_string()),
                event.ts_ms as i64,
            ],
        )?;
        Ok(())
    }

    pub fn record_context_recalls(
        &self,
        project_id: &str,
        proposal_id: &str,
        observation_id: &str,
        evidence: &[super::proposal::EvidenceRef],
        recalled_at: u64,
    ) -> rusqlite::Result<()> {
        for evidence in evidence
            .iter()
            .filter(|entry| !entry.reference.trim().is_empty() || !entry.snippet.trim().is_empty())
        {
            let source = format!("{:?}", evidence.source);
            let reference = if evidence.reference.trim().is_empty() {
                snippet_for_storage(&evidence.snippet, 80)
            } else {
                evidence.reference.trim().to_string()
            };
            let snippet = snippet_for_storage(&evidence.snippet, 240);
            self.conn.execute(
                "INSERT INTO writer_context_recalls
                 (project_id, source, reference, snippet, recall_count, first_recalled_at, last_recalled_at, last_observation_id, last_proposal_id)
                 VALUES (?1, ?2, ?3, ?4, 1, ?5, ?5, ?6, ?7)
                 ON CONFLICT(project_id, source, reference) DO UPDATE SET
                    snippet=excluded.snippet,
                    recall_count=writer_context_recalls.recall_count + 1,
                    last_recalled_at=excluded.last_recalled_at,
                    last_observation_id=excluded.last_observation_id,
                    last_proposal_id=excluded.last_proposal_id",
                rusqlite::params![
                    project_id,
                    source,
                    reference,
                    snippet,
                    recalled_at as i64,
                    observation_id,
                    proposal_id,
                ],
            )?;
        }
        Ok(())
    }

    pub fn list_context_recalls(
        &self,
        project_id: &str,
        limit: usize,
    ) -> rusqlite::Result<Vec<ContextRecallSummary>> {
        let mut stmt = self.conn.prepare(
            "SELECT source, reference, snippet, recall_count, first_recalled_at, last_recalled_at, last_observation_id, last_proposal_id
             FROM writer_context_recalls
             WHERE project_id=?1
             ORDER BY recall_count DESC, last_recalled_at DESC
             LIMIT ?2",
        )?;
        let rows = stmt.query_map(rusqlite::params![project_id, limit as i64], |row| {
            let recall_count: i64 = row.get(3)?;
            let first_recalled_at: i64 = row.get(4)?;
            let last_recalled_at: i64 = row.get(5)?;
            Ok(ContextRecallSummary {
                source: row.get(0)?,
                reference: row.get(1)?,
                snippet: row.get(2)?,
                recall_count: recall_count.max(0) as u64,
                first_recalled_at: first_recalled_at.max(0) as u64,
                last_recalled_at: last_recalled_at.max(0) as u64,
                last_observation_id: row.get(6)?,
                last_proposal_id: row.get(7)?,
            })
        })?;
        rows.collect()
    }

    pub fn list_observation_traces(
        &self,
        limit: usize,
    ) -> rusqlite::Result<Vec<ObservationTraceSummary>> {
        let mut stmt = self.conn.prepare(
            "SELECT observation_id, created_at, reason, chapter_title, paragraph_snippet
             FROM writer_observation_trace ORDER BY id DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(rusqlite::params![limit as i64], |row| {
            let chapter_title: String = row.get(3)?;
            let created_at: i64 = row.get(1)?;
            Ok(ObservationTraceSummary {
                id: row.get(0)?,
                created_at: created_at.max(0) as u64,
                reason: row.get(2)?,
                chapter_title: if chapter_title.trim().is_empty() {
                    None
                } else {
                    Some(chapter_title)
                },
                paragraph_snippet: row.get(4)?,
            })
        })?;
        rows.collect()
    }

    pub fn list_proposal_traces(
        &self,
        limit: usize,
    ) -> rusqlite::Result<Vec<ProposalTraceSummary>> {
        let mut stmt = self.conn.prepare(
            "SELECT proposal_id, observation_id, kind, priority, state, confidence, preview_snippet, evidence_json, context_budget_json, expires_at
             FROM writer_proposal_trace ORDER BY id DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(rusqlite::params![limit as i64], |row| {
            let evidence_json: String = row.get(7)?;
            let context_budget_json: String = row.get(8)?;
            let context_budget = if context_budget_json.trim().is_empty() {
                None
            } else {
                serde_json::from_str::<ContextBudgetTrace>(&context_budget_json).ok()
            };
            let expires_at: Option<i64> = row.get(9)?;
            Ok(ProposalTraceSummary {
                id: row.get(0)?,
                observation_id: row.get(1)?,
                kind: row.get(2)?,
                priority: row.get(3)?,
                state: row.get(4)?,
                confidence: row.get(5)?,
                preview_snippet: row.get(6)?,
                evidence: serde_json::from_str::<Vec<super::proposal::EvidenceRef>>(&evidence_json)
                    .unwrap_or_default(),
                context_budget,
                expires_at: expires_at.map(|value| value.max(0) as u64),
            })
        })?;
        rows.collect()
    }

    pub fn list_feedback_traces(
        &self,
        limit: usize,
    ) -> rusqlite::Result<Vec<FeedbackTraceSummary>> {
        let mut stmt = self.conn.prepare(
            "SELECT proposal_id, action, reason, created_at
             FROM writer_feedback_trace ORDER BY id DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(rusqlite::params![limit as i64], |row| {
            let reason: String = row.get(2)?;
            let created_at: i64 = row.get(3)?;
            Ok(FeedbackTraceSummary {
                proposal_id: row.get(0)?,
                action: row.get(1)?,
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

    pub fn list_run_events(
        &self,
        project_id: &str,
        session_id: &str,
        limit: usize,
    ) -> rusqlite::Result<Vec<RunEventSummary>> {
        let mut stmt = self.conn.prepare(
            "SELECT seq, project_id, session_id, task_id, event_type, source_refs_json, data_json, ts_ms
             FROM writer_run_events
             WHERE project_id=?1 AND session_id=?2
             ORDER BY seq DESC LIMIT ?3",
        )?;
        let rows = stmt.query_map(
            rusqlite::params![project_id, session_id, limit as i64],
            |row| {
                let seq: i64 = row.get(0)?;
                let task_id: String = row.get(3)?;
                let data_json: String = row.get(6)?;
                let ts_ms: i64 = row.get(7)?;
                Ok(RunEventSummary {
                    seq: seq.max(0) as u64,
                    project_id: row.get(1)?,
                    session_id: row.get(2)?,
                    task_id: if task_id.trim().is_empty() {
                        None
                    } else {
                        Some(task_id)
                    },
                    event_type: row.get(4)?,
                    source_refs: string_vec_from_json(row.get::<_, String>(5)?.as_str()),
                    data: serde_json::from_str(&data_json)
                        .unwrap_or_else(|_| serde_json::json!({})),
                    ts_ms: ts_ms.max(0) as u64,
                })
            },
        )?;
        let mut events = rows.collect::<rusqlite::Result<Vec<_>>>()?;
        events.reverse();
        Ok(events)
    }

    pub fn list_project_run_events(
        &self,
        project_id: &str,
        limit: usize,
    ) -> rusqlite::Result<Vec<RunEventSummary>> {
        let mut stmt = self.conn.prepare(
            "SELECT seq, project_id, session_id, task_id, event_type, source_refs_json, data_json, ts_ms
             FROM writer_run_events
             WHERE project_id=?1
             ORDER BY ts_ms DESC, id DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(rusqlite::params![project_id, limit as i64], |row| {
            let seq: i64 = row.get(0)?;
            let task_id: String = row.get(3)?;
            let data_json: String = row.get(6)?;
            let ts_ms: i64 = row.get(7)?;
            Ok(RunEventSummary {
                seq: seq.max(0) as u64,
                project_id: row.get(1)?,
                session_id: row.get(2)?,
                task_id: if task_id.trim().is_empty() {
                    None
                } else {
                    Some(task_id)
                },
                event_type: row.get(4)?,
                source_refs: string_vec_from_json(row.get::<_, String>(5)?.as_str()),
                data: serde_json::from_str(&data_json).unwrap_or_else(|_| serde_json::json!({})),
                ts_ms: ts_ms.max(0) as u64,
            })
        })?;
        let mut events = rows.collect::<rusqlite::Result<Vec<_>>>()?;
        events.reverse();
        Ok(events)
    }

    #[cfg(test)]
    pub fn feedback_stats(&self, proposal_id: &str) -> rusqlite::Result<(i64, i64)> {
        let mut stmt = self.conn.prepare(
            "SELECT COUNT(*) FILTER(WHERE action='accepted'), COUNT(*) FILTER(WHERE action='rejected')
             FROM proposal_feedback WHERE proposal_id=?1"
        )?;
        let (accepted, rejected) = stmt.query_row(rusqlite::params![proposal_id], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })?;
        Ok((accepted, rejected))
    }
}
