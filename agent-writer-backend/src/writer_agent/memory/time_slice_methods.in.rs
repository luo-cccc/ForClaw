impl WriterMemory {
    pub fn upsert_time_slice(&self, label: &str, relative_order: i32, start_ref: &str, end_ref: &str) -> rusqlite::Result<i64> {
        self.conn.execute(
            "INSERT INTO story_time_slices (label, relative_order, start_ref, end_ref) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![label, relative_order, start_ref, end_ref],
        )?;
        Ok(self.conn.last_insert_rowid())
    }
    pub fn list_time_slices(&self) -> rusqlite::Result<Vec<TimeSliceSummary>> {
        let mut stmt = self.conn.prepare("SELECT id, label, relative_order, start_ref, end_ref FROM story_time_slices ORDER BY relative_order")?;
        let rows = stmt.query_map([], |row| {
            Ok(TimeSliceSummary { id: row.get(0)?, label: row.get(1)?, relative_order: row.get(2)?, start_ref: row.get(3)?, end_ref: row.get(4)? })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
    }
    pub fn get_time_slice_by_id(&self, id: i64) -> rusqlite::Result<Option<TimeSliceSummary>> {
        let mut stmt = self.conn.prepare("SELECT id, label, relative_order, start_ref, end_ref FROM story_time_slices WHERE id = ?1")?;
        let mut rows = stmt.query_map(rusqlite::params![id], |row| {
            Ok(TimeSliceSummary { id: row.get(0)?, label: row.get(1)?, relative_order: row.get(2)?, start_ref: row.get(3)?, end_ref: row.get(4)? })
        })?;
        Ok(rows.next().and_then(|r| r.ok()))
    }
}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimeSliceSummary { pub id: i64, pub label: String, pub relative_order: i32, pub start_ref: String, pub end_ref: String }
