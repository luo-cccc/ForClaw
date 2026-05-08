use std::io::Write;
use std::process::ExitCode;

use agent_writer_lib::headless::{
    AnalyzeChapterRequest, AskAgentRequest, AskProjectBrainRequest, BatchGenerateChapterRequest,
    HeadlessBackend, MetacognitiveRecoveryRequest, ParallelDraftRequest,
};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, BufReader};

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
    "craft_library",
    "craft_memory_stats",
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
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
enum ErrorKind {
    Backend,
    Validation,
    Provider,
    Permission,
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
struct ToolCallParams {
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
                    Err(error) => Some(success_response(
                        id,
                        tool_error_result(ErrorKind::Backend, error),
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

fn server_manifest() -> Value {
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

fn classify_error(_tool_name: &str, error: &str) -> ErrorKind {
    let lower = error.to_ascii_lowercase();
    if lower.contains("invalid") && (lower.contains("request") || lower.contains("tool call")) {
        return ErrorKind::Validation;
    }
    if lower.contains("required")
        || lower.contains("must not be empty")
        || lower.contains("missing")
    {
        return ErrorKind::Validation;
    }
    if lower.contains("llm call failed")
        || lower.contains("http request failed")
        || lower.contains("stream read error")
        || lower.contains("rate limit")
        || lower.contains("429")
    {
        return ErrorKind::Provider;
    }
    if lower.contains("approval")
        || lower.contains("permission")
        || lower.contains("denied")
        || lower.contains("read-only")
    {
        return ErrorKind::Permission;
    }
    ErrorKind::Backend
}

async fn call_tool(backend: &HeadlessBackend, params: Value) -> Result<Value, String> {
    let call: ToolCallParams = match serde_json::from_value(params) {
        Ok(c) => c,
        Err(error) => {
            return Ok(tool_error_result(
                ErrorKind::Validation,
                format!("Invalid tool call: {}", error),
            ));
        }
    };
    let arguments = if call.arguments.is_null() {
        json!({})
    } else {
        call.arguments
    };

    let result = match call.name.as_str() {
        "forge_backend_call" => {
            let action = arguments
                .get("action")
                .and_then(Value::as_str)
                .ok_or_else(|| "action is required".to_string())?;
            let params = arguments
                .get("params")
                .cloned()
                .unwrap_or_else(|| json!({}));
            call_backend_action(backend, action, params).await
        }
        "forge_manifest" => Ok(server_manifest()),
        "forge_project_manifest" => serde_json::to_value(backend.project())
            .map_err(|error| format!("Failed to serialize project manifest: {}", error)),
        "forge_project_paths" => backend.dispatch("project_paths", json!({})),
        "forge_agent_tools" => backend.dispatch("agent_tools", json!({})),
        "forge_effective_tool_inventory" => {
            backend.dispatch("effective_agent_tool_inventory", json!({}))
        }
        "forge_agent_kernel_status" => backend.dispatch("agent_kernel_status", json!({})),
        "forge_agent_domain_profile" => backend.dispatch("agent_domain_profile", json!({})),
        "forge_status" => backend.dispatch("status", json!({})),
        "forge_list_chapters" => backend.dispatch("list_chapters", json!({})),
        "forge_create_chapter" => backend.dispatch("create_chapter", arguments),
        "forge_load_chapter" => backend.dispatch("load_chapter", arguments),
        "forge_save_chapter" => backend.dispatch("save_chapter", arguments),
        "forge_chapter_revision" => backend.dispatch("chapter_revision", arguments),
        "forge_rename_chapter_file" => backend.dispatch("rename_chapter_file", arguments),
        "forge_lorebook" => backend.dispatch("load_lorebook", json!({})),
        "forge_save_lore_entry" => backend.dispatch("save_lore_entry", arguments),
        "forge_delete_lore_entry" => backend.dispatch("delete_lore_entry", arguments),
        "forge_outline" => backend.dispatch("load_outline", json!({})),
        "forge_save_outline_node" => backend.dispatch("save_outline_node", arguments),
        "forge_delete_outline_node" => backend.dispatch("delete_outline_node", arguments),
        "forge_update_outline_status" => backend.dispatch("update_outline_status", arguments),
        "forge_reorder_outline_nodes" => backend.dispatch("reorder_outline_nodes", arguments),
        "forge_list_volumes" => backend.dispatch("list_volumes", json!({})),
        "forge_save_volume" => backend.dispatch("save_volume", arguments),
        "forge_delete_volume" => backend.dispatch("delete_volume", arguments),
        "forge_get_volume_snapshot" => backend.dispatch("get_volume_snapshot", arguments),
        "forge_save_volume_snapshot" => backend.dispatch("save_volume_snapshot", arguments),
        "forge_get_book_state" => backend.dispatch("get_book_state", json!({})),
        "forge_save_book_state" => backend.dispatch("save_book_state", arguments),
        "forge_observe" => {
            let observation = arguments.get("observation").cloned().unwrap_or(arguments);
            backend.dispatch("observe", observation)
        }
        "forge_ledger" => backend.dispatch("ledger", json!({})),
        "forge_today_five" => backend.dispatch("today_five", json!({})),
        "forge_pending_proposals" => backend.dispatch("pending_proposals", json!({})),
        "forge_story_review_queue" => backend.dispatch("story_review_queue", json!({})),
        "forge_story_debt" => backend.dispatch("story_debt", json!({})),
        "forge_reader_compensation_review_chain" => {
            backend.dispatch("reader_compensation_review_chain", json!({}))
        }
        "forge_trace" => backend.dispatch("trace", arguments),
        "forge_inspector_timeline" => backend.dispatch("inspector_timeline", arguments),
        "forge_companion_timeline_summary" => {
            backend.dispatch("companion_timeline_summary", json!({}))
        }
        "forge_apply_feedback" => backend.dispatch("apply_feedback", arguments),
        "forge_record_implicit_ghost_rejection" => {
            backend.dispatch("record_implicit_ghost_rejection", arguments)
        }
        "forge_approve_writer_operation" => backend.dispatch("approve_writer_operation", arguments),
        "forge_record_writer_operation_durable_save" => {
            backend.dispatch("record_writer_operation_durable_save", arguments)
        }
        "forge_ambient_entity_hints" => backend.dispatch("ambient_entity_hints", arguments),
        "forge_ask_agent" => {
            let request: AskAgentRequest = serde_json::from_value(arguments)
                .map_err(|error| format!("Invalid ask-agent request: {}", error))?;
            serde_json::to_value(backend.ask_agent(request).await?)
                .map_err(|error| error.to_string())
        }
        "forge_batch_generate_chapter" => {
            let request: BatchGenerateChapterRequest = serde_json::from_value(arguments)
                .map_err(|error| format!("Invalid batch generation request: {}", error))?;
            serde_json::to_value(backend.batch_generate_chapter(request).await?)
                .map_err(|error| error.to_string())
        }
        "forge_generate_chapter_autonomous" => {
            let request = serde_json::from_value(arguments)
                .map_err(|error| format!("Invalid chapter generation request: {}", error))?;
            serde_json::to_value(backend.generate_chapter_autonomous(request).await?)
                .map_err(|error| error.to_string())
        }
        "forge_repair_chapter_state" => {
            let chapter_title = arguments
                .get("chapterTitle")
                .or_else(|| arguments.get("chapter_title"))
                .and_then(Value::as_str)
                .ok_or_else(|| "chapterTitle is required".to_string())?
                .to_string();
            serde_json::to_value(backend.repair_chapter_state(chapter_title)?)
                .map_err(|error| error.to_string())
        }
        "forge_analyze_chapter" => {
            let request: AnalyzeChapterRequest = serde_json::from_value(arguments)
                .map_err(|error| format!("Invalid analyze-chapter request: {}", error))?;
            serde_json::to_value(backend.analyze_chapter(request).await?)
                .map_err(|error| error.to_string())
        }
        "forge_generate_parallel_drafts" => {
            let request: ParallelDraftRequest = serde_json::from_value(arguments)
                .map_err(|error| format!("Invalid parallel-draft request: {}", error))?;
            serde_json::to_value(backend.generate_parallel_drafts(request).await?)
                .map_err(|error| error.to_string())
        }
        "forge_analyze_pacing" => backend
            .analyze_pacing(
                arguments
                    .get("summaries")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
            )
            .await
            .and_then(|value| serde_json::to_value(value).map_err(|error| error.to_string())),
        "forge_ask_project_brain" => {
            let request: AskProjectBrainRequest = serde_json::from_value(arguments)
                .map_err(|error| format!("Invalid project-brain request: {}", error))?;
            serde_json::to_value(backend.ask_project_brain(request).await?)
                .map_err(|error| error.to_string())
        }
        "forge_project_brain_knowledge_graph" => {
            backend.dispatch("get_project_brain_knowledge_graph", json!({}))
        }
        "forge_compare_project_brain_source_revisions" => {
            backend.dispatch("compare_project_brain_source_revisions", arguments)
        }
        "forge_restore_project_brain_source_revision" => {
            backend.dispatch("restore_project_brain_source_revision", arguments)
        }
        "forge_cross_reference_brain_nodes" => {
            backend.dispatch("cross_reference_brain_nodes", arguments)
        }
        "forge_ingest_external_research" => backend.dispatch("ingest_external_research", arguments),
        "forge_run_metacognitive_recovery" => {
            let request: MetacognitiveRecoveryRequest = serde_json::from_value(arguments)
                .map_err(|error| format!("Invalid metacognitive recovery request: {}", error))?;
            serde_json::to_value(backend.run_metacognitive_recovery(request).await?)
                .map_err(|error| error.to_string())
        }
        "forge_craft_library" => backend.dispatch("craft_library", json!({})),
        "forge_craft_memory_stats" => backend.dispatch("craft_memory_stats", json!({})),
        "forge_chapter_quality_report" => backend.dispatch("chapter_quality_report", arguments),
        "forge_context_quality_report" => backend.dispatch("context_quality_report", arguments),
        "forge_budget_calibration" => backend.dispatch("budget_calibration", json!({})),
        "forge_execution_plan" => backend.dispatch("execution_plan", json!({})),
        "forge_start_sprint" => backend.dispatch("start_sprint", arguments),
        "forge_sprint_plan" => backend.dispatch("sprint_plan", json!({})),
        "forge_sprint_progress" => backend.dispatch("sprint_progress", json!({})),
        "forge_pause_sprint" => backend.dispatch("pause_sprint", json!({})),
        "forge_resume_sprint" => backend.dispatch("resume_sprint", json!({})),
        "forge_cancel_sprint" => backend.dispatch("cancel_sprint", json!({})),
        "forge_checkpoint_sprint" => backend.dispatch("checkpoint_sprint", arguments),
        "forge_record_sprint_budget_usage" => {
            backend.dispatch("record_sprint_budget_usage", arguments)
        }
        "forge_set_sprint_quality_gate" => backend.dispatch("set_sprint_quality_gate", arguments),
        "forge_project_graph_data" => backend.dispatch("project_graph_data", json!({})),
        "forge_project_storage_diagnostics" => {
            backend.dispatch("project_storage_diagnostics", json!({}))
        }
        "forge_export_writer_agent_trajectory" => {
            backend.dispatch("export_writer_agent_trajectory", arguments)
        }
        "forge_export_diagnostic_logs" => backend.dispatch("export_diagnostic_logs", json!({})),
        "forge_list_file_backups" => backend.dispatch("list_file_backups", arguments),
        "forge_restore_file_backup" => backend.dispatch("restore_file_backup", arguments),
        "forge_set_api_key" => backend.dispatch("set_api_key", arguments),
        "forge_check_api_key" => backend.dispatch("check_api_key", arguments),
        other => {
            return Ok(tool_error_result(
                ErrorKind::Validation,
                format!("Unknown tool: {}", other),
            ));
        }
    };

    match result {
        Ok(value) => Ok(tool_result(value, false)),
        Err(error) => {
            let kind = classify_error(&call.name, &error);
            Ok(tool_error_result(kind, error))
        }
    }
}

async fn call_backend_action(
    backend: &HeadlessBackend,
    action: &str,
    params: Value,
) -> Result<Value, String> {
    match action {
        "manifest" => Ok(server_manifest()),
        "project_manifest" => serde_json::to_value(backend.project())
            .map_err(|error| format!("Failed to serialize project manifest: {}", error)),
        "ask_agent" => {
            let request: AskAgentRequest = serde_json::from_value(params)
                .map_err(|error| format!("Invalid ask-agent request: {}", error))?;
            serde_json::to_value(backend.ask_agent(request).await?)
                .map_err(|error| error.to_string())
        }
        "batch_generate_chapter" => {
            let request: BatchGenerateChapterRequest = serde_json::from_value(params)
                .map_err(|error| format!("Invalid batch generation request: {}", error))?;
            serde_json::to_value(backend.batch_generate_chapter(request).await?)
                .map_err(|error| error.to_string())
        }
        "generate_chapter_autonomous" => {
            let request = serde_json::from_value(params)
                .map_err(|error| format!("Invalid chapter generation request: {}", error))?;
            serde_json::to_value(backend.generate_chapter_autonomous(request).await?)
                .map_err(|error| error.to_string())
        }
        "analyze_chapter" => {
            let request: AnalyzeChapterRequest = serde_json::from_value(params)
                .map_err(|error| format!("Invalid analyze-chapter request: {}", error))?;
            serde_json::to_value(backend.analyze_chapter(request).await?)
                .map_err(|error| error.to_string())
        }
        "repair_chapter_state" => {
            let chapter_title = params
                .get("chapterTitle")
                .or_else(|| params.get("chapter_title"))
                .and_then(Value::as_str)
                .ok_or_else(|| "chapterTitle is required".to_string())?
                .to_string();
            serde_json::to_value(backend.repair_chapter_state(chapter_title)?)
                .map_err(|error| error.to_string())
        }
        "generate_parallel_drafts" => {
            let request: ParallelDraftRequest = serde_json::from_value(params)
                .map_err(|error| format!("Invalid parallel-draft request: {}", error))?;
            serde_json::to_value(backend.generate_parallel_drafts(request).await?)
                .map_err(|error| error.to_string())
        }
        "analyze_pacing" => {
            let summaries = params
                .get("summaries")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            serde_json::to_value(backend.analyze_pacing(summaries).await?)
                .map_err(|error| error.to_string())
        }
        "ask_project_brain" => {
            let request: AskProjectBrainRequest = serde_json::from_value(params)
                .map_err(|error| format!("Invalid project-brain request: {}", error))?;
            serde_json::to_value(backend.ask_project_brain(request).await?)
                .map_err(|error| error.to_string())
        }
        "run_metacognitive_recovery" => {
            let request: MetacognitiveRecoveryRequest = serde_json::from_value(params)
                .map_err(|error| format!("Invalid metacognitive recovery request: {}", error))?;
            serde_json::to_value(backend.run_metacognitive_recovery(request).await?)
                .map_err(|error| error.to_string())
        }
        _ => backend.dispatch(action, params),
    }
}

fn tool_result(value: Value, is_error: bool) -> Value {
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

fn tool_error_result(kind: ErrorKind, message: String) -> Value {
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

fn tools() -> Vec<Value> {
    let mut tools = vec![
        tool(
            "forge_backend_call",
            "Forge Backend Dispatcher",
            "Dispatch any supported backend action by name with JSON params. This is the stable single-entrypoint surface for plugin schedulers.",
            json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": BACKEND_ACTIONS
                    },
                    "params": { "type": "object" }
                },
                "additionalProperties": false,
                "required": ["action"]
            }),
        ),
        tool(
            "forge_manifest",
            "Forge Manifest",
            "Return server, protocol, transport, capability, and supported backend action metadata.",
            empty_schema(),
        ),
        tool(
            "forge_project_manifest",
            "Project Manifest",
            "Return the active headless Forge project manifest.",
            empty_schema(),
        ),
        tool(
            "forge_project_paths",
            "Project Paths",
            "Return active data, project, chapters, and writer-memory paths.",
            empty_schema(),
        ),
        tool(
            "forge_agent_tools",
            "Agent Tool Descriptors",
            "Return the backend writer-agent tool descriptors registered in Forge.",
            empty_schema(),
        ),
        tool(
            "forge_effective_tool_inventory",
            "Effective Tool Inventory",
            "Return allowed, blocked, and model-callable tool inventory after policy filtering.",
            empty_schema(),
        ),
        tool(
            "forge_agent_kernel_status",
            "Agent Kernel Status",
            "Return low-level tool registry, domain, quality gate, and trace status.",
            empty_schema(),
        ),
        tool(
            "forge_agent_domain_profile",
            "Agent Domain Profile",
            "Return the writing domain profile used by the agent harness.",
            empty_schema(),
        ),
        tool(
            "forge_status",
            "Forge Status",
            "Return project paths, active sprint, and writer-kernel status.",
            empty_schema(),
        ),
        tool(
            "forge_list_chapters",
            "List Chapters",
            "List local Markdown chapters in the active Forge project.",
            empty_schema(),
        ),
        tool(
            "forge_create_chapter",
            "Create Chapter",
            "Create an empty chapter file if it does not already exist.",
            title_schema(),
        ),
        tool(
            "forge_load_chapter",
            "Load Chapter",
            "Load a chapter's Markdown content.",
            title_schema(),
        ),
        tool(
            "forge_save_chapter",
            "Save Chapter",
            "Atomically save chapter content, record a revision, and notify the writer kernel.",
            json!({
                "type": "object",
                "properties": {
                    "title": { "type": "string" },
                    "content": { "type": "string" }
                },
                "required": ["title", "content"]
            }),
        ),
        tool(
            "forge_chapter_revision",
            "Chapter Revision",
            "Return the content revision hash for a chapter.",
            title_schema(),
        ),
        tool(
            "forge_rename_chapter_file",
            "Rename Chapter File",
            "Rename a chapter Markdown file by filename.",
            json!({
                "type": "object",
                "properties": {
                    "oldName": { "type": "string" },
                    "newName": { "type": "string" }
                },
                "required": ["oldName", "newName"]
            }),
        ),
        tool(
            "forge_lorebook",
            "Load Lorebook",
            "Load lorebook entries for the active project.",
            empty_schema(),
        ),
        tool(
            "forge_save_lore_entry",
            "Save Lore Entry",
            "Create or update a lorebook entry.",
            json!({
                "type": "object",
                "properties": {
                    "keyword": { "type": "string" },
                    "content": { "type": "string" }
                },
                "required": ["keyword", "content"]
            }),
        ),
        tool(
            "forge_delete_lore_entry",
            "Delete Lore Entry",
            "Delete a lorebook entry by id.",
            id_schema(),
        ),
        tool(
            "forge_outline",
            "Load Outline",
            "Load outline nodes for the active project.",
            empty_schema(),
        ),
        tool(
            "forge_save_outline_node",
            "Save Outline Node",
            "Create or update an outline node and seed chapter mission memory.",
            json!({
                "type": "object",
                "properties": {
                    "chapterTitle": { "type": "string" },
                    "summary": { "type": "string" },
                    "status": { "type": "string" }
                },
                "required": ["chapterTitle", "summary"]
            }),
        ),
        tool(
            "forge_delete_outline_node",
            "Delete Outline Node",
            "Delete an outline node by chapter title.",
            chapter_title_schema(),
        ),
        tool(
            "forge_update_outline_status",
            "Update Outline Status",
            "Update an outline node status.",
            json!({
                "type": "object",
                "properties": {
                    "chapterTitle": { "type": "string" },
                    "status": { "type": "string" }
                },
                "required": ["chapterTitle", "status"]
            }),
        ),
        tool(
            "forge_reorder_outline_nodes",
            "Reorder Outline Nodes",
            "Reorder outline nodes by chapter title list.",
            json!({
                "type": "object",
                "properties": {
                    "orderedTitles": {
                        "type": "array",
                        "items": { "type": "string" }
                    }
                },
                "required": ["orderedTitles"]
            }),
        ),
        tool(
            "forge_list_volumes",
            "List Volumes",
            "List volume summaries from writer memory.",
            empty_schema(),
        ),
        tool(
            "forge_save_volume",
            "Save Volume",
            "Create or update a volume summary. Pass the VolumeSummary JSON object.",
            free_object_schema(),
        ),
        tool(
            "forge_delete_volume",
            "Delete Volume",
            "Delete a volume by id.",
            volume_id_schema(),
        ),
        tool(
            "forge_get_volume_snapshot",
            "Get Volume Snapshot",
            "Return the latest snapshot for a volume.",
            volume_id_schema(),
        ),
        tool(
            "forge_save_volume_snapshot",
            "Save Volume Snapshot",
            "Save a volume snapshot. Pass the VolumeSnapshotSummary JSON object.",
            free_object_schema(),
        ),
        tool(
            "forge_get_book_state",
            "Get Book State",
            "Return the project-level book state.",
            empty_schema(),
        ),
        tool(
            "forge_save_book_state",
            "Save Book State",
            "Save the project-level book state. Pass the BookStateSummary JSON object.",
            free_object_schema(),
        ),
        tool(
            "forge_observe",
            "Observe Writer State",
            "Send a typed WriterObservation into the writer kernel and return generated proposals.",
            json!({
                "type": "object",
                "properties": {
                    "observation": { "type": "object" }
                },
                "required": ["observation"]
            }),
        ),
        tool(
            "forge_ask_agent",
            "Ask Writer Agent",
            "Run the full headless writer-agent loop with project memory, tools, budget checks, and returned events.",
            json!({
                "type": "object",
                "properties": {
                    "message": { "type": "string" },
                    "context": { "type": "string" },
                    "paragraph": { "type": "string" },
                    "selectedText": { "type": "string" },
                    "chapterTitle": { "type": "string" },
                    "chapterRevision": { "type": "string" },
                    "cursorPosition": { "type": "integer", "minimum": 0 },
                    "dirty": { "type": "boolean" },
                    "inlineOperation": { "type": "boolean" },
                    "providerBudgetApproval": { "type": "object" }
                },
                "required": ["message"]
            }),
        ),
        tool(
            "forge_generate_chapter_autonomous",
            "Generate Chapter Autonomous",
            "Run Forge's full chapter-generation pipeline headlessly: build context, draft/repair length, save with revision checks, apply settlement, persist runtime artifacts, update memory, and return events.",
            json!({
                "type": "object",
                "properties": {
                    "requestId": { "type": "string" },
                    "targetChapterTitle": { "type": "string" },
                    "targetChapterNumber": { "type": "integer", "minimum": 1 },
                    "userInstruction": { "type": "string" },
                    "budget": { "type": "object" },
                    "frontendState": { "type": "object" },
                    "saveMode": {
                        "type": "string",
                        "enum": ["create_if_missing", "replace_if_clean", "save_as_draft"]
                    },
                    "chapterSummaryOverride": { "type": "string" },
                    "chapterContract": { "type": "object" },
                    "providerBudgetApproval": { "type": "object" }
                },
                "required": ["userInstruction"]
            }),
        ),
        tool(
            "forge_batch_generate_chapter",
            "Batch Generate Chapter",
            "Generate one chapter using the batch-generation defaults with a target title and summary.",
            json!({
                "type": "object",
                "properties": {
                    "chapterTitle": { "type": "string" },
                    "summary": { "type": "string" },
                    "frontendState": { "type": "object" }
                },
                "required": ["chapterTitle", "summary"]
            }),
        ),
        tool(
            "forge_repair_chapter_state",
            "Repair Chapter State",
            "Rebuild settlement/runtime artifacts and memory state for an existing chapter without rewriting chapter text.",
            chapter_title_schema(),
        ),
        tool(
            "forge_analyze_chapter",
            "Analyze Chapter",
            "Use the configured model to return structured editorial review items for chapter content.",
            json!({
                "type": "object",
                "properties": {
                    "content": { "type": "string" }
                },
                "required": ["content"]
            }),
        ),
        tool(
            "forge_generate_parallel_drafts",
            "Generate Parallel Drafts",
            "Generate three alternative continuation drafts for the current cursor context.",
            json!({
                "type": "object",
                "properties": {
                    "prefix": { "type": "string" },
                    "suffix": { "type": "string" },
                    "paragraph": { "type": "string" },
                    "selectedText": { "type": "string" },
                    "chapterTitle": { "type": "string" },
                    "missionContext": { "type": "string" },
                    "promiseContext": { "type": "string" }
                },
                "required": ["prefix", "suffix", "paragraph", "selectedText"]
            }),
        ),
        tool(
            "forge_analyze_pacing",
            "Analyze Pacing",
            "Use the configured model to analyze pacing from chapter summaries.",
            json!({
                "type": "object",
                "properties": {
                    "summaries": { "type": "string" }
                },
                "required": ["summaries"]
            }),
        ),
        tool(
            "forge_ask_project_brain",
            "Ask Project Brain",
            "Ask the model a question grounded in the local Project Brain vector index.",
            json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string" },
                    "providerBudgetApproval": { "type": "object" }
                },
                "required": ["query"]
            }),
        ),
        tool(
            "forge_project_brain_knowledge_graph",
            "Project Brain Knowledge Graph",
            "Return or rebuild the Project Brain knowledge graph index.",
            empty_schema(),
        ),
        tool(
            "forge_compare_project_brain_source_revisions",
            "Compare Project Brain Revisions",
            "Compare active and archived revisions for a Project Brain source reference.",
            json!({
                "type": "object",
                "properties": {
                    "sourceRef": { "type": "string" }
                },
                "required": ["sourceRef"]
            }),
        ),
        tool(
            "forge_restore_project_brain_source_revision",
            "Restore Project Brain Revision",
            "Restore a Project Brain source reference to a specific revision.",
            json!({
                "type": "object",
                "properties": {
                    "sourceRef": { "type": "string" },
                    "revision": { "type": "string" }
                },
                "required": ["sourceRef", "revision"]
            }),
        ),
        tool(
            "forge_cross_reference_brain_nodes",
            "Cross Reference Brain Nodes",
            "Create a suggested cross-reference between two Project Brain knowledge nodes.",
            json!({
                "type": "object",
                "properties": {
                    "sourceNodeId": { "type": "string" },
                    "targetNodeId": { "type": "string" }
                },
                "required": ["sourceNodeId", "targetNodeId"]
            }),
        ),
        tool(
            "forge_ingest_external_research",
            "Ingest External Research",
            "Author-approved ingest of an external research source into Project Brain.",
            json!({
                "type": "object",
                "properties": {
                    "provider": { "type": "string" },
                    "urlOrPath": { "type": "string" },
                    "title": { "type": "string" },
                    "content": { "type": "string" },
                    "authorApproved": { "type": "boolean" },
                    "approvalReason": { "type": "string" }
                },
                "required": ["provider", "title", "content", "authorApproved", "approvalReason"]
            }),
        ),
        tool(
            "forge_run_metacognitive_recovery",
            "Run Metacognitive Recovery",
            "Run a read-only planning review or continuity diagnostic through the headless writer-agent loop.",
            json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["planning_review", "continuity_diagnostic"]
                    },
                    "instruction": { "type": "string" },
                    "context": { "type": "string" },
                    "paragraph": { "type": "string" },
                    "selectedText": { "type": "string" },
                    "chapterTitle": { "type": "string" },
                    "chapterRevision": { "type": "string" },
                    "cursorPosition": { "type": "integer", "minimum": 0 },
                    "dirty": { "type": "boolean" },
                    "providerBudgetApproval": { "type": "object" }
                },
                "required": ["action"]
            }),
        ),
        tool(
            "forge_ledger",
            "Writer Ledger",
            "Return story contract, missions, canon, promises, decisions, and memory reliability.",
            empty_schema(),
        ),
        tool(
            "forge_today_five",
            "Today Five",
            "Return the compact companion summary for the current project/chapter.",
            empty_schema(),
        ),
        tool(
            "forge_pending_proposals",
            "Pending Proposals",
            "Return currently pending writer-agent proposals.",
            empty_schema(),
        ),
        tool(
            "forge_story_review_queue",
            "Story Review Queue",
            "Return review queue entries derived from non-ghost proposals.",
            empty_schema(),
        ),
        tool(
            "forge_story_debt",
            "Story Debt",
            "Return unresolved story debt across contract, mission, canon, promise, and pacing categories.",
            empty_schema(),
        ),
        tool(
            "forge_reader_compensation_review_chain",
            "Reader Compensation Chain",
            "Return reader-compensation review chain diagnostics.",
            empty_schema(),
        ),
        tool(
            "forge_trace",
            "Writer Trace",
            "Return recent writer-agent trace events.",
            json!({
                "type": "object",
                "properties": {
                    "limit": { "type": "integer", "minimum": 1, "maximum": 500 }
                }
            }),
        ),
        tool(
            "forge_inspector_timeline",
            "Inspector Timeline",
            "Return recent writer inspector timeline events.",
            json!({
                "type": "object",
                "properties": {
                    "limit": { "type": "integer", "minimum": 1, "maximum": 500 }
                }
            }),
        ),
        tool(
            "forge_companion_timeline_summary",
            "Companion Timeline Summary",
            "Return the compact companion-facing timeline summary.",
            empty_schema(),
        ),
        tool(
            "forge_apply_feedback",
            "Apply Proposal Feedback",
            "Record accepted/rejected/edited/snoozed/explained proposal feedback.",
            json!({
                "type": "object",
                "properties": {
                    "proposalId": { "type": "string" },
                    "action": {
                        "type": "string",
                        "enum": ["accepted", "rejected", "edited", "snoozed", "explained"]
                    },
                    "finalText": { "type": "string" },
                    "reason": { "type": "string" },
                    "createdAt": { "type": "integer" }
                },
                "required": ["proposalId", "action", "createdAt"]
            }),
        ),
        tool(
            "forge_record_implicit_ghost_rejection",
            "Record Ghost Rejection",
            "Record an implicit rejection for an expired ghost proposal.",
            json!({
                "type": "object",
                "properties": {
                    "proposalId": { "type": "string" },
                    "createdAt": { "type": "integer" }
                },
                "required": ["proposalId", "createdAt"]
            }),
        ),
        tool(
            "forge_approve_writer_operation",
            "Approve Writer Operation",
            "Approve and execute a typed writer operation using explicit approval context.",
            json!({
                "type": "object",
                "properties": {
                    "operation": { "type": "object" },
                    "currentRevision": { "type": "string" },
                    "approval": { "type": "object" }
                },
                "required": ["operation", "currentRevision"]
            }),
        ),
        tool(
            "forge_record_writer_operation_durable_save",
            "Record Durable Save",
            "Record durable save outcome for a writer operation.",
            free_object_schema(),
        ),
        tool(
            "forge_ambient_entity_hints",
            "Ambient Entity Hints",
            "Return canon entity hints for the given paragraph and chapter.",
            json!({
                "type": "object",
                "properties": {
                    "paragraph": { "type": "string" },
                    "chapter": { "type": "string" }
                },
                "required": ["paragraph", "chapter"]
            }),
        ),
        tool(
            "forge_start_sprint",
            "Start Supervised Sprint",
            "Create a supervised multi-chapter sprint plan.",
            json!({
                "type": "object",
                "properties": {
                    "chapterTitles": {
                        "type": "array",
                        "items": { "type": "string" },
                        "minItems": 1
                    },
                    "requireApprovalPerChapter": { "type": "boolean" },
                    "maxChaptersPerSession": { "type": "integer", "minimum": 1 },
                    "budgetCeilingMicros": { "type": "integer", "minimum": 0 }
                },
                "required": ["chapterTitles"]
            }),
        ),
        tool(
            "forge_sprint_plan",
            "Sprint Plan",
            "Return the active supervised sprint plan, if any.",
            empty_schema(),
        ),
        tool(
            "forge_sprint_progress",
            "Sprint Progress",
            "Return current supervised sprint progress, if one exists.",
            empty_schema(),
        ),
        tool(
            "forge_pause_sprint",
            "Pause Sprint",
            "Pause the active supervised sprint.",
            empty_schema(),
        ),
        tool(
            "forge_resume_sprint",
            "Resume Sprint",
            "Resume the active supervised sprint.",
            empty_schema(),
        ),
        tool(
            "forge_cancel_sprint",
            "Cancel Sprint",
            "Cancel the active supervised sprint.",
            empty_schema(),
        ),
        tool(
            "forge_checkpoint_sprint",
            "Checkpoint Sprint",
            "Record a checkpoint for the active supervised sprint.",
            json!({
                "type": "object",
                "properties": {
                    "source": { "type": "string" }
                }
            }),
        ),
        tool(
            "forge_record_sprint_budget_usage",
            "Record Sprint Budget Usage",
            "Add provider spend to the active supervised sprint budget accounting.",
            json!({
                "type": "object",
                "properties": {
                    "spentMicros": { "type": "integer", "minimum": 0 }
                },
                "required": ["spentMicros"]
            }),
        ),
        tool(
            "forge_set_sprint_quality_gate",
            "Set Sprint Quality Gate",
            "Configure quality thresholds for the active supervised sprint. Blocks chapter advancement when quality drops below the minimum score or a fatal issue is detected.",
            json!({
                "type": "object",
                "properties": {
                    "minimumQualityScore": { "type": "number", "minimum": 0.0, "maximum": 1.0 },
                    "stopOnFatalIssue": { "type": "boolean" }
                },
                "additionalProperties": false
            }),
        ),
        tool(
            "forge_craft_library",
            "Craft Library",
            "Return the loaded craft rule library with all available writing quality rules.",
            empty_schema(),
        ),
        tool(
            "forge_craft_memory_stats",
            "Craft Memory Stats",
            "Return craft memory statistics: rule acceptance/rejection rates across the project.",
            empty_schema(),
        ),
        tool(
            "forge_chapter_quality_report",
            "Chapter Quality Report",
            "Evaluate chapter text quality across 8 metrics and return a quality report with scores, issues, and revision targets.",
            json!({
                "type": "object",
                "properties": {
                    "chapterText": { "type": "string" },
                    "chapterTitle": { "type": "string" },
                    "targetMinChars": { "type": "integer", "minimum": 1 },
                    "targetMaxChars": { "type": "integer", "minimum": 1 }
                },
                "required": ["chapterText", "chapterTitle"]
            }),
        ),
        tool(
            "forge_context_quality_report",
            "Context Quality Report",
            "Evaluate context quality for a chapter generation request and return completeness and relevance diagnostics.",
            json!({
                "type": "object",
                "properties": {
                    "chapterTitle": { "type": "string" }
                },
                "required": ["chapterTitle"]
            }),
        ),
        tool(
            "forge_budget_calibration",
            "Budget Calibration",
            "Return chapter context budget calibration diagnostics for the current project.",
            empty_schema(),
        ),
        tool(
            "forge_execution_plan",
            "Execution Plan",
            "Return the current chapter generation execution plan with strategy and budget details.",
            empty_schema(),
        ),
        tool(
            "forge_project_graph_data",
            "Project Graph Data",
            "Return headless project graph data from lore, canon, chapters, and outline.",
            empty_schema(),
        ),
        tool(
            "forge_project_storage_diagnostics",
            "Project Storage Diagnostics",
            "Return diagnostics for project JSON files, chapter files, and writer memory database.",
            empty_schema(),
        ),
        tool(
            "forge_export_writer_agent_trajectory",
            "Export Writer Trajectory",
            "Export writer-agent trajectory JSONL to the local data log directory.",
            json!({
                "type": "object",
                "properties": {
                    "limit": { "type": "integer", "minimum": 1, "maximum": 1000 },
                    "format": {
                        "type": "string",
                        "enum": ["jsonl", "trace_viewer", "claude_code", "hf_agent_trace_viewer"]
                    }
                }
            }),
        ),
        tool(
            "forge_export_diagnostic_logs",
            "Export Diagnostic Logs",
            "Export diagnostic logs and storage diagnostics to a local zip file.",
            empty_schema(),
        ),
        tool(
            "forge_list_file_backups",
            "List File Backups",
            "List backups for a target project file.",
            backup_target_schema(),
        ),
        tool(
            "forge_restore_file_backup",
            "Restore File Backup",
            "Restore a backup for a target project file.",
            json!({
                "type": "object",
                "properties": {
                    "target": backup_target_schema(),
                    "backupId": { "type": "string" }
                },
                "required": ["target", "backupId"]
            }),
        ),
        tool(
            "forge_set_api_key",
            "Set API Key",
            "Store an API key for a provider in the backend keyring.",
            json!({
                "type": "object",
                "properties": {
                    "provider": { "type": "string" },
                    "key": { "type": "string" }
                },
                "required": ["provider", "key"]
            }),
        ),
        tool(
            "forge_check_api_key",
            "Check API Key",
            "Check whether an API key is available for a provider.",
            json!({
                "type": "object",
                "properties": {
                    "provider": { "type": "string" }
                },
                "required": ["provider"]
            }),
        ),
    ];
    tools.sort_by(|left, right| {
        left["name"]
            .as_str()
            .unwrap_or("")
            .cmp(right["name"].as_str().unwrap_or(""))
    });
    tools
}

