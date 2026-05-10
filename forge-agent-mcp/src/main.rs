use std::io::Write;
use std::process::ExitCode;

use agent_writer_lib::headless::HeadlessBackend;
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, BufReader};

mod dispatch;
mod tools;

use dispatch::call_tool;
use tools::tools;

const SERVER_NAME: &str = "forge-writer-agent";
const SERVER_TITLE: &str = "Forge Writer Agent";
const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");
const PROTOCOL_VERSION: &str = "2025-11-25";
const SUPPORTED_PROTOCOL_VERSIONS: &[&str] =
    &["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];
const BACKEND_ACTIONS: &[&str] = &[
    "manifest",
    "project_manifest",
    "project_paths",
    "status",
    "agent_tools",
    "effective_agent_tool_inventory",
    "agent_kernel_status",
    "agent_domain_profile",
    "list_chapters",
    "create_chapter",
    "load_chapter",
    "save_chapter",
    "chapter_revision",
    "rename_chapter_file",
    "load_lorebook",
    "save_lore_entry",
    "delete_lore_entry",
    "load_outline",
    "save_outline_node",
    "delete_outline_node",
    "update_outline_status",
    "reorder_outline_nodes",
    "list_volumes",
    "save_volume",
    "delete_volume",
    "get_volume_snapshot",
    "save_volume_snapshot",
    "get_book_state",
    "save_book_state",
    "observe",
    "ledger",
    "today_five",
    "pending_proposals",
    "story_review_queue",
    "story_debt",
    "reader_compensation_review_chain",
    "trace",
    "inspector_timeline",
    "companion_timeline_summary",
    "apply_feedback",
    "record_implicit_ghost_rejection",
    "approve_writer_operation",
    "record_writer_operation_durable_save",
    "ambient_entity_hints",
    "ask_agent",
    "batch_generate_chapter",
    "generate_chapter_autonomous",
    "repair_chapter_state",
    "analyze_chapter",
    "generate_parallel_drafts",
    "analyze_pacing",
    "ask_project_brain",
    "get_project_brain_knowledge_graph",
    "compare_project_brain_source_revisions",
    "restore_project_brain_source_revision",
    "cross_reference_brain_nodes",
    "ingest_external_research",
    "run_metacognitive_recovery",
    "start_sprint",
    "sprint_plan",
    "sprint_progress",
    "pause_sprint",
    "resume_sprint",
    "cancel_sprint",
    "checkpoint_sprint",
    "record_sprint_budget_usage",
    "set_sprint_quality_gate",
    "latest_chapter_generation_checkpoint",
    "chapter_generation_resume_candidates",
    "resume_chapter_generation",
    "craft_library",
    "craft_memory_stats",
    "eval_trend_summary",
    "record_manual_craft_edit_feedback",
    "chapter_quality_report",
    "context_quality_report",
    "budget_calibration",
    "execution_plan",
    "project_graph_data",
    "project_storage_diagnostics",
    "export_writer_agent_trajectory",
    "export_diagnostic_logs",
    "list_file_backups",
    "restore_file_backup",
    "set_api_key",
    "check_api_key",
    "list_world_assets",
    "approve_world_asset",
    "reject_world_asset",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ErrorKind {
    Backend,
    Validation,
    Provider,
    Permission,
    Budget,
    ContextOverflow,
    Storage,
}

#[derive(Debug, Deserialize)]
struct JsonRpcMessage {
    #[serde(default)]
    jsonrpc: Option<String>,
    #[serde(default)]
    id: Option<Value>,
    #[serde(default)]
    method: Option<String>,
    #[serde(default)]
    params: Value,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ToolCallParams {
    name: String,
    #[serde(default)]
    arguments: Value,
}

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(std::env::var("FORGE_AGENT_LOG").unwrap_or_else(|_| {
            "forge_agent_mcp=warn,agent_writer=warn,agent_harness_core=warn".to_string()
        }))
        .with_writer(std::io::stderr)
        .with_target(false)
        .init();

    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        None | Some("stdio") => {}
        Some("--version") | Some("-V") => {
            println!("{} {}", SERVER_NAME, SERVER_VERSION);
            return ExitCode::SUCCESS;
        }
        Some("--help") | Some("-h") => {
            print_help();
            return ExitCode::SUCCESS;
        }
        Some(other) => {
            eprintln!("Unknown command '{other}'. Use 'stdio', '--help', or '--version'.");
            return ExitCode::from(2);
        }
    }

    if let Err(error) = run_stdio().await {
        tracing::error!("{}", error);
        return ExitCode::from(1);
    }

    ExitCode::SUCCESS
}

fn print_help() {
    println!(
        "{SERVER_TITLE} MCP server {SERVER_VERSION}

USAGE:
    forge-agent-mcp [stdio]

ENV:
    FORGE_AGENT_DATA_DIR       Data directory for the headless project.
    FORGE_AGENT_PROJECT_ID     Optional stable project id.
    FORGE_AGENT_PROJECT_NAME   Optional display name for new projects.
    FORGE_AGENT_LOG            tracing filter; logs always go to stderr.
    OPENAI_API_KEY             Provider key for model-backed tools.
    OPENAI_API_BASE            Optional OpenAI-compatible endpoint.
    OPENAI_MODEL               Optional model override.

STDIO:
    Reads newline-delimited JSON-RPC 2.0 messages from stdin and writes one
    newline-delimited JSON-RPC response to stdout for each request."
    );
}

async fn run_stdio() -> Result<(), String> {
    dotenvy::dotenv().ok();
    let backend = HeadlessBackend::open(HeadlessBackend::default_config()?)?;
    let stdin = tokio::io::stdin();
    let mut lines = BufReader::new(stdin).lines();

    while let Some(line) = lines
        .next_line()
        .await
        .map_err(|error| format!("stdin read failed: {}", error))?
    {
        if line.trim().is_empty() {
            continue;
        }

        let parsed = serde_json::from_str::<Value>(&line);
        let response = match parsed {
            Ok(Value::Array(messages)) => {
                let mut responses = Vec::new();
                for message in messages {
                    if let Some(response) = handle_raw_message(&backend, message).await {
                        responses.push(response);
                    }
                }
                if responses.is_empty() {
                    None
                } else {
                    Some(Value::Array(responses))
                }
            }
            Ok(message) => handle_raw_message(&backend, message).await,
            Err(error) => Some(error_response(
                None,
                -32700,
                format!("Parse error: {}", error),
                None,
            )),
        };

        if let Some(response) = response {
            write_message(&response)?;
        }
    }

    Ok(())
}

async fn handle_raw_message(backend: &HeadlessBackend, value: Value) -> Option<Value> {
    match serde_json::from_value::<JsonRpcMessage>(value) {
        Ok(message) => handle_message(backend, message).await,
        Err(error) => Some(error_response(
            None,
            -32600,
            "Invalid Request".to_string(),
            Some(json!({ "reason": error.to_string() })),
        )),
    }
}

async fn handle_message(backend: &HeadlessBackend, message: JsonRpcMessage) -> Option<Value> {
    if message.jsonrpc.as_deref() != Some("2.0") {
        return message.id.map(|id| {
            error_response(
                Some(id),
                -32600,
                "Invalid Request: jsonrpc must be \"2.0\"".to_string(),
                None,
            )
        });
    }

    let Some(method) = message.method.as_deref() else {
        return message.id.map(|id| {
            error_response(
                Some(id),
                -32600,
                "Invalid JSON-RPC message: missing method".to_string(),
                None,
            )
        });
    };

    match method {
        "initialize" => message
            .id
            .map(|id| success_response(id, initialize_result(&message.params))),
        "notifications/initialized" => None,
        "ping" => message.id.map(|id| success_response(id, json!({}))),
        "tools/list" => message
            .id
            .map(|id| success_response(id, json!({ "tools": tools() }))),
        "tools/call" => {
            if let Some(id) = message.id {
                match call_tool(backend, message.params).await {
                    Ok(result) => Some(success_response(id, result)),
                    Err(classified) => Some(success_response(
                        id,
                        tool_error_result(classified.kind, classified.message),
                    )),
                }
            } else {
                None
            }
        }
        _ => message.id.map(|id| {
            error_response(
                Some(id),
                -32601,
                format!("Method not found: {}", method),
                None,
            )
        }),
    }
}

fn initialize_result(params: &Value) -> Value {
    let requested = params
        .get("protocolVersion")
        .and_then(Value::as_str)
        .unwrap_or(PROTOCOL_VERSION);
    let protocol_version = if SUPPORTED_PROTOCOL_VERSIONS.contains(&requested) {
        requested
    } else {
        PROTOCOL_VERSION
    };

    json!({
        "protocolVersion": protocol_version,
        "capabilities": {
            "tools": {
                "listChanged": false
            }
        },
        "serverInfo": {
            "name": SERVER_NAME,
            "title": SERVER_TITLE,
            "version": SERVER_VERSION,
            "description": "Headless Forge writer-agent backend exposed through MCP."
        },
        "_meta": {
            "forge": server_manifest()
        },
        "instructions": "Use Forge tools for long-form fiction project memory, chapters, observations, proposals, ledgers, and supervised sprint state."
    })
}

pub(crate) fn server_manifest() -> Value {
    json!({
        "serverName": SERVER_NAME,
        "serverTitle": SERVER_TITLE,
        "serverVersion": SERVER_VERSION,
        "protocolVersion": PROTOCOL_VERSION,
        "supportedProtocolVersions": SUPPORTED_PROTOCOL_VERSIONS,
        "transport": "stdio-jsonrpc-lines",
        "backend": {
            "mode": "headless",
            "ui": false,
            "uiRuntime": false,
            "uiEvents": false,
            "capabilityPolicy": "backend capabilities are preserved; UI runtime and UI-only hooks are intentionally excluded"
        },
        "actions": BACKEND_ACTIONS,
        "stableEntrypoints": [
            "forge_backend_call",
            "forge_manifest",
            "forge_status",
            "forge_ask_agent",
            "forge_generate_chapter_autonomous",
            "forge_repair_chapter_state"
        ]
    })
}

pub(crate) fn tool_result(value: Value, is_error: bool) -> Value {
    let text = serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string());
    let structured_content = json!({
        "ok": !is_error,
        "data": value,
        "error": Value::Null
    });
    json!({
        "content": [
            {
                "type": "text",
                "text": text
            }
        ],
        "structuredContent": structured_content,
        "isError": is_error
    })
}

