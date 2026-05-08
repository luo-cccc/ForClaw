fn migrate_hermes_schema(conn: &Connection) -> SqlResult<()> {
    ensure_column(
        conn,
        "session_history",
        "created_at",
        "created_at TEXT DEFAULT ''",
    )?;
    backfill_empty_timestamp(conn, "session_history", "created_at")?;
    ensure_column(
        conn,
        "user_drift_profile",
        "confidence",
        "confidence REAL DEFAULT 0.0",
    )?;
    ensure_column(
        conn,
        "user_drift_profile",
        "source",
        "source TEXT DEFAULT 'extracted'",
    )?;
    ensure_column(
        conn,
        "hierarchical_summaries",
        "created_at",
        "created_at TEXT DEFAULT ''",
    )?;
    backfill_empty_timestamp(conn, "hierarchical_summaries", "created_at")?;
    ensure_column(
        conn,
        "agent_skills",
        "category",
        "category TEXT DEFAULT 'general'",
    )?;
    ensure_column(conn, "agent_skills", "active", "active INTEGER DEFAULT 1")?;
    ensure_column(
        conn,
        "agent_skills",
        "created_at",
        "created_at TEXT DEFAULT ''",
    )?;
    backfill_empty_timestamp(conn, "agent_skills", "created_at")?;
    ensure_column(
        conn,
        "character_state",
        "state_json",
        "state_json TEXT NOT NULL DEFAULT '{}'",
    )?;
    ensure_column(
        conn,
        "character_state",
        "status",
        "status TEXT NOT NULL DEFAULT 'active'",
    )?;
    ensure_column(
        conn,
        "character_state",
        "updated_at",
        "updated_at TEXT DEFAULT ''",
    )?;
    backfill_empty_timestamp(conn, "character_state", "updated_at")?;
    ensure_column(
        conn,
        "plot_thread",
        "description",
        "description TEXT DEFAULT ''",
    )?;
    ensure_column(
        conn,
        "plot_thread",
        "introduced_chapter",
        "introduced_chapter TEXT DEFAULT ''",
    )?;
    ensure_column(
        conn,
        "plot_thread",
        "resolved_chapter",
        "resolved_chapter TEXT DEFAULT ''",
    )?;
    ensure_column(
        conn,
        "plot_thread",
        "priority",
        "priority INTEGER DEFAULT 0",
    )?;
    ensure_column(
        conn,
        "plot_thread",
        "status",
        "status TEXT NOT NULL DEFAULT 'open'",
    )?;
    ensure_column(
        conn,
        "plot_thread",
        "created_at",
        "created_at TEXT DEFAULT ''",
    )?;
    backfill_empty_timestamp(conn, "plot_thread", "created_at")?;
    ensure_column(
        conn,
        "world_rule",
        "category",
        "category TEXT NOT NULL DEFAULT 'general'",
    )?;
    ensure_column(conn, "world_rule", "priority", "priority INTEGER DEFAULT 0")?;
    ensure_column(
        conn,
        "world_rule",
        "source_chapter",
        "source_chapter TEXT DEFAULT ''",
    )?;
    ensure_column(conn, "world_rule", "active", "active INTEGER DEFAULT 1")?;
    ensure_column(
        conn,
        "world_rule",
        "created_at",
        "created_at TEXT DEFAULT ''",
    )?;
    backfill_empty_timestamp(conn, "world_rule", "created_at")?;
    Ok(())
}

fn backfill_empty_timestamp(conn: &Connection, table: &str, column: &str) -> SqlResult<()> {
    if !table_exists(conn, table)? || !table_has_column(conn, table, column)? {
        return Ok(());
    }

    conn.execute_batch(&format!(
        "UPDATE {table} SET {column}=datetime('now') WHERE {column} IS NULL OR {column}=''"
    ))?;
    Ok(())
}

fn ensure_column(
    conn: &Connection,
    table: &str,
    column: &str,
    column_definition: &str,
) -> SqlResult<()> {
    if !table_exists(conn, table)? || table_has_column(conn, table, column)? {
        return Ok(());
    }

    conn.execute_batch(&format!(
        "ALTER TABLE {table} ADD COLUMN {column_definition}"
    ))?;
    Ok(())
}

fn table_exists(conn: &Connection, table: &str) -> SqlResult<bool> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
        params![table],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}

fn table_has_column(conn: &Connection, table: &str, column: &str) -> SqlResult<bool> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
    for row in rows {
        if row? == column {
            return Ok(true);
        }
    }
    Ok(false)
}
