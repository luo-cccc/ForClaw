impl WriterMemory {
    pub fn record_reveal_event(
        &self, subject_id: i64, reveal_type: &str, revealed_to: &str, chapter: &str, source_ref: &str,
    ) -> rusqlite::Result<i64> {
        self.conn.execute(
            "INSERT INTO reveal_events (subject_id, reveal_type, revealed_to, chapter, source_ref)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![subject_id, reveal_type, revealed_to, chapter, source_ref],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn list_reveals_by_chapter(&self, chapter_title: &str) -> rusqlite::Result<Vec<RevealEventSummary>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, subject_id, reveal_type, revealed_to, chapter, source_ref
             FROM reveal_events WHERE chapter = ?1 ORDER BY id"
        )?;
        let rows = stmt.query_map(rusqlite::params![chapter_title], |row| {
            Ok(RevealEventSummary {
                id: row.get(0)?, subject_id: row.get(1)?, reveal_type: row.get(2)?,
                revealed_to: row.get(3)?, chapter: row.get(4)?, source_ref: row.get(5)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
    }

    pub fn list_reveals_by_subject(&self, subject_id: i64) -> rusqlite::Result<Vec<RevealEventSummary>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, subject_id, reveal_type, revealed_to, chapter, source_ref
             FROM reveal_events WHERE subject_id = ?1 ORDER BY id"
        )?;
        let rows = stmt.query_map(rusqlite::params![subject_id], |row| {
            Ok(RevealEventSummary {
                id: row.get(0)?, subject_id: row.get(1)?, reveal_type: row.get(2)?,
                revealed_to: row.get(3)?, chapter: row.get(4)?, source_ref: row.get(5)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RevealEventSummary {
    pub id: i64, pub subject_id: i64, pub reveal_type: String,
    pub revealed_to: String, pub chapter: String, pub source_ref: String,
}
