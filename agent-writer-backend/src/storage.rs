use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Condvar, Mutex, OnceLock};
use tauri::Manager;

include!("storage/types.in.rs");
include!("storage/project.in.rs");
include!("storage/content.in.rs");
include!("storage/diagnostics.in.rs");
include!("storage/ops.in.rs");
