fn string_list_json(values: &[String]) -> String {
    serde_json::to_string(values).unwrap_or_else(|_| "[]".to_string())
}

fn string_list_from_json(raw: &str) -> Vec<String> {
    serde_json::from_str::<Vec<String>>(raw).unwrap_or_default()
}

impl WriterMemory {
    pub fn upsert_volume(&self, volume: &VolumeSummary) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT INTO volumes
             (id, project_id, title, start_chapter, end_chapter, contract_json, mission_json, status, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, datetime('now'), datetime('now'))
             ON CONFLICT(project_id, id) DO UPDATE SET
                title=excluded.title,
                start_chapter=excluded.start_chapter,
                end_chapter=excluded.end_chapter,
                contract_json=excluded.contract_json,
                mission_json=excluded.mission_json,
                status=excluded.status,
                updated_at=datetime('now')",
            rusqlite::params![
                volume.id,
                volume.project_id,
                volume.title,
                volume.start_chapter,
                volume.end_chapter,
                serde_json::to_string(&volume.contract).unwrap_or_else(|_| "{}".to_string()),
                serde_json::to_string(&volume.mission).unwrap_or_else(|_| "{}".to_string()),
                volume.status,
            ],
        )?;
        Ok(())
    }

    pub fn list_volumes(&self, project_id: &str) -> rusqlite::Result<Vec<VolumeSummary>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, project_id, title, start_chapter, end_chapter, contract_json, mission_json, status, created_at, updated_at
             FROM volumes
             WHERE project_id=?1
             ORDER BY start_chapter ASC, id ASC",
        )?;
        let rows = stmt.query_map(rusqlite::params![project_id], |row| {
            Ok(VolumeSummary {
                id: row.get(0)?,
                project_id: row.get(1)?,
                title: row.get(2)?,
                start_chapter: row.get(3)?,
                end_chapter: row.get(4)?,
                contract: serde_json::from_str::<serde_json::Value>(&row.get::<_, String>(5)?)
                    .unwrap_or_else(|_| serde_json::json!({})),
                mission: serde_json::from_str::<serde_json::Value>(&row.get::<_, String>(6)?)
                    .unwrap_or_else(|_| serde_json::json!({})),
                status: row.get(7)?,
                created_at: row.get(8)?,
                updated_at: row.get(9)?,
            })
        })?;
        rows.collect()
    }

    pub fn delete_volume(&self, project_id: &str, volume_id: &str) -> rusqlite::Result<bool> {
        let changed = self.conn.execute(
            "DELETE FROM volumes WHERE project_id=?1 AND id=?2",
            rusqlite::params![project_id, volume_id],
        )?;
        Ok(changed > 0)
    }

    pub fn find_volume_for_chapter(
        &self,
        project_id: &str,
        chapter_number: i64,
    ) -> rusqlite::Result<Option<VolumeSummary>> {
        self.conn
            .query_row(
                "SELECT id, project_id, title, start_chapter, end_chapter, contract_json, mission_json, status, created_at, updated_at
                 FROM volumes
                 WHERE project_id=?1 AND start_chapter<=?2 AND end_chapter>=?2
                 ORDER BY start_chapter ASC
                 LIMIT 1",
                rusqlite::params![project_id, chapter_number],
                |row| {
                    Ok(VolumeSummary {
                        id: row.get(0)?,
                        project_id: row.get(1)?,
                        title: row.get(2)?,
                        start_chapter: row.get(3)?,
                        end_chapter: row.get(4)?,
                        contract: serde_json::from_str::<serde_json::Value>(&row.get::<_, String>(5)?)
                            .unwrap_or_else(|_| serde_json::json!({})),
                        mission: serde_json::from_str::<serde_json::Value>(&row.get::<_, String>(6)?)
                            .unwrap_or_else(|_| serde_json::json!({})),
                        status: row.get(7)?,
                        created_at: row.get(8)?,
                        updated_at: row.get(9)?,
                    })
                },
            )
            .optional()
    }

    pub fn upsert_volume_snapshot(
        &self,
        snapshot: &VolumeSnapshotSummary,
    ) -> rusqlite::Result<i64> {
        self.conn.execute(
            "INSERT INTO volume_snapshots (project_id, volume_id, snapshot_json, created_at)
             VALUES (?1, ?2, ?3, datetime('now'))",
            rusqlite::params![
                snapshot.project_id,
                snapshot.volume_id,
                serde_json::to_string(&snapshot.snapshot).unwrap_or_else(|_| "{}".to_string()),
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn get_latest_volume_snapshot(
        &self,
        project_id: &str,
        volume_id: &str,
    ) -> rusqlite::Result<Option<VolumeSnapshotSummary>> {
        self.conn
            .query_row(
                "SELECT project_id, volume_id, snapshot_json, created_at
                 FROM volume_snapshots
                 WHERE project_id=?1 AND volume_id=?2
                 ORDER BY id DESC
                 LIMIT 1",
                rusqlite::params![project_id, volume_id],
                |row| {
                    Ok(VolumeSnapshotSummary {
                        project_id: row.get(0)?,
                        volume_id: row.get(1)?,
                        snapshot: serde_json::from_str::<serde_json::Value>(&row.get::<_, String>(2)?)
                            .unwrap_or_else(|_| serde_json::json!({})),
                        created_at: row.get(3)?,
                    })
                },
            )
            .optional()
    }

    pub fn upsert_arc_snapshot(&self, arc: &ArcSnapshotSummary) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT INTO arc_snapshots
             (arc_id, project_id, volume_id, title, start_chapter, end_chapter, snapshot_json, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, datetime('now'), datetime('now'))
             ON CONFLICT(project_id, arc_id) DO UPDATE SET
                volume_id=excluded.volume_id,
                title=excluded.title,
                start_chapter=excluded.start_chapter,
                end_chapter=excluded.end_chapter,
                snapshot_json=excluded.snapshot_json,
                updated_at=datetime('now')",
            rusqlite::params![
                arc.arc_id,
                arc.project_id,
                arc.volume_id,
                arc.title,
                arc.start_chapter,
                arc.end_chapter,
                serde_json::to_string(&arc.snapshot).unwrap_or_else(|_| "{}".to_string()),
            ],
        )?;
        Ok(())
    }

    pub fn list_arc_snapshots(
        &self,
        project_id: &str,
        volume_id: &str,
    ) -> rusqlite::Result<Vec<ArcSnapshotSummary>> {
        let mut stmt = self.conn.prepare(
            "SELECT arc_id, project_id, volume_id, title, start_chapter, end_chapter, snapshot_json, created_at, updated_at
             FROM arc_snapshots
             WHERE project_id=?1 AND volume_id=?2
             ORDER BY start_chapter ASC, arc_id ASC",
        )?;
        let rows = stmt.query_map(rusqlite::params![project_id, volume_id], |row| {
            Ok(ArcSnapshotSummary {
                arc_id: row.get(0)?,
                project_id: row.get(1)?,
                volume_id: row.get(2)?,
                title: row.get(3)?,
                start_chapter: row.get(4)?,
                end_chapter: row.get(5)?,
                snapshot: serde_json::from_str::<serde_json::Value>(&row.get::<_, String>(6)?)
                    .unwrap_or_else(|_| serde_json::json!({})),
                created_at: row.get(7)?,
                updated_at: row.get(8)?,
            })
        })?;
        rows.collect()
    }

    pub fn upsert_book_state(&self, book: &BookStateSummary) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT INTO book_state
             (project_id, title, long_term_constraints_json, mega_promises_json, irreversible_changes_json, source_ref, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, datetime('now'))
             ON CONFLICT(project_id) DO UPDATE SET
                title=excluded.title,
                long_term_constraints_json=excluded.long_term_constraints_json,
                mega_promises_json=excluded.mega_promises_json,
                irreversible_changes_json=excluded.irreversible_changes_json,
                source_ref=excluded.source_ref,
                updated_at=datetime('now')",
            rusqlite::params![
                book.project_id,
                book.title,
                string_list_json(&book.long_term_constraints),
                string_list_json(&book.mega_promises),
                string_list_json(&book.irreversible_changes),
                book.source_ref,
            ],
        )?;
        Ok(())
    }

    pub fn get_book_state(&self, project_id: &str) -> rusqlite::Result<Option<BookStateSummary>> {
        self.conn
            .query_row(
                "SELECT project_id, title, long_term_constraints_json, mega_promises_json, irreversible_changes_json, source_ref, updated_at
                 FROM book_state
                 WHERE project_id=?1",
                rusqlite::params![project_id],
                |row| {
                    Ok(BookStateSummary {
                        project_id: row.get(0)?,
                        title: row.get(1)?,
                        long_term_constraints: string_list_from_json(&row.get::<_, String>(2)?),
                        mega_promises: string_list_from_json(&row.get::<_, String>(3)?),
                        irreversible_changes: string_list_from_json(&row.get::<_, String>(4)?),
                        source_ref: row.get(5)?,
                        updated_at: row.get(6)?,
                    })
                },
            )
            .optional()
    }

    pub fn ensure_default_book_state(
        &self,
        project_id: &str,
        title: &str,
    ) -> rusqlite::Result<bool> {
        if self.get_book_state(project_id)?.is_some() {
            return Ok(false);
        }
        self.upsert_book_state(&BookStateSummary {
            project_id: project_id.to_string(),
            title: title.to_string(),
            long_term_constraints: vec!["Preserve established canon and approved story contract.".to_string()],
            mega_promises: Vec::new(),
            irreversible_changes: Vec::new(),
            source_ref: "book_state.seed".to_string(),
            updated_at: String::new(),
        })?;
        Ok(true)
    }
}

