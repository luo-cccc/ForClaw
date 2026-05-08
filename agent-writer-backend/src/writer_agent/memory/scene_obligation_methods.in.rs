impl WriterMemory {
    pub fn upsert_scene_obligations(&self, scene_id: i64, promise_ids: &[i64], mission_refs: &[String], payoff_targets: &[String]) -> rusqlite::Result<i64> {
        let pj = serde_json::to_string(promise_ids).unwrap_or_default();
        let mj = serde_json::to_string(mission_refs).unwrap_or_default();
        let tj = serde_json::to_string(payoff_targets).unwrap_or_default();
        self.conn.execute("INSERT INTO scene_obligations (scene_id, promise_ids_json, mission_refs_json, payoff_targets_json) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![scene_id, pj, mj, tj])?;
        Ok(self.conn.last_insert_rowid())
    }
    pub fn get_scene_obligations(&self, scene_id: i64) -> rusqlite::Result<Option<SceneObligationSummary>> {
        let mut stmt = self.conn.prepare("SELECT id, scene_id, promise_ids_json, mission_refs_json, payoff_targets_json FROM scene_obligations WHERE scene_id = ?1")?;
        let mut rows = stmt.query_map(rusqlite::params![scene_id], |row| {
            Ok(SceneObligationSummary { id: row.get(0)?, scene_id: row.get(1)?,
                promise_ids: serde_json::from_str(&row.get::<_,String>(2)?).unwrap_or_default(),
                mission_refs: serde_json::from_str(&row.get::<_,String>(3)?).unwrap_or_default(),
                payoff_targets: serde_json::from_str(&row.get::<_,String>(4)?).unwrap_or_default() })
        })?;
        Ok(rows.next().and_then(|r| r.ok()))
    }
}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SceneObligationSummary { pub id: i64, pub scene_id: i64, pub promise_ids: Vec<i64>, pub mission_refs: Vec<String>, pub payoff_targets: Vec<String> }
