impl WriterMemory {
    pub fn upsert_character_state(
        &self,
        character_id: i64,
        valid_from_chapter: &str,
        core_commitments: &serde_json::Value,
        goal_state: &serde_json::Value,
        identity_state: &serde_json::Value,
        relationship_refs: &[i64],
        source_ref: &str,
    ) -> rusqlite::Result<i64> {
        let commitments_json = serde_json::to_string(core_commitments).unwrap_or_default();
        let goal_json = serde_json::to_string(goal_state).unwrap_or_default();
        let identity_json = serde_json::to_string(identity_state).unwrap_or_default();
        let rel_refs_json = serde_json::to_string(relationship_refs).unwrap_or_default();
        let now_ms = crate::agent_runtime::now_ms();
        self.conn.execute(
            "INSERT INTO character_state_versions
             (character_id, valid_from_chapter, core_commitments_json, goal_state_json,
              identity_state_json, relationship_refs_json, source_ref, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            rusqlite::params![
                character_id, valid_from_chapter, commitments_json, goal_json,
                identity_json, rel_refs_json, source_ref, now_ms
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn get_active_state(
        &self,
        character_id: i64,
        chapter_title: &str,
    ) -> rusqlite::Result<Option<CharacterStateVersion>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, character_id, valid_from_chapter, valid_to_chapter,
                    core_commitments_json, goal_state_json, identity_state_json,
                    relationship_refs_json, source_ref, created_at
             FROM character_state_versions
             WHERE character_id = ?1
               AND valid_from_chapter <= ?2
               AND (valid_to_chapter = '' OR valid_to_chapter >= ?2)
             ORDER BY created_at DESC
             LIMIT 1"
        )?;
        let mut rows = stmt.query_map(rusqlite::params![character_id, chapter_title], |row| {
            Ok(CharacterStateVersion {
                id: row.get(0)?,
                character_id: row.get(1)?,
                valid_from_chapter: row.get(2)?,
                valid_to_chapter: row.get(3)?,
                core_commitments: serde_json::from_str(&row.get::<_, String>(4)?).unwrap_or_default(),
                goal_state: serde_json::from_str(&row.get::<_, String>(5)?).unwrap_or_default(),
                identity_state: serde_json::from_str(&row.get::<_, String>(6)?).unwrap_or_default(),
                relationship_refs: serde_json::from_str(&row.get::<_, String>(7)?).unwrap_or_default(),
                source_ref: row.get(8)?,
                created_at: row.get(9)?,
            })
        })?;
        match rows.next() {
            Some(Ok(v)) => Ok(Some(v)),
            _ => Ok(None),
        }
    }

    pub fn close_state_version(&self, version_id: i64, valid_to_chapter: &str) -> rusqlite::Result<()> {
        self.conn.execute(
            "UPDATE character_state_versions SET valid_to_chapter = ?1 WHERE id = ?2",
            rusqlite::params![valid_to_chapter, version_id],
        )?;
        Ok(())
    }

    pub fn close_active_states_for_character(
        &self,
        character_id: i64,
        valid_to_chapter: &str,
    ) -> rusqlite::Result<usize> {
        let count = self.conn.execute(
            "UPDATE character_state_versions
             SET valid_to_chapter = ?1
             WHERE character_id = ?2 AND valid_to_chapter = ''",
            rusqlite::params![valid_to_chapter, character_id],
        )?;
        Ok(count)
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CharacterStateVersion {
    pub id: i64,
    pub character_id: i64,
    pub valid_from_chapter: String,
    pub valid_to_chapter: String,
    pub core_commitments: serde_json::Value,
    pub goal_state: serde_json::Value,
    pub identity_state: serde_json::Value,
    pub relationship_refs: Vec<i64>,
    pub source_ref: String,
    pub created_at: i64,
}
