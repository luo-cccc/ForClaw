#[cfg(test)]
mod tests {
    use super::*;

    fn memory_db() -> HermesDB {
        let conn = Connection::open_in_memory().unwrap();
        let db = HermesDB { conn };
        db.initialize().unwrap();
        db
    }

    #[test]
    fn upsert_character_state_updates_existing_chapter_state() {
        let db = memory_db();
        db.upsert_character_state("林墨", "chapter-1", r#"{"mood":"calm"}"#)
            .unwrap();
        db.upsert_character_state("林墨", "chapter-1", r#"{"mood":"angry"}"#)
            .unwrap();

        let rows = db.get_characters_for_chapter("chapter-1").unwrap();
        assert_eq!(rows.len(), 1);
        assert!(rows[0].1.contains("angry"));
    }

    #[test]
    fn search_sessions_uses_fts_index() {
        let db = memory_db();
        db.log_interaction("user", "Lin Mo found a hidden door in the ruined temple.")
            .unwrap();
        db.log_interaction("assistant", "其他无关内容").unwrap();

        let rows = db.search_sessions("hidden", 5).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].role, "user");
        assert!(rows[0].content.contains("hidden door"));
    }

    #[test]
    fn initialize_migrates_legacy_hermes_columns() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE agent_skills (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                skill TEXT NOT NULL
            );
            INSERT INTO agent_skills (skill) VALUES ('偏好克制对白');
            CREATE TABLE character_state (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                chapter_id TEXT NOT NULL
            );
            INSERT INTO character_state (name, chapter_id) VALUES ('林墨', 'chapter-1');
            CREATE TABLE plot_thread (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL
            );
            INSERT INTO plot_thread (name) VALUES ('玉佩去向');
            CREATE TABLE world_rule (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                rule TEXT NOT NULL UNIQUE
            );
            INSERT INTO world_rule (rule) VALUES ('禁止复活');",
        )
        .unwrap();
        let db = HermesDB { conn };

        db.initialize().unwrap();

        assert!(table_has_column(&db.conn, "agent_skills", "active").unwrap());
        assert!(table_has_column(&db.conn, "character_state", "state_json").unwrap());
        assert!(table_has_column(&db.conn, "plot_thread", "introduced_chapter").unwrap());
        assert!(table_has_column(&db.conn, "world_rule", "active").unwrap());
        let version: i64 = db
            .conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, HERMES_SCHEMA_VERSION);

        let skills = db.get_active_skills().unwrap();
        assert_eq!(skills[0].skill, "偏好克制对白");
        assert!(db.get_characters_for_chapter("chapter-1").unwrap()[0]
            .1
            .contains("{}"));
    }
}
