
pub const HERMES_DB_FILENAME: &str = "hermes_memory.db";
pub const WRITER_MEMORY_DB_FILENAME: &str = "writer_memory.db";
const MAX_FILE_BACKUPS: usize = 20;
static ACTIVE_WRITE_LOCKS: OnceLock<(Mutex<HashSet<PathBuf>>, Condvar)> = OnceLock::new();
static ATOMIC_WRITE_TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

const HERMES_DIAGNOSTIC_TABLES: &[&str] = &[
    "session_history",
    "user_drift_profile",
    "hierarchical_summaries",
    "agent_skills",
    "character_state",
    "plot_thread",
    "world_rule",
];

const WRITER_MEMORY_DIAGNOSTIC_TABLES: &[&str] = &[
    "story_contracts",
    "chapter_missions",
    "chapter_result_snapshots",
    "canon_entities",
    "canon_facts",
    "canon_rules",
    "plot_promises",
    "style_preferences",
    "creative_decisions",
    "proposal_feedback",
    "memory_audit_events",
    "manual_agent_turns",
    "writer_observation_trace",
    "writer_proposal_trace",
    "writer_feedback_trace",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoreEntry {
    pub id: String,
    pub keyword: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChapterInfo {
    pub title: String,
    pub filename: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutlineNode {
    pub chapter_title: String,
    pub summary: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectManifest {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectStorageDiagnostics {
    pub project_id: String,
    pub project_name: String,
    pub app_data_dir: String,
    pub project_data_dir: String,
    pub checked_at: u64,
    pub healthy: bool,
    pub files: Vec<StorageFileDiagnostic>,
    pub databases: Vec<SqliteDatabaseDiagnostic>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageFileDiagnostic {
    pub label: String,
    pub path: String,
    pub exists: bool,
    pub bytes: Option<u64>,
    pub record_count: Option<usize>,
    pub backup_count: usize,
    pub status: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SqliteDatabaseDiagnostic {
    pub label: String,
    pub path: String,
    pub exists: bool,
    pub bytes: Option<u64>,
    pub user_version: Option<i64>,
    pub quick_check: Option<String>,
    pub table_counts: Vec<SqliteTableCount>,
    pub status: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SqliteTableCount {
    pub table: String,
    pub rows: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BackupTarget {
    Lorebook,
    Outline,
    ProjectBrain,
    Chapter { title: String },
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileBackupInfo {
    pub id: String,
    pub filename: String,
    pub path: String,
    pub bytes: u64,
    pub modified_at: u64,
}
