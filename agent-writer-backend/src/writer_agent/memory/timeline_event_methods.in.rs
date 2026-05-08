impl WriterMemory {
    pub fn record_timeline_event(&self, subject_ids: &[i64], event_type: &str, time_slice_id: i64, source_ref: &str) -> rusqlite::Result<i64> {
        let ids_json = serde_json::to_string(subject_ids).unwrap_or_default();
        self.conn.execute("INSERT INTO timeline_events (subject_ids_json, event_type, time_slice_id, source_ref) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![ids_json, event_type, time_slice_id, source_ref])?;
        Ok(self.conn.last_insert_rowid())
    }
    pub fn list_timeline_events_by_slice(&self, time_slice_id: i64) -> rusqlite::Result<Vec<TimelineEventSummary>> {
        let mut stmt = self.conn.prepare("SELECT id, subject_ids_json, event_type, time_slice_id, source_ref FROM timeline_events WHERE time_slice_id = ?1 ORDER BY id")?;
        let rows = stmt.query_map(rusqlite::params![time_slice_id], |row| {
            Ok(TimelineEventSummary { id: row.get(0)?, subject_ids: serde_json::from_str(&row.get::<_,String>(1)?).unwrap_or_default(), event_type: row.get(2)?, time_slice_id: row.get(3)?, source_ref: row.get(4)? })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
    }
}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineEventSummary { pub id: i64, pub subject_ids: Vec<i64>, pub event_type: String, pub time_slice_id: i64, pub source_ref: String }