fn tool(name: &str, title: &str, description: &str, input_schema: Value) -> Value {
    let read_only = is_read_only_tool(name);
    json!({
        "name": name,
        "title": title,
        "description": description,
        "inputSchema": input_schema,
        "outputSchema": tool_output_schema(),
        "annotations": {
            "title": title,
            "readOnlyHint": read_only,
            "destructiveHint": is_destructive_tool(name),
            "idempotentHint": is_idempotent_tool(name),
            "openWorldHint": is_open_world_tool(name)
        },
        "_meta": {
            "forge/action": name.strip_prefix("forge_").unwrap_or(name),
            "forge/backend": "headless",
            "forge/readOnly": read_only
        }
    })
}

fn tool_output_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "ok": { "type": "boolean" },
            "data": {},
            "error": {
                "anyOf": [
                    { "type": "null" },
                    {
                        "type": "object",
                        "properties": {
                            "message": { "type": "string" },
                            "kind": { "type": "string" }
                        },
                        "required": ["message", "kind"],
                        "additionalProperties": true
                    }
                ]
            }
        },
        "required": ["ok", "data", "error"],
        "additionalProperties": false
    })
}

fn is_read_only_tool(name: &str) -> bool {
    matches!(
        name,
        "forge_manifest"
            | "forge_project_manifest"
            | "forge_project_paths"
            | "forge_agent_tools"
            | "forge_effective_tool_inventory"
            | "forge_agent_kernel_status"
            | "forge_agent_domain_profile"
            | "forge_status"
            | "forge_list_chapters"
            | "forge_load_chapter"
            | "forge_chapter_revision"
            | "forge_lorebook"
            | "forge_outline"
            | "forge_list_volumes"
            | "forge_get_volume_snapshot"
            | "forge_get_book_state"
            | "forge_ledger"
            | "forge_today_five"
            | "forge_pending_proposals"
            | "forge_story_review_queue"
            | "forge_story_debt"
            | "forge_reader_compensation_review_chain"
            | "forge_trace"
            | "forge_inspector_timeline"
            | "forge_companion_timeline_summary"
            | "forge_analyze_chapter"
            | "forge_generate_parallel_drafts"
            | "forge_analyze_pacing"
            | "forge_ask_project_brain"
            | "forge_sprint_plan"
            | "forge_sprint_progress"
            | "forge_project_graph_data"
            | "forge_project_storage_diagnostics"
            | "forge_list_file_backups"
            | "forge_check_api_key"
    )
}

