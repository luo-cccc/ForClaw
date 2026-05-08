impl WriterMemory {
    pub fn upsert_reader_compensation_profile(
        &self,
        project_id: &str,
        profile: &ReaderCompensationProfile,
    ) -> rusqlite::Result<bool> {
        self.conn.execute(
            "INSERT INTO reader_compensation_profiles (
                project_id, target_reader, primary_lack, secondary_lacks_json,
                protagonist_proxy_state, dominant_relationship_soil, pressure_mode,
                payoff_mode, payoff_path, escalation_ladder, forbidden_shortcuts_json,
                confidence, source_refs_json, pending_approval, approved_by, approved_at,
                created_at, updated_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)
            ON CONFLICT(project_id) DO UPDATE SET
                target_reader = excluded.target_reader,
                primary_lack = excluded.primary_lack,
                secondary_lacks_json = excluded.secondary_lacks_json,
                protagonist_proxy_state = excluded.protagonist_proxy_state,
                dominant_relationship_soil = excluded.dominant_relationship_soil,
                pressure_mode = excluded.pressure_mode,
                payoff_mode = excluded.payoff_mode,
                payoff_path = excluded.payoff_path,
                escalation_ladder = excluded.escalation_ladder,
                forbidden_shortcuts_json = excluded.forbidden_shortcuts_json,
                confidence = excluded.confidence,
                source_refs_json = excluded.source_refs_json,
                pending_approval = excluded.pending_approval,
                approved_by = excluded.approved_by,
                approved_at = excluded.approved_at,
                updated_at = excluded.updated_at",
            rusqlite::params![
                project_id,
                profile.target_reader,
                profile.primary_lack,
                serde_json::to_string(&profile.secondary_lacks).unwrap_or_default(),
                profile.protagonist_proxy_state,
                profile.dominant_relationship_soil,
                profile.pressure_mode,
                profile.payoff_mode,
                profile.payoff_path,
                profile.escalation_ladder,
                serde_json::to_string(&profile.forbidden_shortcuts).unwrap_or_default(),
                profile.confidence,
                serde_json::to_string(&profile.source_refs).unwrap_or_default(),
                profile.pending_approval,
                profile.approved_by,
                profile.approved_at,
                profile.created_at,
                profile.updated_at,
            ],
        )?;
        Ok(self.conn.changes() > 0)
    }

    pub fn get_reader_compensation_profile(
        &self,
        project_id: &str,
    ) -> rusqlite::Result<Option<ReaderCompensationProfile>> {
        self.conn
            .query_row(
                "SELECT target_reader, primary_lack, secondary_lacks_json,
                        protagonist_proxy_state, dominant_relationship_soil, pressure_mode,
                        payoff_mode, payoff_path, escalation_ladder, forbidden_shortcuts_json,
                        confidence, source_refs_json, pending_approval, approved_by, approved_at,
                        created_at, updated_at
                 FROM reader_compensation_profiles WHERE project_id = ?1",
                rusqlite::params![project_id],
                |row| {
                    Ok(ReaderCompensationProfile {
                        target_reader: row.get(0)?,
                        primary_lack: row.get(1)?,
                        secondary_lacks: row
                            .get::<_, String>(2)
                            .ok()
                            .and_then(|j| serde_json::from_str(&j).ok())
                            .unwrap_or_default(),
                        protagonist_proxy_state: row.get(3)?,
                        dominant_relationship_soil: row.get(4)?,
                        pressure_mode: row.get(5)?,
                        payoff_mode: row.get(6)?,
                        payoff_path: row.get(7)?,
                        escalation_ladder: row.get(8)?,
                        forbidden_shortcuts: row
                            .get::<_, String>(9)
                            .ok()
                            .and_then(|j| serde_json::from_str(&j).ok())
                            .unwrap_or_default(),
                        confidence: row.get(10)?,
                        source_refs: row
                            .get::<_, String>(11)
                            .ok()
                            .and_then(|j| serde_json::from_str(&j).ok())
                            .unwrap_or_default(),
                        pending_approval: row.get(12)?,
                        approved_by: row.get(13)?,
                        approved_at: row.get(14)?,
                        created_at: row.get(15)?,
                        updated_at: row.get(16)?,
                    })
                },
            )
            .optional()
    }

    pub fn approve_reader_compensation_profile(
        &self,
        project_id: &str,
        approved_by: &str,
    ) -> rusqlite::Result<bool> {
        self.conn.execute(
            "UPDATE reader_compensation_profiles SET pending_approval = 0, approved_by = ?2, approved_at = datetime('now'), updated_at = datetime('now') WHERE project_id = ?1",
            rusqlite::params![project_id, approved_by],
        )?;
        Ok(self.conn.changes() > 0)
    }
}