pub(crate) fn tool_error_result(kind: ErrorKind, message: String) -> Value {
    let structured_content = json!({
        "ok": false,
        "data": Value::Null,
        "error": {
            "kind": kind,
            "message": message,
        }
    });
    json!({
        "content": [
            {
                "type": "text",
                "text": structured_content["error"]["message"]
                    .as_str()
                    .unwrap_or("Backend error")
            }
        ],
        "structuredContent": structured_content,
        "isError": true
    })
}

fn success_response(id: Value, result: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result
    })
}

fn error_response(id: Option<Value>, code: i64, message: String, data: Option<Value>) -> Value {
    let mut error = json!({
        "code": code,
        "message": message
    });
    if let Some(data) = data {
        error["data"] = data;
    }
    json!({
        "jsonrpc": "2.0",
        "id": id.unwrap_or(Value::Null),
        "error": error
    })
}

fn write_message(message: &Value) -> Result<(), String> {
    let mut stdout = std::io::stdout().lock();
    serde_json::to_writer(&mut stdout, message).map_err(|error| error.to_string())?;
    stdout
        .write_all(b"\n")
        .map_err(|error| format!("stdout write failed: {}", error))?;
    stdout
        .flush()
        .map_err(|error| format!("stdout flush failed: {}", error))
}

