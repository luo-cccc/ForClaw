impl WriterMemory {
    pub fn upsert_identity_layer(
        &self, character_id: i64, public_identity: &str, private_identity: &str,
        revealed_to: &[String], valid_from_chapter: &str,
    ) -> rusqlite::Result<i64> {
        let revealed_json = serde_json::to_string(revealed_to).unwrap_or_default();
        self.conn.execute(
            "INSERT INTO identity_layers (character_id, public_identity, private_identity, revealed_to_json, valid_from_chapter)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![character_id, public_identity, private_identity, revealed_json, valid_from_chapter],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn get_active_identity(&self, character_id: i64, chapter_title: &str) -> rusqlite::Result<Option<IdentityLayerSummary>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, character_id, public_identity, private_identity, revealed_to_json, valid_from_chapter, valid_to_chapter
             FROM identity_layers WHERE character_id = ?1
               AND valid_from_chapter <= ?2 AND (valid_to_chapter = '' OR valid_to_chapter >= ?2)
             ORDER BY valid_from_chapter DESC LIMIT 1"
        )?;
        let mut rows = stmt.query_map(rusqlite::params![character_id, chapter_title], |row| {
            Ok(IdentityLayerSummary {
                id: row.get(0)?, character_id: row.get(1)?, public_identity: row.get(2)?,
                private_identity: row.get(3)?,
                revealed_to: serde_json::from_str(&row.get::<_, String>(4)?).unwrap_or_default(),
                valid_from_chapter: row.get(5)?, valid_to_chapter: row.get(6)?,
            })
        })?;
        Ok(rows.next().and_then(|r| r.ok()))
    }

    pub fn close_identity_layer(&self, layer_id: i64, valid_to_chapter: &str) -> rusqlite::Result<()> {
        self.conn.execute(
            "UPDATE identity_layers SET valid_to_chapter = ?1 WHERE id = ?2",
            rusqlite::params![valid_to_chapter, layer_id],
        )?;
        Ok(())
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IdentityLayerSummary {
    pub id: i64, pub character_id: i64, pub public_identity: String,
    pub private_identity: String, pub revealed_to: Vec<String>,
    pub valid_from_chapter: String, pub valid_to_chapter: String,
}
