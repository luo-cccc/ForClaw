fn chapter_mission_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ChapterMissionSummary> {
    let status: String = row.get(7)?;
    Ok(ChapterMissionSummary {
        id: row.get(0)?,
        project_id: row.get(1)?,
        chapter_title: row.get(2)?,
        mission: row.get(3)?,
        must_include: row.get(4)?,
        must_not: row.get(5)?,
        expected_ending: row.get(6)?,
        status: crate::writer_agent::kernel::normalize_chapter_mission_status(&status),
        source_ref: row.get(8)?,
        updated_at: row.get(9)?,
        blocked_reason: row.get::<_, String>(10).unwrap_or_default(),
        retired_history: row.get::<_, String>(11).unwrap_or_default(),
        reader_lack_this_chapter: row.get::<_, String>(12).unwrap_or_default(),
        relationship_soil_this_chapter: row.get::<_, String>(13).unwrap_or_default(),
        pressure_scene: row.get::<_, String>(14).unwrap_or_default(),
        interest_mechanism: row.get::<_, String>(15).unwrap_or_default(),
        payoff_target: row.get::<_, String>(16).unwrap_or_default(),
        payoff_path: row.get::<_, String>(17).unwrap_or_default(),
        next_lack_opened: row.get::<_, String>(18).unwrap_or_default(),
    })
}

fn chapter_result_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ChapterResultSummary> {
    let created_at: i64 = row.get(12)?;
    Ok(ChapterResultSummary {
        id: row.get(0)?,
        project_id: row.get(1)?,
        chapter_title: row.get(2)?,
        chapter_revision: row.get(3)?,
        summary: row.get(4)?,
        state_changes: string_vec_from_json(row.get::<_, String>(5)?.as_str()),
        character_progress: string_vec_from_json(row.get::<_, String>(6)?.as_str()),
        new_conflicts: string_vec_from_json(row.get::<_, String>(7)?.as_str()),
        new_clues: string_vec_from_json(row.get::<_, String>(8)?.as_str()),
        promise_updates: string_vec_from_json(row.get::<_, String>(9)?.as_str()),
        canon_updates: string_vec_from_json(row.get::<_, String>(10)?.as_str()),
        source_ref: row.get(11)?,
        created_at: created_at.max(0) as u64,
    })
}

fn memory_feedback_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<MemoryFeedbackSummary> {
    let source_error: String = row.get(4)?;
    let reason: String = row.get(6)?;
    let created_at: i64 = row.get(7)?;
    Ok(MemoryFeedbackSummary {
        slot: row.get(0)?,
        category: row.get(1)?,
        action: row.get(2)?,
        confidence_delta: row.get(3)?,
        source_error: if source_error.trim().is_empty() {
            None
        } else {
            Some(source_error)
        },
        proposal_id: row.get(5)?,
        reason: if reason.trim().is_empty() {
            None
        } else {
            Some(reason)
        },
        created_at: created_at.max(0) as u64,
    })
}

fn string_vec_json(values: &[String]) -> String {
    serde_json::to_string(values).unwrap_or_else(|_| "[]".to_string())
}

fn string_vec_from_json(value: &str) -> Vec<String> {
    serde_json::from_str::<Vec<String>>(value).unwrap_or_default()
}

fn snippet_for_storage(text: &str, max_chars: usize) -> String {
    let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.chars().count() <= max_chars {
        return normalized;
    }
    normalized.chars().take(max_chars).collect()
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
        rusqlite::params![table],
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
