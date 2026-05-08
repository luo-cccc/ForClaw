use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Condvar, Mutex, OnceLock};

include!("storage/types.in.rs");

pub fn chapter_filename(title: &str) -> String {
    let mut safe = String::new();
    let mut last_was_dash = false;
    for ch in title.trim().to_lowercase().chars() {
        let next = if ch.is_ascii_alphanumeric() || is_cjk(ch) {
            Some(ch)
        } else if ch == ' ' || ch == '-' || ch == '_' {
            Some('-')
        } else {
            None
        };

        if let Some(ch) = next {
            if ch == '-' {
                if last_was_dash {
                    continue;
                }
                last_was_dash = true;
            } else {
                last_was_dash = false;
            }
            safe.push(ch);
        }
    }

    let safe = safe.trim_matches('-');
    let stem = if safe.is_empty() { "untitled" } else { safe };
    format!("{}.md", stem)
}

fn is_cjk(ch: char) -> bool {
    matches!(
        ch as u32,
        0x3400..=0x4DBF
            | 0x4E00..=0x9FFF
            | 0xF900..=0xFAFF
            | 0x20000..=0x2A6DF
            | 0x2A700..=0x2B73F
            | 0x2B740..=0x2B81F
            | 0x2B820..=0x2CEAF
    )
}

pub fn content_revision(content: &str) -> String {
    const FNV_OFFSET: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;

    let mut hash = FNV_OFFSET;
    for byte in content.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    format!("{:016x}-{}", hash, content.len())
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

fn backup_dir_for(path: &Path) -> Result<PathBuf, String> {
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

fn prune_backups(dir: &Path, keep: usize) -> Result<(), String> {
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

fn unix_time_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn atomic_write_creates_bounded_backups_for_existing_files() {
        let path = temp_path("chapter.md");
        std::fs::write(&path, "old").unwrap();

        atomic_write(&path, "new").unwrap();

        assert_eq!(std::fs::read_to_string(&path).unwrap(), "new");
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

    fn temp_path(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "forge-storage-test-{}-{}-{}",
            std::process::id(),
            unix_time_ms(),
            name
        ));
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        path
    }
}
