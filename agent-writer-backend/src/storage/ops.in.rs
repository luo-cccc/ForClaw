fn diagnose_sqlite_database(
    label: &str,
    path: &std::path::Path,
    tables: &[&str],
) -> SqliteDatabaseDiagnostic {
    let mut diagnostic = SqliteDatabaseDiagnostic {
        label: label.to_string(),
        path: path.to_string_lossy().to_string(),
        exists: path.exists(),
        bytes: file_size(path),
        user_version: None,
        quick_check: None,
        table_counts: vec![],
        status: "unknown".to_string(),
        error: None,
    };
    if !diagnostic.exists {
        diagnostic.status = "missing".to_string();
        diagnostic.error = Some("Database file is missing".to_string());
        return diagnostic;
    }

    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY;
    let conn = match Connection::open_with_flags(path, flags) {
        Ok(conn) => conn,
        Err(e) => {
            diagnostic.status = "error".to_string();
            diagnostic.error = Some(format!("Open failed: {}", e));
            return diagnostic;
        }
    };

    diagnostic.user_version = conn
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .ok();
    match conn.query_row("PRAGMA quick_check", [], |row| row.get::<_, String>(0)) {
        Ok(result) => diagnostic.quick_check = Some(result),
        Err(e) => {
            diagnostic.status = "error".to_string();
            diagnostic.error = Some(format!("quick_check failed: {}", e));
            return diagnostic;
        }
    }

    for table in tables {
        match sqlite_table_row_count(&conn, table) {
            Ok(Some(rows)) => diagnostic.table_counts.push(SqliteTableCount {
                table: (*table).to_string(),
                rows,
            }),
            Ok(None) => {}
            Err(e) => {
                diagnostic.status = "error".to_string();
                diagnostic.error = Some(format!("Count failed for '{}': {}", table, e));
                return diagnostic;
            }
        }
    }

    if diagnostic.quick_check.as_deref() == Some("ok") {
        diagnostic.status = "ok".to_string();
    } else {
        diagnostic.status = "error".to_string();
        diagnostic.error = diagnostic
            .quick_check
            .as_ref()
            .map(|check| format!("SQLite quick_check reported '{}'", check));
    }
    diagnostic
}

fn sqlite_table_row_count(conn: &Connection, table: &str) -> rusqlite::Result<Option<u64>> {
    if !sqlite_table_exists(conn, table)? {
        return Ok(None);
    }
    let count: i64 = conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
        row.get(0)
    })?;
    Ok(Some(count.max(0) as u64))
}

fn sqlite_table_exists(conn: &Connection, table: &str) -> rusqlite::Result<bool> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
        rusqlite::params![table],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}

fn base_file_diagnostic(label: &str, path: &std::path::Path) -> StorageFileDiagnostic {
    StorageFileDiagnostic {
        label: label.to_string(),
        path: path.to_string_lossy().to_string(),
        exists: path.exists(),
        bytes: file_size(path),
        record_count: None,
        backup_count: backup_count(path),
        status: "unknown".to_string(),
        error: None,
    }
}

fn file_size(path: &std::path::Path) -> Option<u64> {
    std::fs::metadata(path).ok().map(|metadata| metadata.len())
}

fn unix_time_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

pub fn rename_chapter_file(
    app: &tauri::AppHandle,
    old_name: String,
    new_name: String,
) -> Result<(), String> {
    let dir = project_dir(app)?;
    let old_path = safe_chapter_file_path(&dir, &old_name)?;
    let new_path = safe_chapter_file_path(&dir, &new_name)?;
    if old_path.exists() && !new_path.exists() {
        std::fs::rename(&old_path, &new_path).map_err(|e| e.to_string())
    } else {
        Ok(())
    }
}

fn safe_chapter_file_path(
    dir: &std::path::Path,
    filename: &str,
) -> Result<std::path::PathBuf, String> {
    let path = std::path::Path::new(filename);
    if path.components().count() != 1 || path.file_name().is_none() {
        return Err(format!("Invalid chapter filename: {}", filename));
    }
    if path.extension().map(|e| e != "md").unwrap_or(true) {
        return Err(format!("Chapter filename must end with .md: {}", filename));
    }
    Ok(dir.join(path))
}

pub fn atomic_write(path: &Path, content: &str) -> Result<(), String> {
    let _guard = acquire_write_guard(path)?;
    backup_existing_file(path)?;
    let tmp = unique_atomic_tmp_path(path)?;
    std::fs::write(&tmp, content).map_err(|e| format!("Write tmp failed: {}", e))?;
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("Atomic rename failed: {}", e));
    }
    Ok(())
}