#[cfg(test)]
mod tests {
    use super::dispatch::classify_error;
    use super::tools::{is_destructive_tool, is_open_world_tool, is_read_only_tool};
    use super::*;

    fn tool_names() -> Vec<String> {
        tools()
            .into_iter()
            .map(|tool| tool["name"].as_str().unwrap().to_string())
            .collect()
    }

    #[test]
    fn backend_actions_match_specific_tool_surface() {
        let tool_names = tool_names();
        for action in BACKEND_ACTIONS {
            if *action == "manifest" {
                assert!(
                    tool_names.iter().any(|name| name == "forge_manifest"),
                    "manifest must be exposed as forge_manifest"
                );
                continue;
            }
            let expected = match *action {
                "load_lorebook" => "forge_lorebook".to_string(),
                "load_outline" => "forge_outline".to_string(),
                "effective_agent_tool_inventory" => "forge_effective_tool_inventory".to_string(),
                "get_project_brain_knowledge_graph" => {
                    "forge_project_brain_knowledge_graph".to_string()
                }
                other => format!("forge_{other}"),
            };
            assert!(
                tool_names.iter().any(|name| name == &expected),
                "backend action '{action}' is missing specific MCP tool '{expected}'"
            );
        }
    }

    #[test]
    fn every_specific_tool_has_backend_action_or_manifest_exception() {
        for name in tool_names() {
            if name == "forge_backend_call" || name == "forge_manifest" {
                continue;
            }
            let action = match name.as_str() {
                "forge_lorebook" => "load_lorebook",
                "forge_outline" => "load_outline",
                "forge_effective_tool_inventory" => "effective_agent_tool_inventory",
                "forge_project_brain_knowledge_graph" => "get_project_brain_knowledge_graph",
                other => other.strip_prefix("forge_").unwrap(),
            };
            assert!(
                BACKEND_ACTIONS.contains(&action),
                "MCP tool '{name}' is not backed by BACKEND_ACTIONS"
            );
        }
    }

