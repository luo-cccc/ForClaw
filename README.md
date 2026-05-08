# Forge Agent

[![CI](https://github.com/luo-cccc/ForClaw/actions/workflows/ci.yml/badge.svg)](https://github.com/luo-cccc/ForClaw/actions/workflows/ci.yml)

Forge Agent is a headless long-form fiction writing agent exposed through the Model Context Protocol (MCP). It provides project storage, chapter operations, story memory, Project Brain retrieval, proposal review, diagnostics, supervised chapter sprints, and model-backed writing workflows through a stable stdio server.

The repository is designed as a backend-first agent runtime. MCP clients and schedulers can run the agent as a plugin without embedding a desktop UI.

## Features

- Headless MCP server over newline-delimited JSON-RPC stdio.
- Full writer-agent backend with story ledger, proposals, typed operations, memory, diagnostics, and trace history.
- Chapter generation pipeline with context assembly, provider-budget checks, draft validation, repair/compression, revision-safe saves, settlement, runtime artifacts, and Project Brain embedding.
- Project assets for chapters, lorebook, outline, volume snapshots, book state, backups, and storage diagnostics.
- Project Brain tools for graph indexing, source revision comparison/restore, cross-reference suggestions, and approved external research ingest.
- Supervised sprint tools for planning, progress, pause/resume/cancel, checkpointing, and budget accounting.
- Codex plugin metadata under `plugins/forge-writer-agent/`.

## Repository Layout

```text
agent-harness-core/       Shared agent runtime, providers, tool registry, context packing, memory utilities
agent-writer-backend/     Forge writer backend and headless project implementation
forge-agent-mcp/          MCP stdio server that exposes backend capabilities
plugins/forge-writer-agent/ Codex plugin bundle and skill metadata
scripts/                  Windows launchers for the MCP server
docs/                     Protocol and caller contracts
config/                   Runtime configuration assets
```

## Requirements

- Rust stable toolchain
- Windows PowerShell or `cmd` for the bundled launch scripts
- Optional: `OPENAI_API_KEY` or a stored provider key for model-backed tools

The default build is headless. Desktop/Tauri support is available only when explicitly enabling the `desktop` feature for `agent-writer`.

## Quick Start

Build the MCP server:

```powershell
cargo build -p forge-agent-mcp
```

Run it over stdio:

```powershell
.\scripts\forge-agent-mcp.cmd stdio
```

By default, project data is stored in `.forge-agent-data` at the repository root. Override it with:

```powershell
$env:FORGE_AGENT_DATA_DIR="C:\path\to\forge-data"
```

For model-backed tools:

```powershell
$env:OPENAI_API_KEY="..."
$env:OPENAI_API_BASE="https://openrouter.ai/api/v1"
$env:OPENAI_MODEL="deepseek/deepseek-v4-flash"
```

## MCP Usage

The stdio entrypoint is `scripts\forge-agent-mcp.cmd`. It launches `forge-agent-mcp.exe`, keeps logs on stderr, and reserves stdout for JSON-RPC responses. The bundled plugin starts it as:

```json
{
  "command": "cmd",
  "args": ["/c", "scripts\\forge-agent-mcp.cmd", "stdio"],
  "cwd": "..\\.."
}
```

Use `initialize`, then `tools/list`, then `tools/call` from an MCP client. `forge_backend_call` is the stable generic dispatcher:

```json
{
  "action": "status",
  "params": {}
}
```

Specific MCP tools are exposed for discovery and scheduling:

- Protocol/project: `forge_manifest`, `forge_project_manifest`, `forge_project_paths`
- Agent/kernel: `forge_ask_agent`, `forge_agent_tools`, `forge_effective_tool_inventory`, `forge_agent_kernel_status`, `forge_agent_domain_profile`, `forge_status`
- Chapters/generation: `forge_list_chapters`, `forge_create_chapter`, `forge_load_chapter`, `forge_save_chapter`, `forge_chapter_revision`, `forge_rename_chapter_file`, `forge_generate_chapter_autonomous`, `forge_batch_generate_chapter`, `forge_repair_chapter_state`
- Lore/outline/book structure: `forge_lorebook`, `forge_save_lore_entry`, `forge_delete_lore_entry`, `forge_outline`, `forge_save_outline_node`, `forge_delete_outline_node`, `forge_update_outline_status`, `forge_reorder_outline_nodes`, `forge_list_volumes`, `forge_save_volume`, `forge_delete_volume`, `forge_get_volume_snapshot`, `forge_save_volume_snapshot`, `forge_get_book_state`, `forge_save_book_state`
- Writer memory/agent state: `forge_observe`, `forge_ledger`, `forge_today_five`, `forge_pending_proposals`, `forge_story_review_queue`, `forge_story_debt`, `forge_reader_compensation_review_chain`, `forge_trace`, `forge_inspector_timeline`, `forge_companion_timeline_summary`
- Proposals/operations: `forge_apply_feedback`, `forge_record_implicit_ghost_rejection`, `forge_approve_writer_operation`, `forge_record_writer_operation_durable_save`, `forge_ambient_entity_hints`
- Model-backed helpers: `forge_analyze_chapter`, `forge_generate_parallel_drafts`, `forge_analyze_pacing`, `forge_ask_project_brain`, `forge_run_metacognitive_recovery`
- Project Brain: `forge_project_brain_knowledge_graph`, `forge_compare_project_brain_source_revisions`, `forge_restore_project_brain_source_revision`, `forge_cross_reference_brain_nodes`, `forge_ingest_external_research`
- Supervised sprint: `forge_start_sprint`, `forge_sprint_plan`, `forge_sprint_progress`, `forge_pause_sprint`, `forge_resume_sprint`, `forge_cancel_sprint`, `forge_checkpoint_sprint`, `forge_record_sprint_budget_usage`
- Project diagnostics: `forge_project_graph_data`, `forge_project_storage_diagnostics`, `forge_export_writer_agent_trajectory`, `forge_export_diagnostic_logs`, `forge_list_file_backups`, `forge_restore_file_backup`
- Settings: `forge_set_api_key`, `forge_check_api_key`

Machine-readable tool responses use the stable Forge envelope in `result.structuredContent`:

```json
{
  "ok": true,
  "data": {},
  "error": null
}
```

Backend failures return `ok: false` with `error.kind` and `error.message`. JSON-RPC parse, invalid request, and unknown method failures use standard JSON-RPC error responses.

For caller context requirements, budget approval rules, revision safety, and write-sensitive scheduling, see [docs/CONTEXT_CONTRACT.md](docs/CONTEXT_CONTRACT.md).

## Development

Run the default headless checks:

```powershell
cargo fmt --check
cargo check -p forge-agent-mcp
cargo test -p forge-agent-mcp
cargo test -p agent-harness-core
cargo test -p agent-writer --lib
cargo clippy -p agent-harness-core --all-targets -- -D warnings
cargo clippy -p agent-writer --all-targets -- -D warnings
cargo clippy -p forge-agent-mcp --all-targets -- -D warnings
```

Run the optional desktop feature locally:

```powershell
cargo test -p agent-writer --features desktop
cargo build -p agent-writer --features desktop
```

The headless path does not require Tauri config, icons, generated desktop schemas, or renderer assets.

## Plugin

The Codex plugin bundle lives at `plugins/forge-writer-agent/`. It exposes the MCP server named `forge-writer-agent` and defaults data storage to `.forge-agent-data` unless `FORGE_AGENT_DATA_DIR` is set.

## Security

Do not commit local project data, provider keys, logs, or generated desktop artifacts. See [SECURITY.md](SECURITY.md) for vulnerability reporting and operational guidance.

## Contributing

Contributions should preserve the MCP contract, keep the headless path buildable in a clean clone, and include focused tests for protocol or backend behavior changes. See [CONTRIBUTING.md](CONTRIBUTING.md).

## License

This project is currently unlicensed. Do not redistribute or reuse the code outside the permissions granted by the repository owner.