fn is_destructive_tool(name: &str) -> bool {
    matches!(
        name,
        "forge_backend_call"
            | "forge_delete_lore_entry"
            | "forge_delete_outline_node"
            | "forge_delete_volume"
            | "forge_restore_project_brain_source_revision"
            | "forge_cancel_sprint"
            | "forge_restore_file_backup"
            | "forge_rename_chapter_file"
    )
}

fn is_idempotent_tool(name: &str) -> bool {
    is_read_only_tool(name)
        || matches!(
            name,
            "forge_create_chapter"
                | "forge_save_chapter"
                | "forge_save_lore_entry"
                | "forge_save_outline_node"
                | "forge_update_outline_status"
                | "forge_reorder_outline_nodes"
                | "forge_save_volume"
                | "forge_save_volume_snapshot"
                | "forge_save_book_state"
                | "forge_record_writer_operation_durable_save"
                | "forge_pause_sprint"
                | "forge_resume_sprint"
                | "forge_set_api_key"
        )
}

fn is_open_world_tool(name: &str) -> bool {
    matches!(
        name,
        "forge_backend_call"
            | "forge_ask_agent"
            | "forge_batch_generate_chapter"
            | "forge_generate_chapter_autonomous"
            | "forge_analyze_chapter"
            | "forge_generate_parallel_drafts"
            | "forge_analyze_pacing"
            | "forge_ask_project_brain"
            | "forge_project_brain_knowledge_graph"
            | "forge_compare_project_brain_source_revisions"
            | "forge_restore_project_brain_source_revision"
            | "forge_cross_reference_brain_nodes"
            | "forge_ingest_external_research"
            | "forge_run_metacognitive_recovery"
    )
}

fn empty_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false
    })
}

fn title_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "title": { "type": "string" }
        },
        "required": ["title"]
    })
}

fn id_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "id": { "type": "string" }
        },
        "required": ["id"]
    })
}

fn chapter_title_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "chapterTitle": { "type": "string" }
        },
        "required": ["chapterTitle"]
    })
}

fn volume_id_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "volumeId": { "type": "string" }
        },
        "required": ["volumeId"]
    })
}

fn free_object_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": true
    })
}

fn backup_target_schema() -> Value {
    json!({
        "type": "object",
        "oneOf": [
            {
                "properties": {
                    "kind": {
                        "type": "string",
                        "enum": ["lorebook", "outline", "project_brain"]
                    }
                },
                "required": ["kind"]
            },
            {
                "properties": {
                    "kind": {
                        "type": "string",
                        "enum": ["chapter"]
                    },
                    "title": { "type": "string" }
                },
                "required": ["kind", "title"]
            }
        ]
    })
}

#[cfg(test)]
mod tests {
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
}
