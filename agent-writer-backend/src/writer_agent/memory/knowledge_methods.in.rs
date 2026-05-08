impl WriterMemory {
    pub fn upsert_knowledge_item(&self, topic: &str, truth_state: &str, source_ref: &str) -> rusqlite::Result<i64> {
        self.conn.execute(
            "INSERT OR REPLACE INTO knowledge_items (topic, truth_state, source_ref)
             VALUES (?1, ?2, ?3)",
            rusqlite::params![topic, truth_state, source_ref],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn get_knowledge_item(&self, topic: &str) -> rusqlite::Result<Option<KnowledgeItemSummary>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, topic, truth_state, source_ref FROM knowledge_items WHERE topic = ?1"
        )?;
        let mut rows = stmt.query_map(rusqlite::params![topic], |row| {
            Ok(KnowledgeItemSummary { id: row.get(0)?, topic: row.get(1)?, truth_state: row.get(2)?, source_ref: row.get(3)? })
        })?;
        Ok(rows.next().and_then(|r| r.ok()))
    }

    pub fn list_knowledge_items(&self, truth_state_filter: Option<&str>) -> rusqlite::Result<Vec<KnowledgeItemSummary>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, topic, truth_state, source_ref FROM knowledge_items ORDER BY topic"
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(KnowledgeItemSummary { id: row.get(0)?, topic: row.get(1)?, truth_state: row.get(2)?, source_ref: row.get(3)? })
        })?.collect::<rusqlite::Result<Vec<_>>>()?;
        if let Some(ts) = truth_state_filter {
            Ok(rows.into_iter().filter(|ki| ki.truth_state == ts).collect())
        } else {
            Ok(rows)
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeItemSummary {
    pub id: i64,
    pub topic: String,
    pub truth_state: String,
    pub source_ref: String,
}
