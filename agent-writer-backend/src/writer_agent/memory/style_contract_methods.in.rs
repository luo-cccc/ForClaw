impl WriterMemory {
    pub fn upsert_style_preference(
        &self,
        key: &str,
        value: &str,
        accepted: bool,
    ) -> rusqlite::Result<()> {
        let existing_value = self
            .conn
            .query_row(
                "SELECT value FROM style_preferences WHERE key=?1",
                rusqlite::params![key],
                |row| row.get::<_, String>(0),
            )
            .ok();
        let merged_value = if accepted {
            existing_value
                .as_deref()
                .map(|existing| merge_style_preference_value(existing, value))
                .unwrap_or_else(|| value.trim().to_string())
        } else {
            existing_value.unwrap_or_else(|| value.trim().to_string())
        };

        self.conn.execute(
            "INSERT INTO style_preferences (key, value, accepted_count, rejected_count, updated_at)
             VALUES (?1, ?2, ?3, ?4, datetime('now'))
             ON CONFLICT(key) DO UPDATE SET
             value=excluded.value,
             accepted_count = accepted_count + ?3,
             rejected_count = rejected_count + ?4,
             updated_at = datetime('now')",
            rusqlite::params![
                key,
                merged_value,
                if accepted { 1 } else { 0 },
                if accepted { 0 } else { 1 }
            ],
        )?;
        Ok(())
    }

    pub fn list_style_preferences(
        &self,
        limit: usize,
    ) -> rusqlite::Result<Vec<StylePreferenceSummary>> {
        let mut stmt = self.conn.prepare(
            "SELECT key, value, accepted_count, rejected_count
             FROM style_preferences
             ORDER BY accepted_count DESC, updated_at DESC
             LIMIT ?1",
        )?;
        let rows = stmt.query_map(rusqlite::params![limit as i64], |row| {
            Ok(StylePreferenceSummary {
                key: row.get(0)?,
                value: row.get(1)?,
                accepted_count: row.get(2)?,
                rejected_count: row.get(3)?,
            })
        })?;
        rows.collect()
    }

    // -- Story Contract --

    pub fn upsert_story_contract(&self, contract: &StoryContractSummary) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT INTO story_contracts
             (project_id, title, genre, target_reader, reader_promise, first_30_chapter_promise,
              main_conflict, structural_boundary, tone_contract, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, datetime('now'))
             ON CONFLICT(project_id) DO UPDATE SET
                title=excluded.title,
                genre=excluded.genre,
                target_reader=excluded.target_reader,
                reader_promise=excluded.reader_promise,
                first_30_chapter_promise=excluded.first_30_chapter_promise,
                main_conflict=excluded.main_conflict,
                structural_boundary=excluded.structural_boundary,
                tone_contract=excluded.tone_contract,
                updated_at=datetime('now')",
            rusqlite::params![
                contract.project_id,
                contract.title,
                contract.genre,
                contract.target_reader,
                contract.reader_promise,
                contract.first_30_chapter_promise,
                contract.main_conflict,
                contract.structural_boundary,
                contract.tone_contract,
            ],
        )?;
        Ok(())
    }

    pub fn get_story_contract(
        &self,
        project_id: &str,
    ) -> rusqlite::Result<Option<StoryContractSummary>> {
        self.conn
            .query_row(
                "SELECT project_id, title, genre, target_reader, reader_promise,
                        first_30_chapter_promise, main_conflict, structural_boundary,
                        tone_contract, updated_at
                 FROM story_contracts WHERE project_id=?1",
                rusqlite::params![project_id],
                |row| {
                    let mut summary = StoryContractSummary {
                        project_id: row.get(0)?,
                        title: row.get(1)?,
                        genre: row.get(2)?,
                        target_reader: row.get(3)?,
                        reader_promise: row.get(4)?,
                        first_30_chapter_promise: row.get(5)?,
                        main_conflict: row.get(6)?,
                        structural_boundary: row.get(7)?,
                        tone_contract: row.get(8)?,
                        updated_at: row.get(9)?,
                        quality: String::new(),
                        quality_gaps: Vec::new(),
                    };
                    summary.fill_quality();
                    Ok(summary)
                },
            )
            .optional()
    }

    pub fn ensure_story_contract_seed(
        &self,
        project_id: &str,
        title: &str,
        genre: &str,
        reader_promise: &str,
        main_conflict: &str,
        structural_boundary: &str,
    ) -> rusqlite::Result<bool> {
        if self.get_story_contract(project_id)?.is_some() {
            return Ok(false);
        }
        let mut contract = StoryContractSummary {
            project_id: project_id.to_string(),
            title: title.to_string(),
            genre: genre.to_string(),
            target_reader: String::new(),
            reader_promise: reader_promise.to_string(),
            first_30_chapter_promise: String::new(),
            main_conflict: main_conflict.to_string(),
            structural_boundary: structural_boundary.to_string(),
            tone_contract: String::new(),
            updated_at: String::new(),
            quality: String::new(),
            quality_gaps: Vec::new(),
        };
        contract.fill_quality();
        self.upsert_story_contract(&contract)?;
        Ok(true)
    }

    // -- Chapter Mission --


}
