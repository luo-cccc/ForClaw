#[derive(Debug, Clone)]
pub struct CraftRuleStats {
    pub rule_id: String,
    pub accepted_count: u32,
    pub rejected_count: u32,
}

impl CraftRuleStats {
    pub fn acceptance_rate(&self) -> f32 {
        let total = self.accepted_count + self.rejected_count;
        if total == 0 {
            0.5
        } else {
            self.accepted_count as f32 / total as f32
        }
    }
}

pub fn ensure_craft_tables(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS craft_rules (
            id TEXT PRIMARY KEY,
            rule_id TEXT NOT NULL,
            scope TEXT NOT NULL DEFAULT '',
            accepted_count INTEGER NOT NULL DEFAULT 0,
            rejected_count INTEGER NOT NULL DEFAULT 0
        );
        CREATE TABLE IF NOT EXISTS craft_examples (
            id TEXT PRIMARY KEY,
            excerpt_ref TEXT NOT NULL,
            reason TEXT NOT NULL DEFAULT '',
            pattern TEXT NOT NULL DEFAULT '',
            scene_types TEXT NOT NULL DEFAULT ''
        );
        CREATE TABLE IF NOT EXISTS craft_bad_patterns (
            id TEXT PRIMARY KEY,
            pattern TEXT NOT NULL,
            correction TEXT NOT NULL DEFAULT '',
            rejected_count INTEGER NOT NULL DEFAULT 0
        );",
    )
    .map_err(|e| e.to_string())
}

pub fn record_craft_accept(conn: &Connection, rule_id: &str, scope: &str) -> Result<(), String> {
    conn.execute(
        "INSERT INTO craft_rules (id, rule_id, scope, accepted_count, rejected_count)
         VALUES (?1, ?2, ?3, 1, 0)
         ON CONFLICT(id) DO UPDATE SET accepted_count = accepted_count + 1",
        rusqlite::params![format!("{}-{}", rule_id, scope), rule_id, scope],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn record_craft_reject(conn: &Connection, rule_id: &str, scope: &str) -> Result<(), String> {
    conn.execute(
        "INSERT INTO craft_rules (id, rule_id, scope, accepted_count, rejected_count)
         VALUES (?1, ?2, ?3, 0, 1)
         ON CONFLICT(id) DO UPDATE SET rejected_count = rejected_count + 1",
        rusqlite::params![format!("{}-{}", rule_id, scope), rule_id, scope],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn get_craft_rule_stats(conn: &Connection, rule_id: &str) -> Option<CraftRuleStats> {
    conn.query_row(
        "SELECT rule_id, SUM(accepted_count), SUM(rejected_count)
         FROM craft_rules WHERE rule_id = ?1 GROUP BY rule_id",
        rusqlite::params![rule_id],
        |row| {
            Ok(CraftRuleStats {
                rule_id: row.get(0)?,
                accepted_count: row.get(1)?,
                rejected_count: row.get(2)?,
            })
        },
    )
    .ok()
}

#[cfg(test)]
mod craft_memory_tests {
    use super::*;

    fn test_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        ensure_craft_tables(&conn).unwrap();
        conn
    }

    #[test]
    fn ensure_tables_creates_without_error() {
        let conn = test_conn();
        let count: i32 = conn
            .query_row(
                "SELECT COUNT(*) FROM craft_rules",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn record_accept_upserts() {
        let conn = test_conn();
        record_craft_accept(&conn, "dialogue_function", "chapter-7").unwrap();
        record_craft_accept(&conn, "dialogue_function", "chapter-7").unwrap();
        let stats = get_craft_rule_stats(&conn, "dialogue_function").unwrap();
        assert_eq!(stats.accepted_count, 2);
        assert_eq!(stats.rejected_count, 0);
    }

    #[test]
    fn record_reject_upserts() {
        let conn = test_conn();
        record_craft_reject(&conn, "ending_hook", "chapter-3").unwrap();
        record_craft_reject(&conn, "ending_hook", "chapter-3").unwrap();
        let stats = get_craft_rule_stats(&conn, "ending_hook").unwrap();
        assert_eq!(stats.rejected_count, 2);
    }

    #[test]
    fn unknown_rule_returns_none() {
        let conn = test_conn();
        assert!(get_craft_rule_stats(&conn, "nonexistent").is_none());
    }

    #[test]
    fn acceptance_rate_calculation() {
        let stats = CraftRuleStats {
            rule_id: "test".into(),
            accepted_count: 7,
            rejected_count: 3,
        };
        assert!((stats.acceptance_rate() - 0.7).abs() < 0.01);
    }

    #[test]
    fn zero_total_defaults_to_half() {
        let stats = CraftRuleStats {
            rule_id: "test".into(),
            accepted_count: 0,
            rejected_count: 0,
        };
        assert_eq!(stats.acceptance_rate(), 0.5);
    }
}
