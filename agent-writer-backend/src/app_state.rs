use std::sync::{Mutex, MutexGuard};

use agent_harness_core::hermes_memory::HermesDB;
use tokio_util::sync::CancellationToken;

use crate::{manual_agent::ManualAgentHistory, storage, writer_agent};

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub(crate) enum HarnessState {
    Idle,
}

pub(crate) struct AppState {
    pub(crate) harness_state: Mutex<HarnessState>,
    pub(crate) hermes_db: Mutex<HermesDB>,
    pub(crate) editor_prediction: Mutex<Option<EditorPredictionTask>>,
    pub(crate) writer_kernel: Mutex<writer_agent::WriterAgentKernel>,
    pub(crate) manual_agent_history: Mutex<ManualAgentHistory>,
    pub(crate) current_sprint: Mutex<Option<writer_agent::supervised_sprint::SupervisedSprintPlan>>,
}

impl AppState {
    pub(crate) fn open(app: &tauri::AppHandle) -> Result<Self, String> {
        let hermes_db = open_app_hermes_db(app)?;
        let writer_kernel = open_app_writer_kernel(app)?;
        let current_sprint = writer_kernel
            .memory
            .get_latest_active_supervised_sprint(&writer_kernel.project_id)
            .map_err(|e| format!("Failed to restore supervised sprint: {}", e))?;
        Ok(Self {
            harness_state: Mutex::new(HarnessState::Idle),
            hermes_db: Mutex::new(hermes_db),
            editor_prediction: Mutex::new(None),
            writer_kernel: Mutex::new(writer_kernel),
            manual_agent_history: Mutex::new(ManualAgentHistory::default()),
            current_sprint: Mutex::new(current_sprint),
        })
    }
}

pub(crate) struct EditorPredictionTask {
    pub(crate) request_id: String,
    pub(crate) cancel: CancellationToken,
}

pub(crate) fn lock_hermes<'a>(state: &'a AppState) -> Result<MutexGuard<'a, HermesDB>, String> {
    state
        .hermes_db
        .lock()
        .map_err(|_| "Hermes memory lock poisoned".to_string())
}

pub(crate) fn lock_harness_state<'a>(
    state: &'a AppState,
) -> Result<MutexGuard<'a, HarnessState>, String> {
    state
        .harness_state
        .lock()
        .map_err(|_| "Harness state lock poisoned".to_string())
}

pub(crate) fn lock_editor_prediction<'a>(
    state: &'a AppState,
) -> Result<MutexGuard<'a, Option<EditorPredictionTask>>, String> {
    state
        .editor_prediction
        .lock()
        .map_err(|_| "Editor prediction lock poisoned".to_string())
}

fn legacy_workspace_db_path(filename: &str) -> Option<std::path::PathBuf> {
    std::env::current_dir().ok().map(|dir| dir.join(filename))
}

pub(crate) fn migrate_legacy_db_if_needed(
    target_path: &std::path::Path,
    legacy_path: Option<std::path::PathBuf>,
) -> Result<(), String> {
    if target_path.exists() {
        return Ok(());
    }

    let Some(legacy_path) = legacy_path else {
        return Ok(());
    };
    if legacy_path == target_path || !legacy_path.exists() {
        return Ok(());
    }

    if let Some(parent) = target_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            format!(
                "Failed to create memory DB directory '{}': {}",
                parent.display(),
                e
            )
        })?;
    }

    std::fs::copy(&legacy_path, target_path).map_err(|e| {
        format!(
            "Failed to migrate memory DB from '{}' to '{}': {}",
            legacy_path.display(),
            target_path.display(),
            e
        )
    })?;
    Ok(())
}

fn active_project_db_path(
    app: &tauri::AppHandle,
    filename: &str,
) -> Result<std::path::PathBuf, String> {
    Ok(storage::active_project_data_dir(app)?.join(filename))
}

fn open_app_hermes_db(app: &tauri::AppHandle) -> Result<HermesDB, String> {
    let path = active_project_db_path(app, storage::HERMES_DB_FILENAME)?;
    let app_data_legacy = storage::app_data_dir(app)?.join(storage::HERMES_DB_FILENAME);
    migrate_legacy_db_if_needed(&path, Some(app_data_legacy))?;
    migrate_legacy_db_if_needed(&path, legacy_workspace_db_path(storage::HERMES_DB_FILENAME))?;
    HermesDB::open(&path).map_err(|e| {
        format!(
            "Failed to open Hermes memory DB at '{}': {}",
            path.display(),
            e
        )
    })
}

fn open_app_writer_kernel(
    app: &tauri::AppHandle,
) -> Result<writer_agent::WriterAgentKernel, String> {
    let project_id = storage::active_project_id(app)?;
    let path = active_project_db_path(app, storage::WRITER_MEMORY_DB_FILENAME)?;
    let app_data_legacy = storage::app_data_dir(app)?.join(storage::WRITER_MEMORY_DB_FILENAME);
    migrate_legacy_db_if_needed(&path, Some(app_data_legacy))?;
    migrate_legacy_db_if_needed(
        &path,
        legacy_workspace_db_path(storage::WRITER_MEMORY_DB_FILENAME),
    )?;
    let memory = writer_agent::memory::WriterMemory::open(&path).map_err(|e| {
        format!(
            "Failed to open writer memory DB at '{}': {}",
            path.display(),
            e
        )
    })?;
    seed_story_model_if_empty(app, &project_id, &memory);
    Ok(writer_agent::WriterAgentKernel::new(&project_id, memory))
}

fn seed_story_model_if_empty(
    app: &tauri::AppHandle,
    project_id: &str,
    memory: &writer_agent::memory::WriterMemory,
) {
    let project_name = storage::active_project_manifest(app)
        .map(|manifest| manifest.name)
        .unwrap_or_else(|_| "Local Project".to_string());
    let lorebook = storage::load_lorebook(app).unwrap_or_default();
    let outline = storage::load_outline(app).unwrap_or_default();
    match writer_agent::context::seed_story_contract_from_project_assets(
        project_id,
        &project_name,
        &lorebook,
        &outline,
        memory,
    ) {
        Ok(true) => tracing::info!("Seeded initial story contract for project {}", project_id),
        Ok(false) => {}
        Err(e) => tracing::warn!("Story contract seed skipped: {}", e),
    }
    match writer_agent::context::seed_chapter_missions_from_outline(project_id, &outline, memory) {
        Ok(count) if count > 0 => {
            tracing::info!(
                "Seeded {} chapter missions for project {}",
                count,
                project_id
            )
        }
        Ok(_) => {}
        Err(e) => tracing::warn!("Chapter mission seed skipped: {}", e),
    }
    match memory.ensure_default_book_state(project_id, &project_name) {
        Ok(true) => tracing::info!("Seeded default book state for project {}", project_id),
        Ok(false) => {}
        Err(e) => tracing::warn!("Book state seed skipped: {}", e),
    }
}

pub(crate) fn startup_error(message: String) -> Box<dyn std::error::Error> {
    tracing::error!("{}", message);
    std::io::Error::other(message).into()
}
