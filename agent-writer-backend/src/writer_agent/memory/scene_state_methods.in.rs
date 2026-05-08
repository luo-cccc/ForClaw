impl WriterMemory {
    pub fn upsert_scene_state(&self, scene_id: i64, objective: &str, participants: &[String], location_ref: &str, entry_state: &serde_json::Value, exit_state: &serde_json::Value) -> rusqlite::Result<i64> {
        let p = serde_json::to_string(participants).unwrap_or_default();
        let e = serde_json::to_string(entry_state).unwrap_or_default();
        let x = serde_json::to_string(exit_state).unwrap_or_default();
        self.conn.execute("INSERT INTO scene_state (scene_id, objective, participants_json, location_ref, entry_state_json, exit_state_json) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![scene_id, objective, p, location_ref, e, x])?;
        Ok(self.conn.last_insert_rowid())
    }
    pub fn get_scene_state(&self, scene_id: i64) -> rusqlite::Result<Option<SceneStateSummary>> {
        let mut stmt = self.conn.prepare("SELECT id, scene_id, objective, participants_json, location_ref, entry_state_json, exit_state_json FROM scene_state WHERE scene_id = ?1")?;
        let mut rows = stmt.query_map(rusqlite::params![scene_id], |row| {
            Ok(SceneStateSummary { id: row.get(0)?, scene_id: row.get(1)?, objective: row.get(2)?,
                participants: serde_json::from_str(&row.get::<_,String>(3)?).unwrap_or_default(),
                location_ref: row.get(4)?, entry_state: serde_json::from_str(&row.get::<_,String>(5)?).unwrap_or_default(),
                exit_state: serde_json::from_str(&row.get::<_,String>(6)?).unwrap_or_default() })
        })?;
        Ok(rows.next().and_then(|r| r.ok()))
    }
}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SceneStateSummary { pub id: i64, pub scene_id: i64, pub objective: String, pub participants: Vec<String>, pub location_ref: String, pub entry_state: serde_json::Value, pub exit_state: serde_json::Value }
