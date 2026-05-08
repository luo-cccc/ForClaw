impl WriterMemory {
    pub fn upsert_chapter_time_mapping(&self, chapter_title: &str, scene_id: Option<i64>, time_slice_id: i64, narrative_mode: &str) -> rusqlite::Result<i64> {
        self.conn.execute(
            "INSERT INTO chapter_time_mapping (chapter_title, scene_id, time_slice_id, narrative_mode) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![chapter_title, scene_id, time_slice_id, narrative_mode],
        )?;
        Ok(self.conn.last_insert_rowid())
    }
    pub fn get_time_mapping_for_chapter(&self, chapter_title: &str) -> rusqlite::Result<Vec<ChapterTimeMappingSummary>> {
        let mut stmt = self.conn.prepare("SELECT id, chapter_title, scene_id, time_slice_id, narrative_mode FROM chapter_time_mapping WHERE chapter_title = ?1 ORDER BY id")?;
        let rows = stmt.query_map(rusqlite::params![chapter_title], |row| {
            Ok(ChapterTimeMappingSummary { id: row.get(0)?, chapter_title: row.get(1)?, scene_id: row.get(2)?, time_slice_id: row.get(3)?, narrative_mode: row.get(4)? })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
    }
}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChapterTimeMappingSummary { pub id: i64, pub chapter_title: String, pub scene_id: Option<i64>, pub time_slice_id: i64, pub narrative_mode: String }