struct FileWriteGuard {
    path: PathBuf,
}

impl Drop for FileWriteGuard {
    fn drop(&mut self) {
        let (mutex, cvar) = write_locks();
        if let Ok(mut active) = mutex.lock() {
            active.remove(&self.path);
            cvar.notify_all();
        }
    }
}

fn write_locks() -> &'static (Mutex<HashSet<PathBuf>>, Condvar) {
    ACTIVE_WRITE_LOCKS.get_or_init(|| (Mutex::new(HashSet::new()), Condvar::new()))
}

fn acquire_write_guard(path: &Path) -> Result<FileWriteGuard, String> {
    let key = write_lock_key(path);
    let (mutex, cvar) = write_locks();
    let mut active = mutex
        .lock()
        .map_err(|e| format!("Storage write lock poisoned: {}", e))?;
    while active.contains(&key) {
        active = cvar
            .wait(active)
            .map_err(|e| format!("Storage write lock poisoned: {}", e))?;
    }
    active.insert(key.clone());
    Ok(FileWriteGuard { path: key })
}

fn write_lock_key(path: &Path) -> PathBuf {
    if let Ok(canonical) = path.canonicalize() {
        return canonical;
    }
    if let (Some(parent), Some(file_name)) = (path.parent(), path.file_name()) {
        if let Ok(parent) = parent.canonicalize() {
            return parent.join(file_name);
        }
    }
    path.to_path_buf()
}

fn unique_atomic_tmp_path(path: &Path) -> Result<PathBuf, String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("Path '{}' has no parent directory", path.display()))?;
    let file_name = path
        .file_name()
        .ok_or_else(|| format!("Path '{}' has no filename", path.display()))?
        .to_string_lossy();
    for _ in 0..100 {
        let sequence = ATOMIC_WRITE_TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let tmp_name = format!(
            ".{}.{}.{}.{}.tmp",
            file_name,
            std::process::id(),
            unix_time_ms(),
            sequence
        );
        let tmp = parent.join(tmp_name);
        if !tmp.exists() {
            return Ok(tmp);
        }
    }
    Err(format!(
        "Could not allocate a unique tmp file for '{}'",
        path.display()
    ))
}

fn backup_existing_file(path: &Path) -> Result<(), String> {
    if !path.exists() || path.is_dir() {
        return Ok(());
    }

    let backup_dir = backup_dir_for(path)?;
    std::fs::create_dir_all(&backup_dir).map_err(|e| {
        format!(
            "Failed to create backup dir '{}': {}",
            backup_dir.display(),
            e
        )
    })?;
    let filename = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    let backup_path = backup_dir.join(format!("{}-{}", unix_time_ms(), filename));
    std::fs::copy(path, &backup_path).map_err(|e| {
        format!(
            "Failed to backup '{}' to '{}': {}",
            path.display(),
            backup_path.display(),
            e
        )
    })?;
    prune_backups(&backup_dir, MAX_FILE_BACKUPS)
}

fn backup_count(path: &std::path::Path) -> usize {
    let Ok(dir) = backup_dir_for(path) else {
        return 0;
    };
    std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .filter(|entry| entry.path().is_file())
                .count()
        })
        .unwrap_or(0)
}

fn backup_dir_for(path: &std::path::Path) -> Result<std::path::PathBuf, String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("Path '{}' has no parent directory", path.display()))?;
    let file_stem = path
        .file_name()
        .ok_or_else(|| format!("Path '{}' has no filename", path.display()))?
        .to_string_lossy()
        .to_string();
    Ok(parent
        .join(".backups")
        .join(safe_backup_segment(&file_stem)))
}

fn prune_backups(dir: &std::path::Path, keep: usize) -> Result<(), String> {
    let mut entries = std::fs::read_dir(dir)
        .map_err(|e| format!("Failed to read backup dir '{}': {}", dir.display(), e))?
        .flatten()
        .filter(|entry| entry.path().is_file())
        .filter_map(|entry| {
            let modified = entry.metadata().ok()?.modified().ok()?;
            Some((modified, entry.path()))
        })
        .collect::<Vec<_>>();
    entries.sort_by_key(|b| std::cmp::Reverse(b.0));
    for (_, path) in entries.into_iter().skip(keep) {
        std::fs::remove_file(&path)
            .map_err(|e| format!("Failed to prune backup '{}': {}", path.display(), e))?;
    }
    Ok(())
}

