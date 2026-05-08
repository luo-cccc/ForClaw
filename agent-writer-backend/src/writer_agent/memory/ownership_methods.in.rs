impl WriterMemory {
    pub fn upsert_knowledge_ownership(
        &self, knowledge_id: i64, holder_type: &str, holder_id: i64,
        knowledge_mode: &str, valid_from_chapter: &str, source_ref: &str,
    ) -> rusqlite::Result<i64> {
        self.conn.execute(
            "INSERT INTO knowledge_ownership (knowledge_id, holder_type, holder_id, knowledge_mode, valid_from_chapter, source_ref)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![knowledge_id, holder_type, holder_id, knowledge_mode, valid_from_chapter, source_ref],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn get_knowledge_by_holder(&self, holder_type: &str, holder_id: i64, chapter_title: &str) -> rusqlite::Result<Vec<KnowledgeOwnershipSummary>> {
        let mut stmt = self.conn.prepare(
            "SELECT ko.id, ki.topic, ko.knowledge_mode, ko.valid_from_chapter, ko.valid_to_chapter
             FROM knowledge_ownership ko JOIN knowledge_items ki ON ko.knowledge_id = ki.id
             WHERE ko.holder_type = ?1 AND ko.holder_id = ?2
               AND ko.valid_from_chapter <= ?3 AND (ko.valid_to_chapter = '' OR ko.valid_to_chapter >= ?3)
             ORDER BY ko.valid_from_chapter DESC"
        )?;
        let rows = stmt.query_map(rusqlite::params![holder_type, holder_id, chapter_title], |row| {
            Ok(KnowledgeOwnershipSummary {
                id: row.get(0)?, topic: row.get(1)?, knowledge_mode: row.get(2)?,
                valid_from_chapter: row.get(3)?, valid_to_chapter: row.get(4)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
    }

    pub fn close_ownership(&self, ownership_id: i64, valid_to_chapter: &str) -> rusqlite::Result<()> {
        self.conn.execute(
            "UPDATE knowledge_ownership SET valid_to_chapter = ?1 WHERE id = ?2",
            rusqlite::params![valid_to_chapter, ownership_id],
        )?;
        Ok(())
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeOwnershipSummary {
    pub id: i64, pub topic: String, pub knowledge_mode: String,
    pub valid_from_chapter: String, pub valid_to_chapter: String,
}