    #[test]
    fn write_tools_are_not_marked_read_only() {
        for name in [
            "forge_create_chapter",
            "forge_save_chapter",
            "forge_rename_chapter_file",
            "forge_save_lore_entry",
            "forge_delete_lore_entry",
            "forge_save_outline_node",
            "forge_delete_outline_node",
            "forge_update_outline_status",
            "forge_reorder_outline_nodes",
            "forge_save_volume",
            "forge_delete_volume",
            "forge_save_volume_snapshot",
            "forge_save_book_state",
            "forge_observe",
            "forge_apply_feedback",
            "forge_record_implicit_ghost_rejection",
            "forge_approve_writer_operation",
            "forge_record_writer_operation_durable_save",
            "forge_start_sprint",
            "forge_pause_sprint",
            "forge_resume_sprint",
            "forge_cancel_sprint",
            "forge_checkpoint_sprint",
            "forge_record_sprint_budget_usage",
            "forge_set_sprint_quality_gate",
            "forge_latest_chapter_generation_checkpoint",
            "forge_chapter_generation_resume_candidates",
            "forge_record_manual_craft_edit_feedback",
            "forge_ingest_external_research",
            "forge_restore_file_backup",
            "forge_set_api_key",
        ] {
            assert!(!is_read_only_tool(name), "{name} must not be read-only");
        }
    }

    #[test]
    fn tool_annotations_cover_known_destructive_and_open_world_cases() {
        for name in [
            "forge_backend_call",
            "forge_delete_lore_entry",
            "forge_delete_outline_node",
            "forge_delete_volume",
            "forge_restore_project_brain_source_revision",
            "forge_cancel_sprint",
            "forge_restore_file_backup",
            "forge_rename_chapter_file",
        ] {
            assert!(is_destructive_tool(name), "{name} must be destructive");
        }

        for name in [
            "forge_ask_agent",
            "forge_generate_chapter_autonomous",
            "forge_project_brain_knowledge_graph",
            "forge_ingest_external_research",
            "forge_run_metacognitive_recovery",
        ] {
            assert!(is_open_world_tool(name), "{name} must be open-world");
        }
    }

    #[test]
    fn tool_result_uses_structured_content_envelope() {
        let result = tool_result(json!({ "value": 1 }), false);
        assert_eq!(result["isError"], false);
        assert_eq!(result["structuredContent"]["ok"], true);
        assert_eq!(result["structuredContent"]["data"]["value"], 1);
        assert!(result["structuredContent"]["error"].is_null());

        let error = tool_error_result(ErrorKind::Backend, "failed".to_string());
        assert_eq!(error["isError"], true);
        assert_eq!(error["structuredContent"]["ok"], false);
        assert_eq!(error["structuredContent"]["error"]["kind"], "backend");
        assert_eq!(error["structuredContent"]["error"]["message"], "failed");
    }

    #[test]
    fn validation_error_has_correct_kind() {
        let result = tool_error_result(ErrorKind::Validation, "chapterTitle is required".into());
        let sc = &result["structuredContent"];
        assert_eq!(sc["ok"], false);
        assert_eq!(sc["error"]["kind"], "validation");
        assert_eq!(sc["error"]["message"], "chapterTitle is required");
    }

    #[test]
    fn provider_error_has_correct_kind() {
        let result = tool_error_result(
            ErrorKind::Provider,
            "LLM call failed (429): rate limited".into(),
        );
        let sc = &result["structuredContent"];
        assert_eq!(sc["error"]["kind"], "provider");
    }

    #[test]
    fn classify_error_detects_validation() {
        assert_eq!(
            classify_error("forge_load_chapter", "chapterTitle is required"),
            ErrorKind::Validation
        );
    }

    #[test]
    fn classify_error_detects_provider() {
        assert_eq!(
            classify_error("forge_ask_agent", "LLM call failed (500)"),
            ErrorKind::Provider
        );
    }

    #[test]
    fn classify_error_defaults_to_backend() {
        assert_eq!(
            classify_error("forge_status", "something unexpected happened"),
            ErrorKind::Backend
        );
    }

    #[test]
    fn classify_error_permission_denied() {
        assert_eq!(
            classify_error("forge_save_chapter", "Tool requires explicit approval"),
            ErrorKind::Permission
        );
    }

    #[test]
    fn classify_error_detects_budget() {
        assert_eq!(
            classify_error("forge_generate_chapter", "provider budget exceeded"),
            ErrorKind::Budget
        );
    }

    #[test]
    fn classify_error_detects_context_overflow() {
        assert_eq!(
            classify_error("forge_ask_agent", "context length exceeds token limit"),
            ErrorKind::ContextOverflow
        );
    }

    #[test]
    fn classify_error_detects_storage() {
        assert_eq!(
            classify_error("forge_save_chapter", "sqlite storage save failed"),
            ErrorKind::Storage
        );
    }
}
