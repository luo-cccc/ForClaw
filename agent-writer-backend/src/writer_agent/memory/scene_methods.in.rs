impl WriterMemory {
    pub fn upsert_scene(&self, chapter_title: &str, sequence: i32, scene_type: &str, summary: &str) -> rusqlite::Result<i64> {
        self.conn.execute(
            "INSERT INTO scenes (chapter_title, sequence, scene_type, summary) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![chapter_title, sequence, scene_type, summary],
        )?;
        Ok(self.conn.last_insert_rowid())
    }
    pub fn list_scenes_by_chapter(&self, chapter_title: &str) -> rusqlite::Result<Vec<SceneSummary>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, chapter_title, sequence, scene_type, summary FROM scenes WHERE chapter_title = ?1 ORDER BY sequence"
        )?;
        let rows = stmt.query_map(rusqlite::params![chapter_title], |row| {
            Ok(SceneSummary { id: row.get(0)?, chapter_title: row.get(1)?, sequence: row.get(2)?, scene_type: row.get(3)?, summary: row.get(4)? })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
    }
    pub fn reorder_scenes(&self, chapter_title: &str, ordered_ids: &[i64]) -> rusqlite::Result<()> {
        for (i, id) in ordered_ids.iter().enumerate() {
            self.conn.execute("UPDATE scenes SET sequence = ?1 WHERE id = ?2 AND chapter_title = ?3", rusqlite::params![i as i32, id, chapter_title])?;
        }
        Ok(())
    }
}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SceneSummary { pub id: i64, pub chapter_title: String, pub sequence: i32, pub scene_type: String, pub summary: String }