fn safe_backup_segment(name: &str) -> String {
    name.chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.' {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

fn safe_backup_file_path(
    backup_dir: &std::path::Path,
    backup_id: &str,
) -> Result<std::path::PathBuf, String> {
    let path = std::path::Path::new(backup_id);
    if path.components().count() != 1 || path.file_name().is_none() {
        return Err(format!("Invalid backup id: {}", backup_id));
    }
    Ok(backup_dir.join(path))
}

fn backup_info(entry: std::fs::DirEntry) -> Result<FileBackupInfo, String> {
    let path = entry.path();
    let metadata = entry
        .metadata()
        .map_err(|e| format!("Failed to read backup metadata '{}': {}", path.display(), e))?;
    let modified_at = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0);
    let filename = entry.file_name().to_string_lossy().to_string();
    Ok(FileBackupInfo {
        id: filename.clone(),
        filename,
        path: path.to_string_lossy().to_string(),
        bytes: metadata.len(),
        modified_at,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_local_project_id_is_stable_and_path_safe() {
        let id = generated_local_project_id(std::path::Path::new("C:/Users/Msi/AppData/Forge"));

        assert!(id.starts_with("local-"));
        assert!(valid_project_id(&id));
        assert_eq!(
            id,
            generated_local_project_id(std::path::Path::new("C:/Users/Msi/AppData/Forge"))
        );
    }

    #[test]
    fn project_id_validation_rejects_path_traversal() {
        assert!(valid_project_id("local-abc_123"));
        assert!(!valid_project_id(""));
        assert!(!valid_project_id("../novel"));
        assert!(!valid_project_id("novel/one"));
        assert!(!valid_project_id("novel one"));
    }

    #[test]
    fn parse_json_list_reports_corrupt_lorebook() {
        let path = std::path::Path::new("lorebook.json");
        let err = parse_json_list::<LoreEntry>("{not json", path, "lorebook").unwrap_err();

        assert!(err.contains("Failed to parse lorebook"));
        assert!(err.contains("lorebook.json"));
    }

    #[test]
    fn parse_json_list_accepts_valid_outline() {
        let path = std::path::Path::new("outline.json");
        let nodes = parse_json_list::<OutlineNode>(
            r#"[{"chapter_title":"第一章","summary":"开端","status":"draft"}]"#,
            path,
            "outline",
        )
        .unwrap();

        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].chapter_title, "第一章");
    }

    #[test]
    fn patch_outline_node_updates_allowed_fields() {
        let mut nodes = vec![OutlineNode {
            chapter_title: "第一章".to_string(),
            summary: "旧".to_string(),
            status: "draft".to_string(),
        }];
        let patch = serde_json::json!({"summary": "新", "status": "done"});

        apply_outline_patch(&mut nodes, "第一章", &patch).unwrap();

        assert_eq!(nodes[0].summary, "新");
        assert_eq!(nodes[0].status, "done");
    }

    #[test]
    fn patch_outline_node_rejects_unknown_fields() {
        let mut nodes = vec![OutlineNode {
            chapter_title: "第一章".to_string(),
            summary: "旧".to_string(),
            status: "draft".to_string(),
        }];

        let err = apply_outline_patch(&mut nodes, "第一章", &serde_json::json!({"notes": "x"}))
            .unwrap_err();
        assert!(err.contains("Unsupported outline patch field"));
    }

    #[test]
    fn chapter_filename_rejects_path_segments() {
        assert_eq!(
            chapter_filename("../第一章: 开端/草稿"),
            "第一章-开端草稿.md"
        );
        assert_eq!(chapter_filename("   "), "untitled.md");
        assert_eq!(
            chapter_filename("Chapter 01 - Setup"),
            "chapter-01-setup.md"
        );
    }

    #[test]
    fn safe_chapter_file_path_rejects_traversal() {
        let dir = std::path::Path::new("chapters");

        assert!(safe_chapter_file_path(dir, "chapter-1.md").is_ok());
        assert!(safe_chapter_file_path(dir, "../chapter-1.md").is_err());
        assert!(safe_chapter_file_path(dir, "nested/chapter-1.md").is_err());
        assert!(safe_chapter_file_path(dir, "chapter-1.txt").is_err());
    }

    #[test]
    fn safe_backup_file_path_rejects_traversal() {
        let dir = std::path::Path::new("backups");

        assert!(safe_backup_file_path(dir, "123-chapter.md").is_ok());
        assert!(safe_backup_file_path(dir, "../123-chapter.md").is_err());
        assert!(safe_backup_file_path(dir, "nested/123-chapter.md").is_err());
    }

    #[test]
    fn diagnose_json_array_file_reports_parse_errors() {
        let path = temp_path("bad-lorebook.json");
        std::fs::write(&path, "{not json").unwrap();

        let diagnostic = diagnose_json_array_file::<LoreEntry>("lorebook", &path);

        assert_eq!(diagnostic.status, "error");
        assert!(diagnostic.error.unwrap().contains("JSON parse failed"));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn diagnose_sqlite_database_reports_version_and_counts() {
        let path = temp_path("writer-memory.db");
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(
                "PRAGMA user_version = 7;
                CREATE TABLE canon_entities (id INTEGER PRIMARY KEY, name TEXT);
                INSERT INTO canon_entities (name) VALUES ('林墨'), ('张三');",
            )
            .unwrap();
        }

        let diagnostic =
            diagnose_sqlite_database("writer_memory", &path, &["canon_entities", "missing_table"]);

        assert_eq!(diagnostic.status, "ok");
        assert_eq!(diagnostic.user_version, Some(7));
        assert_eq!(diagnostic.quick_check.as_deref(), Some("ok"));
        assert_eq!(diagnostic.table_counts[0].table, "canon_entities");
        assert_eq!(diagnostic.table_counts[0].rows, 2);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn atomic_write_creates_bounded_backups_for_existing_files() {
        let path = temp_path("chapter.md");
        std::fs::write(&path, "old").unwrap();

        atomic_write(&path, "new").unwrap();

        assert_eq!(std::fs::read_to_string(&path).unwrap(), "new");
        assert!(!path.with_extension("tmp").exists());
        let backup_dir = backup_dir_for(&path).unwrap();
        let backups = std::fs::read_dir(&backup_dir)
            .unwrap()
            .flatten()
            .collect::<Vec<_>>();
        assert_eq!(backups.len(), 1);
        assert_eq!(std::fs::read_to_string(backups[0].path()).unwrap(), "old");
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir_all(backup_dir);
    }

    #[test]
    fn atomic_write_serializes_concurrent_writes_to_same_target() {
        let path = temp_path("concurrent-chapter.md");
        std::fs::write(&path, "base").unwrap();

        let handles = (0..8)
            .map(|i| {
                let path = path.clone();
                std::thread::spawn(move || {
                    atomic_write(&path, &format!("content-{i}")).unwrap();
                })
            })
            .collect::<Vec<_>>();

        for handle in handles {
            handle.join().unwrap();
        }

        let final_content = std::fs::read_to_string(&path).unwrap();
        assert!(final_content.starts_with("content-"));
        assert!(!path.with_extension("tmp").exists());
        let parent = path.parent().unwrap();
        let leaked_tmp = std::fs::read_dir(parent).unwrap().flatten().any(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .contains("concurrent-chapter.md")
                && entry
                    .path()
                    .extension()
                    .map(|ext| ext == "tmp")
                    .unwrap_or(false)
        });
        assert!(!leaked_tmp);
        let backup_dir = backup_dir_for(&path).unwrap();
        let backup_count = std::fs::read_dir(&backup_dir).unwrap().flatten().count();
        assert!(backup_count <= MAX_FILE_BACKUPS);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir_all(backup_dir);
    }

    #[test]
    fn backup_info_lists_safe_restore_candidates() {
        let path = temp_path("outline.json");
        std::fs::write(&path, "v1").unwrap();
        atomic_write(&path, "v2").unwrap();
        let dir = backup_dir_for(&path).unwrap();
        let backups = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .filter_map(|entry| backup_info(entry).ok())
            .collect::<Vec<_>>();

        assert_eq!(backups.len(), 1);
        assert!(backups[0].id.ends_with("outline.json"));
        assert_eq!(backups[0].bytes, 2);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn validate_backup_content_rejects_corrupt_json_targets() {
        let path = std::path::Path::new("outline.json");

        assert!(validate_backup_content(&BackupTarget::Outline, "{bad", path).is_err());
        assert!(validate_backup_content(
            &BackupTarget::Chapter {
                title: "Chapter-1".to_string(),
            },
            "{bad",
            path,
        )
        .is_ok());
    }

    #[test]
    fn prune_backups_keeps_newest_entries() {
        let dir = std::env::temp_dir().join(format!(
            "forge-storage-test-{}-{}-backups",
            std::process::id(),
            unix_time_ms()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        for i in 0..5 {
            std::fs::write(dir.join(format!("{}.txt", i)), i.to_string()).unwrap();
        }

        prune_backups(&dir, 2).unwrap();

        let count = std::fs::read_dir(&dir).unwrap().flatten().count();
        assert_eq!(count, 2);
        let _ = std::fs::remove_dir_all(dir);
    }

    fn temp_path(name: &str) -> std::path::PathBuf {
        let unique = unix_time_ms();
        let path = std::env::temp_dir().join(format!(
            "forge-storage-test-{}-{}-{}",
            std::process::id(),
            unique,
            name
        ));
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        path
    }
}
