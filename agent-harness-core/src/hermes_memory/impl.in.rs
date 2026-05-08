impl HermesDB {
    pub fn open(path: &Path) -> SqlResult<Self> {
        let conn = Connection::open(path)?;
        let db = Self { conn };
        db.initialize()?;
        Ok(db)
    }

    fn initialize(&self) -> SqlResult<()> {
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS session_history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                role TEXT NOT NULL,
                content TEXT NOT NULL,
                created_at TEXT DEFAULT (datetime('now'))
            );
            CREATE INDEX IF NOT EXISTS idx_session_role ON session_history(role);
            CREATE INDEX IF NOT EXISTS idx_session_created ON session_history(created_at);
            CREATE VIRTUAL TABLE IF NOT EXISTS session_history_fts USING fts5(
                content,
                role UNINDEXED,
                created_at UNINDEXED,
                content='session_history',
                content_rowid='id'
            );

            CREATE TABLE IF NOT EXISTS user_drift_profile (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                key TEXT NOT NULL UNIQUE,
                value TEXT NOT NULL,
                confidence REAL DEFAULT 0.0,
                source TEXT DEFAULT 'extracted'
            );

            CREATE TABLE IF NOT EXISTS hierarchical_summaries (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                level TEXT NOT NULL CHECK(level IN ('chunk','chapter','book')),
                ref_key TEXT NOT NULL,
                summary TEXT NOT NULL,
                created_at TEXT DEFAULT (datetime('now'))
            );
            CREATE INDEX IF NOT EXISTS idx_hs_level_key ON hierarchical_summaries(level, ref_key);

            CREATE TABLE IF NOT EXISTS agent_skills (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                skill TEXT NOT NULL,
                category TEXT DEFAULT 'general',
                active INTEGER DEFAULT 1,
                created_at TEXT DEFAULT (datetime('now'))
            );

            CREATE TABLE IF NOT EXISTS character_state (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                chapter_id TEXT NOT NULL,
                state_json TEXT NOT NULL DEFAULT '{}',
                status TEXT NOT NULL DEFAULT 'active',
                updated_at TEXT DEFAULT (datetime('now'))
            );
            CREATE INDEX IF NOT EXISTS idx_char_name ON character_state(name);
            DELETE FROM character_state
            WHERE id NOT IN (
                SELECT MAX(id) FROM character_state GROUP BY name, chapter_id
            );

            CREATE TABLE IF NOT EXISTS plot_thread (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                description TEXT DEFAULT '',
                introduced_chapter TEXT DEFAULT '',
                resolved_chapter TEXT DEFAULT '',
                priority INTEGER DEFAULT 0,
                status TEXT NOT NULL DEFAULT 'open'
                    CHECK(status IN ('open','foreshadowed','developed','resolved')),
                created_at TEXT DEFAULT (datetime('now'))
            );

            CREATE TABLE IF NOT EXISTS world_rule (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                rule TEXT NOT NULL UNIQUE,
                category TEXT NOT NULL DEFAULT 'general',
                priority INTEGER DEFAULT 0,
                source_chapter TEXT DEFAULT '',
                active INTEGER DEFAULT 1,
                created_at TEXT DEFAULT (datetime('now'))
            );

            INSERT INTO session_history_fts(rowid, content, role, created_at)
            SELECT id, content, role, created_at
            FROM session_history
             WHERE id NOT IN (SELECT rowid FROM session_history_fts);",
        )?;
        migrate_hermes_schema(&self.conn)?;
        self.conn.execute_batch(
            "CREATE UNIQUE INDEX IF NOT EXISTS idx_char_name_chapter
                ON character_state(name, chapter_id);
            CREATE INDEX IF NOT EXISTS idx_plot_status ON plot_thread(status);",
        )?;
        self.conn
            .pragma_update(None, "user_version", HERMES_SCHEMA_VERSION)?;
        Ok(())
    }

    // ---- session_history ----

    pub fn log_interaction(&self, role: &str, content: &str) -> SqlResult<i64> {
        self.conn.execute(
            "INSERT INTO session_history (role, content) VALUES (?1, ?2)",
            params![role, content],
        )?;
        let id = self.conn.last_insert_rowid();
        self.conn.execute(
            "INSERT INTO session_history_fts(rowid, content, role, created_at)
             SELECT id, content, role, created_at FROM session_history WHERE id = ?1",
            params![id],
        )?;
        Ok(id)
    }

    pub fn recent_interactions(&self, limit: usize) -> SqlResult<Vec<SessionRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, role, content, created_at FROM session_history
             ORDER BY id DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit as i64], |row| {
            Ok(SessionRecord {
                id: row.get(0)?,
                role: row.get(1)?,
                content: row.get(2)?,
                created_at: row.get(3)?,
            })
        })?;
        let mut records = Vec::new();
        for r in rows {
            records.push(r?);
        }
        records.reverse();
        Ok(records)
    }

    // ---- user_drift_profile ----

    pub fn upsert_drift(&self, key: &str, value: &str, confidence: f64) -> SqlResult<()> {
        self.conn.execute(
            "INSERT INTO user_drift_profile (key, value, confidence)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(key) DO UPDATE SET value=?2, confidence=?3",
            params![key, value, confidence],
        )?;
        Ok(())
    }

    pub fn get_drift_profiles(&self) -> SqlResult<Vec<DriftProfile>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, key, value, confidence, source FROM user_drift_profile")?;
        let rows = stmt.query_map([], |row| {
            Ok(DriftProfile {
                id: row.get(0)?,
                key: row.get(1)?,
                value: row.get(2)?,
                confidence: row.get(3)?,
                source: row.get(4)?,
            })
        })?;
        let mut profiles = Vec::new();
        for r in rows {
            profiles.push(r?);
        }
        Ok(profiles)
    }

    // ---- agent_skills ----

    pub fn insert_skill(&self, skill: &str, category: &str) -> SqlResult<i64> {
        self.conn.execute(
            "INSERT INTO agent_skills (skill, category) VALUES (?1, ?2)",
            params![skill, category],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn get_active_skills(&self) -> SqlResult<Vec<AgentSkill>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, skill, category, active, created_at FROM agent_skills WHERE active = 1",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(AgentSkill {
                id: row.get(0)?,
                skill: row.get(1)?,
                category: row.get(2)?,
                active: row.get(3)?,
                created_at: row.get(4)?,
            })
        })?;
        let mut skills = Vec::new();
        for r in rows {
            skills.push(r?);
        }
        Ok(skills)
    }

    pub fn search_skills(&self, keyword: &str) -> SqlResult<Vec<AgentSkill>> {
        let pattern = format!("%{}%", keyword);
        let mut stmt = self.conn.prepare(
            "SELECT id, skill, category, active, created_at FROM agent_skills
             WHERE active = 1 AND skill LIKE ?1",
        )?;
        let rows = stmt.query_map(params![pattern], |row| {
            Ok(AgentSkill {
                id: row.get(0)?,
                skill: row.get(1)?,
                category: row.get(2)?,
                active: row.get(3)?,
                created_at: row.get(4)?,
            })
        })?;
        let mut skills = Vec::new();
        for r in rows {
            skills.push(r?);
        }
        Ok(skills)
    }

    pub fn deactivate_skill(&self, id: i64) -> SqlResult<()> {
        self.conn.execute(
            "UPDATE agent_skills SET active = 0 WHERE id = ?1",
            params![id],
        )?;
        Ok(())
    }

    /// Consolidation: decay → merge → prune (SimpleMem pattern)
    /// Returns (decayed, merged, pruned) counts
    pub fn consolidate(&self) -> SqlResult<(usize, usize, usize)> {
        // 1. Decay: deactivate skills older than 90 days
        let decayed = self.conn.execute(
            "UPDATE agent_skills SET active = 0
             WHERE active = 1 AND julianday('now') - julianday(created_at) > 90",
            [],
        )?;

        // 2. Merge: deactivate exact duplicates (keep earliest)
        let merged = self.conn.execute(
            "UPDATE agent_skills SET active = 0 WHERE id IN (
                SELECT a.id FROM agent_skills a
                INNER JOIN agent_skills b ON a.skill = b.skill
                AND a.id > b.id AND a.active = 1 AND b.active = 1
            )",
            [],
        )?;

        // 3. Prune: keep only most recent 200 active skills
        let pruned = self.conn.execute(
            "UPDATE agent_skills SET active = 0
             WHERE active = 1 AND id NOT IN (
                 SELECT id FROM agent_skills WHERE active = 1
                 ORDER BY id DESC LIMIT 200
             )",
            [],
        )?;

        Ok((decayed, merged, pruned))
    }

    // ---- hierarchical_summaries ----

    pub fn upsert_summary(&self, level: &str, ref_key: &str, summary: &str) -> SqlResult<()> {
        self.conn.execute(
            "INSERT INTO hierarchical_summaries (level, ref_key, summary)
             VALUES (?1, ?2, ?3)
             ON CONFLICT DO UPDATE SET summary=?3",
            params![level, ref_key, summary],
        )?;
        Ok(())
    }

    pub fn get_summaries(&self, level: &str, limit: usize) -> SqlResult<Vec<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT summary FROM hierarchical_summaries WHERE level = ?1
             ORDER BY id DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![level, limit as i64], |row| row.get(0))?;
        let mut summaries = Vec::new();
        for r in rows {
            summaries.push(r?);
        }
        Ok(summaries)
    }

    pub fn get_recent_summaries_by_level(
        &self,
        level: &str,
        limit: usize,
    ) -> SqlResult<Vec<String>> {
        self.get_summaries(level, limit)
    }

    /// Clean session_history older than 7 days
    pub fn clean_old_sessions(&self) -> SqlResult<usize> {
        let deleted = self.conn.execute(
            "DELETE FROM session_history
             WHERE julianday('now') - julianday(created_at) > 7",
            [],
        )?;
        self.conn.execute(
            "DELETE FROM session_history_fts
             WHERE rowid NOT IN (SELECT id FROM session_history)",
            [],
        )?;
        Ok(deleted)
    }

    // ---- character_state ----

    pub fn upsert_character_state(
        &self,
        name: &str,
        chapter_id: &str,
        state_json: &str,
    ) -> SqlResult<()> {
        self.conn.execute(
            "INSERT INTO character_state (name, chapter_id, state_json, updated_at)
             VALUES (?1, ?2, ?3, datetime('now'))
             ON CONFLICT(name, chapter_id) DO UPDATE SET
             state_json = ?3, updated_at = datetime('now')",
            rusqlite::params![name, chapter_id, state_json],
        )?;
        Ok(())
    }

    pub fn get_characters_for_chapter(&self, chapter_id: &str) -> SqlResult<Vec<(String, String)>> {
        let mut stmt = self.conn.prepare(
            "SELECT name, state_json FROM character_state
             WHERE chapter_id = ?1 AND status = 'active'
             ORDER BY updated_at DESC",
        )?;
        let rows = stmt.query_map(rusqlite::params![chapter_id], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })?;
        rows.collect()
    }

    // ---- plot_thread ----

    pub fn add_plot_thread(
        &self,
        name: &str,
        description: &str,
        chapter: &str,
        priority: i32,
    ) -> SqlResult<i64> {
        self.conn.execute(
            "INSERT INTO plot_thread (name, description, introduced_chapter, priority)
             VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![name, description, chapter, priority],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn get_open_plot_threads(&self) -> SqlResult<Vec<(String, String, String)>> {
        let mut stmt = self.conn.prepare(
            "SELECT name, description, introduced_chapter FROM plot_thread
             WHERE status != 'resolved' ORDER BY priority DESC",
        )?;
        let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?;
        rows.collect()
    }

    pub fn resolve_plot_thread(&self, name: &str, chapter: &str) -> SqlResult<()> {
        self.conn.execute(
            "UPDATE plot_thread SET status = 'resolved', resolved_chapter = ?2
             WHERE name = ?1",
            rusqlite::params![name, chapter],
        )?;
        Ok(())
    }

    // ---- world_rule ----

    pub fn add_world_rule(&self, rule: &str, category: &str, priority: i32) -> SqlResult<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO world_rule (rule, category, priority) VALUES (?1, ?2, ?3)",
            rusqlite::params![rule, category, priority],
        )?;
        Ok(())
    }

    pub fn check_world_rules(&self, statement: &str) -> SqlResult<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT rule FROM world_rule WHERE active = 1")?;
        let rules: Vec<String> = stmt
            .query_map([], |row| row.get(0))?
            .filter_map(|r| r.ok())
            .collect();

        let violations: Vec<String> = rules
            .into_iter()
            .filter(|rule| {
                if let Some(negated) = rule.strip_prefix("没有") {
                    statement.contains(negated)
                } else if let Some(forbidden) = rule.strip_prefix("禁止") {
                    statement.contains(forbidden)
                } else {
                    false
                }
            })
            .collect();

        Ok(violations)
    }

    // ---- session_search ----

    pub fn search_sessions(
        &self,
        query: &str,
        limit: usize,
    ) -> SqlResult<Vec<SessionSearchResult>> {
        if query.trim().is_empty() {
            return Ok(Vec::new());
        }

        let mut stmt = self.conn.prepare(
            "SELECT role, content, created_at FROM session_history_fts
             WHERE session_history_fts MATCH ?1
             ORDER BY bm25(session_history_fts)
             LIMIT ?2",
        )?;
        let rows = stmt.query_map(rusqlite::params![query.trim(), limit as i64], |row| {
            Ok(SessionSearchResult {
                role: row.get(0)?,
                content: row.get(1)?,
                created_at: row.get(2)?,
            })
        })?;
        rows.collect()
    }
}
