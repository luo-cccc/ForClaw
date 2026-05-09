#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CraftRuleStats {
    pub rule_id: String,
    pub accepted_count: u32,
    pub rejected_count: u32,
}

#[derive(Debug, Clone)]
pub struct CraftFeedbackEvent {
    pub rule_id: String,
    pub scope: String,
    pub action: String,
    pub matched_metrics: Vec<String>,
    pub score_before: f32,
    pub score_after: f32,
    pub evidence_ref: String,
    pub reason: String,
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
        );
        CREATE TABLE IF NOT EXISTS craft_feedback_events (
            id TEXT PRIMARY KEY,
            rule_id TEXT NOT NULL,
            scope TEXT NOT NULL DEFAULT '',
            action TEXT NOT NULL,
            matched_metrics TEXT NOT NULL DEFAULT '',
            score_before REAL NOT NULL DEFAULT 0,
            score_after REAL NOT NULL DEFAULT 0,
            evidence_ref TEXT NOT NULL DEFAULT '',
            reason TEXT NOT NULL DEFAULT '',
            created_at INTEGER NOT NULL DEFAULT 0
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

pub fn record_craft_feedback_event(
    conn: &Connection,
    event: &CraftFeedbackEvent,
) -> Result<(), String> {
    let created_at = crate::agent_runtime::now_ms();
    let id = format!(
        "{}-{}-{}-{}",
        event.rule_id,
        event.scope,
        event.action,
        created_at
    );
    conn.execute(
        "INSERT INTO craft_feedback_events
         (id, rule_id, scope, action, matched_metrics, score_before, score_after, evidence_ref, reason, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        rusqlite::params![
            id,
            event.rule_id,
            event.scope,
            event.action,
            event.matched_metrics.join(","),
            event.score_before,
            event.score_after,
            event.evidence_ref,
            event.reason,
            created_at as i64,
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
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

    #[test]
    fn record_feedback_event_persists_metric_evidence() {
        let conn = test_conn();
        record_craft_feedback_event(
            &conn,
            &CraftFeedbackEvent {
                rule_id: "dialogue_function".to_string(),
                scope: "chapter-7".to_string(),
                action: "accepted".to_string(),
                matched_metrics: vec!["dialogue_function".to_string()],
                score_before: 0.2,
                score_after: 0.8,
                evidence_ref: "revision_report:dialogue_function".to_string(),
                reason: "metric improved".to_string(),
            },
        )
        .unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM craft_feedback_events WHERE rule_id = ?1 AND action = ?2",
                rusqlite::params!["dialogue_function", "accepted"],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }
}
