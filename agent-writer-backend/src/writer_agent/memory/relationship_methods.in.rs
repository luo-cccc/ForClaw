impl WriterMemory {
    pub fn upsert_relationship(
        &self,
        character_a_id: i64,
        character_b_id: i64,
        relation_type: &str,
        visibility: &str,
        valid_from_chapter: &str,
        source_ref: &str,
    ) -> rusqlite::Result<i64> {
        self.conn.execute(
            "INSERT INTO character_relationships
             (character_a_id, character_b_id, relation_type, visibility,
              valid_from_chapter, source_ref)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                character_a_id, character_b_id, relation_type, visibility,
                valid_from_chapter, source_ref
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn get_active_relationships(
        &self,
        character_id: i64,
        chapter_title: &str,
    ) -> rusqlite::Result<Vec<RelationshipSummary>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, character_a_id, character_b_id, relation_type, visibility,
                    valid_from_chapter, valid_to_chapter, source_ref
             FROM character_relationships
             WHERE (character_a_id = ?1 OR character_b_id = ?1)
               AND valid_from_chapter <= ?2
               AND (valid_to_chapter = '' OR valid_to_chapter >= ?2)
             ORDER BY valid_from_chapter DESC"
        )?;
        let rows = stmt.query_map(rusqlite::params![character_id, chapter_title], |row| {
            Ok(RelationshipSummary {
                id: row.get(0)?,
                character_a_id: row.get(1)?,
                character_b_id: row.get(2)?,
                relation_type: row.get(3)?,
                visibility: row.get(4)?,
                valid_from_chapter: row.get(5)?,
                valid_to_chapter: row.get(6)?,
                source_ref: row.get(7)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
    }

    pub fn close_relationship(&self, rel_id: i64, valid_to_chapter: &str) -> rusqlite::Result<()> {
        self.conn.execute(
            "UPDATE character_relationships SET valid_to_chapter = ?1 WHERE id = ?2",
            rusqlite::params![valid_to_chapter, rel_id],
        )?;
        Ok(())
    }

    pub fn close_active_relationships_for_character(
        &self,
        character_id: i64,
        valid_to_chapter: &str,
    ) -> rusqlite::Result<usize> {
        let count = self.conn.execute(
            "UPDATE character_relationships
             SET valid_to_chapter = ?1
             WHERE (character_a_id = ?2 OR character_b_id = ?2)
               AND valid_to_chapter = ''",
            rusqlite::params![valid_to_chapter, character_id],
        )?;
        Ok(count)
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RelationshipSummary {
    pub id: i64,
    pub character_a_id: i64,
    pub character_b_id: i64,
    pub relation_type: String,
    pub visibility: String,
    pub valid_from_chapter: String,
    pub valid_to_chapter: String,
    pub source_ref: String,
}
