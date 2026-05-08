impl WriterMemory {
    pub fn upsert_canon_entity(
        &self,
        kind: &str,
        name: &str,
        aliases: &[String],
        summary: &str,
        attributes: &serde_json::Value,
        confidence: f64,
    ) -> rusqlite::Result<i64> {
        let aliases_json = serde_json::to_string(aliases).unwrap();
        let attrs_json = attributes.to_string();
        self.conn.execute(
            "INSERT INTO canon_entities (kind, name, aliases_json, summary, attributes_json, confidence, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, datetime('now'))
             ON CONFLICT(name) DO UPDATE SET
                kind=excluded.kind,
                aliases_json=excluded.aliases_json,
                summary=excluded.summary,
                attributes_json=excluded.attributes_json,
                confidence=excluded.confidence,
                updated_at=datetime('now')",
            rusqlite::params![kind, name, aliases_json, summary, attrs_json, confidence],
        )?;

        let entity_id: i64 = self.conn.query_row(
            "SELECT id FROM canon_entities WHERE name=?1",
            rusqlite::params![name],
            |row| row.get(0),
        )?;

        if let Some(map) = attributes.as_object() {
            for (key, value) in map {
                let fact_value = match value {
                    serde_json::Value::String(s) => s.clone(),
                    serde_json::Value::Null => continue,
                    other => other.to_string(),
                };
                self.conn.execute(
                    "DELETE FROM canon_facts WHERE entity_id=?1 AND key=?2",
                    rusqlite::params![entity_id, key],
                )?;
                self.conn.execute(
                    "INSERT OR IGNORE INTO canon_facts (entity_id, key, value, source_ref, confidence, status, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, 'active', datetime('now'))",
                    rusqlite::params![entity_id, key, fact_value, "canon_entity.attributes", confidence],
                )?;
            }
        }

        Ok(entity_id)
    }

    pub fn get_canon_facts_for_entity(
        &self,
        entity_name: &str,
    ) -> rusqlite::Result<Vec<(String, String)>> {
        let resolved_name = self
            .resolve_canon_entity_name(entity_name)?
            .unwrap_or_else(|| entity_name.to_string());
        let mut stmt = self.conn.prepare(
            "SELECT f.key, f.value FROM canon_facts f
             JOIN canon_entities e ON f.entity_id = e.id
             WHERE e.name = ?1 AND f.status = 'active'",
        )?;
        let rows = stmt.query_map(rusqlite::params![resolved_name], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })?;
        rows.collect()
    }

    pub fn update_canon_attribute(
        &self,
        entity_name: &str,
        attribute: &str,
        value: &str,
        confidence: f64,
    ) -> rusqlite::Result<()> {
        let Some(resolved_name) = self.resolve_canon_entity_name(entity_name)? else {
            return Err(rusqlite::Error::QueryReturnedNoRows);
        };

        let (entity_id, attributes_json): (i64, String) = self.conn.query_row(
            "SELECT id, attributes_json FROM canon_entities WHERE name=?1",
            rusqlite::params![resolved_name],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        let mut attributes =
            serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&attributes_json)
                .unwrap_or_default();
        attributes.insert(
            attribute.to_string(),
            serde_json::Value::String(value.to_string()),
        );
        self.conn.execute(
            "UPDATE canon_entities SET attributes_json=?1, confidence=?2, updated_at=datetime('now') WHERE id=?3",
            rusqlite::params![serde_json::Value::Object(attributes).to_string(), confidence, entity_id],
        )?;
        self.conn.execute(
            "DELETE FROM canon_facts WHERE entity_id=?1 AND key=?2",
            rusqlite::params![entity_id, attribute],
        )?;
        self.conn.execute(
            "INSERT OR IGNORE INTO canon_facts (entity_id, key, value, source_ref, confidence, status, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, 'active', datetime('now'))",
            rusqlite::params![entity_id, attribute, value, "canon.update_attribute", confidence],
        )?;
        Ok(())
    }

    pub fn resolve_canon_entity_name(&self, entity_name: &str) -> rusqlite::Result<Option<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT name, aliases_json FROM canon_entities ORDER BY length(name) DESC")?;
        let rows = stmt.query_map([], |row| {
            let name: String = row.get(0)?;
            let aliases_json: String = row.get(1)?;
            Ok((name, aliases_json))
        })?;

        for row in rows {
            let (name, aliases_json) = row?;
            if name == entity_name {
                return Ok(Some(name));
            }
            if let Ok(aliases) = serde_json::from_str::<Vec<String>>(&aliases_json) {
                if aliases.iter().any(|alias| alias == entity_name) {
                    return Ok(Some(name));
                }
            }
        }
        Ok(None)
    }

    pub fn get_canon_entity_names(&self) -> rusqlite::Result<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT name, aliases_json FROM canon_entities ORDER BY length(name) DESC")?;
        let rows = stmt.query_map([], |row| {
            let name: String = row.get(0)?;
            let aliases_json: String = row.get(1)?;
            Ok((name, aliases_json))
        })?;

        let mut names = Vec::new();
        for row in rows {
            let (name, aliases_json) = row?;
            if !name.trim().is_empty() {
                names.push(name);
            }
            if let Ok(aliases) = serde_json::from_str::<Vec<String>>(&aliases_json) {
                for alias in aliases {
                    if !alias.trim().is_empty() && !names.contains(&alias) {
                        names.push(alias);
                    }
                }
            }
        }
        Ok(names)
    }

    pub fn list_canon_entities(&self) -> rusqlite::Result<Vec<CanonEntitySummary>> {
        let mut stmt = self.conn.prepare(
            "SELECT kind, name, summary, attributes_json, confidence
             FROM canon_entities ORDER BY updated_at DESC, name ASC",
        )?;
        let rows = stmt.query_map([], |row| {
            let attributes_json: String = row.get(3)?;
            let attributes = serde_json::from_str(&attributes_json)
                .unwrap_or_else(|_| serde_json::Value::Object(serde_json::Map::new()));
            Ok(CanonEntitySummary {
                kind: row.get(0)?,
                name: row.get(1)?,
                summary: row.get(2)?,
                attributes,
                confidence: row.get(4)?,
            })
        })?;
        rows.collect()
    }

    pub fn upsert_canon_rule(
        &self,
        rule: &str,
        category: &str,
        priority: i32,
        source_ref: &str,
    ) -> rusqlite::Result<i64> {
        self.conn.execute(
            "INSERT INTO canon_rules (rule, category, priority, source_ref, status, updated_at)
             VALUES (?1, ?2, ?3, ?4, 'active', datetime('now'))
             ON CONFLICT(rule) DO UPDATE SET
                category=excluded.category,
                priority=excluded.priority,
                source_ref=excluded.source_ref,
                status='active',
                updated_at=datetime('now')",
            rusqlite::params![rule, category, priority, source_ref],
        )?;
        self.conn.query_row(
            "SELECT id FROM canon_rules WHERE rule=?1",
            rusqlite::params![rule],
            |row| row.get(0),
        )
    }

    pub fn list_canon_rules(&self, limit: usize) -> rusqlite::Result<Vec<CanonRuleSummary>> {
        let mut stmt = self.conn.prepare(
            "SELECT rule, category, priority, status
             FROM canon_rules
             WHERE status = 'active'
             ORDER BY priority DESC, updated_at DESC
             LIMIT ?1",
        )?;
        let rows = stmt.query_map(rusqlite::params![limit as i64], |row| {
            Ok(CanonRuleSummary {
                rule: row.get(0)?,
                category: row.get(1)?,
                priority: row.get(2)?,
                status: row.get(3)?,
            })
        })?;
        rows.collect()
    }

    // -- Plot Promises --


}
