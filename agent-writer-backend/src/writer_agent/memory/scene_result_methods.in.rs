impl WriterMemory {
    pub fn record_scene_result(&self, scene_id: i64, outcome: &str, consequence: &str, source_ref: &str) -> rusqlite::Result<i64> {
        self.conn.execute("INSERT INTO scene_results (scene_id, outcome, consequence, source_ref) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![scene_id, outcome, consequence, source_ref])?;
        Ok(self.conn.last_insert_rowid())
    }
    pub fn get_scene_results(&self, scene_id: i64) -> rusqlite::Result<Vec<SceneResultSummary>> {
        let mut stmt = self.conn.prepare("SELECT id, scene_id, outcome, consequence, source_ref FROM scene_results WHERE scene_id = ?1 ORDER BY id")?;
        let rows = stmt.query_map(rusqlite::params![scene_id], |row| {
            Ok(SceneResultSummary { id: row.get(0)?, scene_id: row.get(1)?, outcome: row.get(2)?, consequence: row.get(3)?, source_ref: row.get(4)? })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
    }
}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SceneResultSummary { pub id: i64, pub scene_id: i64, pub outcome: String, pub consequence: String, pub source_ref: String }