#[cfg(test)]
mod volume_arc_book_tests {
    use super::*;

    #[test]
    fn volume_crud_and_scope_lookup_roundtrip() {
        let memory = WriterMemory::open(std::path::Path::new(":memory:")).unwrap();
        let volume = VolumeSummary {
            id: "v1".to_string(),
            project_id: "eval".to_string(),
            title: "第一卷".to_string(),
            start_chapter: 1,
            end_chapter: 50,
            contract: serde_json::json!({"promise": "keep tension"}),
            mission: serde_json::json!({"goal": "setup"}),
            status: "active".to_string(),
            created_at: String::new(),
            updated_at: String::new(),
        };
        memory.upsert_volume(&volume).unwrap();
        let volumes = memory.list_volumes("eval").unwrap();
        assert_eq!(volumes.len(), 1);
        let scoped = memory.find_volume_for_chapter("eval", 32).unwrap().unwrap();
        assert_eq!(scoped.id, "v1");
        assert!(memory.delete_volume("eval", "v1").unwrap());
    }

    #[test]
    fn default_book_state_roundtrip() {
        let memory = WriterMemory::open(std::path::Path::new(":memory:")).unwrap();
        assert!(memory.ensure_default_book_state("eval", "Book A").unwrap());
        let state = memory.get_book_state("eval").unwrap().unwrap();
        assert_eq!(state.title, "Book A");
        assert!(!state.long_term_constraints.is_empty());
    }
}
