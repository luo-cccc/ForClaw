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

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CraftExampleMemory {
    pub id: String,
    pub rule_id: String,
    pub scope: String,
    pub excerpt_ref: String,
    pub excerpt: String,
    pub reason: String,
    pub pattern: String,
    pub scene_types: Vec<String>,
    pub score_delta: f32,
    pub created_at: u64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CraftBadPatternMemory {
    pub id: String,
    pub rule_id: String,
    pub scope: String,
    pub pattern: String,
    pub evidence_ref: String,
    pub evidence_excerpt: String,
    pub correction: String,
    pub rejected_count: u32,
    pub created_at: u64,
    pub updated_at: u64,
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
            rule_id TEXT NOT NULL DEFAULT '',
            scope TEXT NOT NULL DEFAULT '',
            excerpt_ref TEXT NOT NULL,
            excerpt TEXT NOT NULL DEFAULT '',
            reason TEXT NOT NULL DEFAULT '',
            pattern TEXT NOT NULL DEFAULT '',
            scene_types TEXT NOT NULL DEFAULT '',
            score_delta REAL NOT NULL DEFAULT 0,
            created_at INTEGER NOT NULL DEFAULT 0
        );
        CREATE TABLE IF NOT EXISTS craft_bad_patterns (
            id TEXT PRIMARY KEY,
            rule_id TEXT NOT NULL DEFAULT '',
            scope TEXT NOT NULL DEFAULT '',
            pattern TEXT NOT NULL,
            evidence_ref TEXT NOT NULL DEFAULT '',
            evidence_excerpt TEXT NOT NULL DEFAULT '',
            correction TEXT NOT NULL DEFAULT '',
            rejected_count INTEGER NOT NULL DEFAULT 0,
            created_at INTEGER NOT NULL DEFAULT 0,
            updated_at INTEGER NOT NULL DEFAULT 0
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
    .map_err(|e| e.to_string())?;
    ensure_craft_column(conn, "craft_examples", "rule_id", "rule_id TEXT NOT NULL DEFAULT ''")?;
    ensure_craft_column(conn, "craft_examples", "scope", "scope TEXT NOT NULL DEFAULT ''")?;
    ensure_craft_column(conn, "craft_examples", "excerpt", "excerpt TEXT NOT NULL DEFAULT ''")?;
    ensure_craft_column(
        conn,
        "craft_examples",
        "score_delta",
        "score_delta REAL NOT NULL DEFAULT 0",
    )?;
    ensure_craft_column(
        conn,
        "craft_examples",
        "created_at",
        "created_at INTEGER NOT NULL DEFAULT 0",
    )?;
    ensure_craft_column(conn, "craft_bad_patterns", "rule_id", "rule_id TEXT NOT NULL DEFAULT ''")?;
    ensure_craft_column(conn, "craft_bad_patterns", "scope", "scope TEXT NOT NULL DEFAULT ''")?;
    ensure_craft_column(
        conn,
        "craft_bad_patterns",
        "evidence_ref",
        "evidence_ref TEXT NOT NULL DEFAULT ''",
    )?;
    ensure_craft_column(
        conn,
        "craft_bad_patterns",
        "evidence_excerpt",
        "evidence_excerpt TEXT NOT NULL DEFAULT ''",
    )?;
    ensure_craft_column(
        conn,
        "craft_bad_patterns",
        "created_at",
        "created_at INTEGER NOT NULL DEFAULT 0",
    )?;
    ensure_craft_column(
        conn,
        "craft_bad_patterns",
        "updated_at",
        "updated_at INTEGER NOT NULL DEFAULT 0",
    )?;
    Ok(())
}

fn ensure_craft_column(
    conn: &Connection,
    table: &str,
    column: &str,
    definition: &str,
) -> Result<(), String> {
    let pragma = format!("PRAGMA table_info({})", table);
    let mut stmt = conn.prepare(&pragma).map_err(|e| e.to_string())?;
    let exists = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|e| e.to_string())?
        .filter_map(Result::ok)
        .any(|name| name == column);
    if !exists {
        conn.execute_batch(&format!("ALTER TABLE {} ADD COLUMN {};", table, definition))
            .map_err(|e| e.to_string())?;
    }
    Ok(())
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

pub fn record_craft_example(
    conn: &Connection,
    example: &CraftExampleMemory,
) -> Result<(), String> {
    conn.execute(
        "INSERT INTO craft_examples
         (id, rule_id, scope, excerpt_ref, excerpt, reason, pattern, scene_types, score_delta, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
         ON CONFLICT(id) DO UPDATE SET
            excerpt=excluded.excerpt,
            reason=excluded.reason,
            pattern=excluded.pattern,
            scene_types=excluded.scene_types,
            score_delta=excluded.score_delta",
        rusqlite::params![
            example.id,
            example.rule_id,
            example.scope,
            example.excerpt_ref,
            example.excerpt,
            example.reason,
            example.pattern,
            example.scene_types.join(","),
            example.score_delta,
            example.created_at as i64,
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn record_craft_bad_pattern(
    conn: &Connection,
    pattern: &CraftBadPatternMemory,
) -> Result<(), String> {
    conn.execute(
        "INSERT INTO craft_bad_patterns
         (id, rule_id, scope, pattern, evidence_ref, evidence_excerpt, correction, rejected_count, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
         ON CONFLICT(id) DO UPDATE SET
            evidence_ref=excluded.evidence_ref,
            evidence_excerpt=excluded.evidence_excerpt,
            correction=excluded.correction,
            rejected_count=craft_bad_patterns.rejected_count + excluded.rejected_count,
            updated_at=excluded.updated_at",
        rusqlite::params![
            pattern.id,
            pattern.rule_id,
            pattern.scope,
            pattern.pattern,
            pattern.evidence_ref,
            pattern.evidence_excerpt,
            pattern.correction,
            pattern.rejected_count,
            pattern.created_at as i64,
            pattern.updated_at as i64,
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn list_craft_examples(
    conn: &Connection,
    rule_id: &str,
    limit: usize,
) -> Result<Vec<CraftExampleMemory>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, rule_id, scope, excerpt_ref, excerpt, reason, pattern, scene_types, score_delta, created_at
             FROM craft_examples
             WHERE rule_id = ?1
             ORDER BY created_at DESC
             LIMIT ?2",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(rusqlite::params![rule_id, limit as i64], |row| {
            let scene_types: String = row.get(7)?;
            Ok(CraftExampleMemory {
                id: row.get(0)?,
                rule_id: row.get(1)?,
                scope: row.get(2)?,
                excerpt_ref: row.get(3)?,
                excerpt: row.get(4)?,
                reason: row.get(5)?,
                pattern: row.get(6)?,
                scene_types: split_csv(&scene_types),
                score_delta: row.get(8)?,
                created_at: row.get::<_, i64>(9)? as u64,
            })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|e| e.to_string())
}

pub fn list_craft_bad_patterns(
    conn: &Connection,
    rule_id: &str,
    limit: usize,
) -> Result<Vec<CraftBadPatternMemory>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, rule_id, scope, pattern, evidence_ref, evidence_excerpt, correction, rejected_count, created_at, updated_at
             FROM craft_bad_patterns
             WHERE rule_id = ?1
             ORDER BY rejected_count DESC, updated_at DESC
             LIMIT ?2",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(rusqlite::params![rule_id, limit as i64], |row| {
            Ok(CraftBadPatternMemory {
                id: row.get(0)?,
                rule_id: row.get(1)?,
                scope: row.get(2)?,
                pattern: row.get(3)?,
                evidence_ref: row.get(4)?,
                evidence_excerpt: row.get(5)?,
                correction: row.get(6)?,
                rejected_count: row.get(7)?,
                created_at: row.get::<_, i64>(8)? as u64,
                updated_at: row.get::<_, i64>(9)? as u64,
            })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|e| e.to_string())
}

fn split_csv(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(ToString::to_string)
        .collect()
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

    #[test]
    fn records_good_examples_and_bad_patterns() {
        let conn = test_conn();
        let example = CraftExampleMemory {
            id: "dialogue_function-ch7-example".to_string(),
            rule_id: "dialogue_function".to_string(),
            scope: "chapter-7".to_string(),
            excerpt_ref: "revision_report:chapter-7:dialogue_function".to_string(),
            excerpt: "他说：你必须选择。".to_string(),
            reason: "dialogue metric improved".to_string(),
            pattern: "dialogue_function".to_string(),
            scene_types: vec!["chapter_revision".to_string()],
            score_delta: 0.42,
            created_at: 100,
        };
        record_craft_example(&conn, &example).unwrap();

        let bad = CraftBadPatternMemory {
            id: "ending_hook-ch8-bad".to_string(),
            rule_id: "ending_hook".to_string(),
            scope: "chapter-8".to_string(),
            pattern: "ending_hook".to_string(),
            evidence_ref: "revision_report:chapter-8:ending_hook".to_string(),
            evidence_excerpt: "结尾没有后果。".to_string(),
            correction: "章末交付后果并留下选择。".to_string(),
            rejected_count: 1,
            created_at: 110,
            updated_at: 110,
        };
        record_craft_bad_pattern(&conn, &bad).unwrap();
        record_craft_bad_pattern(&conn, &bad).unwrap();

        let examples = list_craft_examples(&conn, "dialogue_function", 10).unwrap();
        assert_eq!(examples, vec![example]);

        let patterns = list_craft_bad_patterns(&conn, "ending_hook", 10).unwrap();
        assert_eq!(patterns.len(), 1);
        assert_eq!(patterns[0].rejected_count, 2);
        assert_eq!(patterns[0].correction, "章末交付后果并留下选择。");
    }
}
