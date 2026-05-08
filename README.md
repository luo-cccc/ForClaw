# Forge Agent

This folder is the headless backend-only Forge writer agent. It keeps the original backend/kernel capabilities from `C:\Users\Msi\Desktop\Forge` and adds a stdio MCP server so the agent can be scheduled as a plugin by MCP-capable hosts.

## What Is Kept

- `agent-writer-backend/`: Forge writer backend, writer kernel, memory, chapter generation, diagnostics, story ledger, supervised sprint logic, provider budget, and storage-related backend modules.
- `agent-harness-core/`: reusable agent runtime, provider abstraction, task packets, compaction, tool registry, vector/BM25 utilities, and Hermes memory.
- `forge-agent-mcp/`: stdio MCP adapter that exposes the backend as tools.
- `plugins/forge-writer-agent/`: local Codex plugin metadata pointing at the MCP server.

Frontend/Tauri desktop UI assets, Vite/React files, desktop icons, `dist/`, `node_modules/`, eval reports, and source repo worktrees are intentionally not part of this project. The Tauri desktop shell and renderer event stream are removed; the backend writer agent, storage, memory, chapter-generation pipeline, tools, provider budget checks, and headless model-backed agent loop remain.

## Run

```powershell
cargo build -p forge-agent-mcp
.\scripts\forge-agent-mcp.cmd
```

By default, project data is stored under `.forge-agent-data` in this folder. Override it with:

```powershell
$env:FORGE_AGENT_DATA_DIR="C:\path\to\forge-data"
```

For model-backed tools, set the same provider variables used by Forge:

```powershell
$env:OPENAI_API_KEY="..."
$env:OPENAI_API_BASE="https://openrouter.ai/api/v1"
$env:OPENAI_MODEL="deepseek/deepseek-v4-flash"
```

## MCP Tools

The stdio entrypoint is `scripts\forge-agent-mcp.cmd`. It runs the compiled `forge-agent-mcp.exe` in stdio mode, keeps logs on stderr, and leaves stdout for newline-delimited JSON-RPC responses only.

`forge_backend_call` is the stable generic dispatcher for backend actions. Specific tools are also exposed for client discovery and plugin scheduling:

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

The generic dispatcher accepts:

```json
{
  "action": "status",
  "params": {}
}
```

Use `tools/list` from an MCP client for the full JSON input schemas.

For scheduler and plugin authors, the context boundary is documented in `docs/CONTEXT_CONTRACT.md`. MCP callers pass intent, active editor state, chapter revision, dirty flag, and optional budget approval; Forge assembles durable project context from local storage and writer memory.

All tool calls return a stable `structuredContent` envelope:

```json
{
  "ok": true,
  "data": {},
  "error": null
}
```

Backend failures return `ok: false` with `error.message` and `isError: true`; JSON-RPC protocol failures use standard JSON-RPC error codes.

Model-backed tools require `OPENAI_API_KEY` or a stored provider key. `forge_ask_agent` returns the agent answer, proposals, typed operations, run metadata, and collected loop events instead of emitting Tauri renderer events. `forge_generate_chapter_autonomous` now uses the same chapter-generation pipeline behind a headless project storage abstraction, including context building, provider-budget checks, draft repair/compression, revision-safe save, settlement application, runtime artifacts, Project Brain embedding, and returned generation events.

What is intentionally not kept is the original Tauri/React desktop UI and renderer event stream. Editor-only renderer commands such as live prediction abort/reporting and semantic-lint UI state are not exposed as plugin tools because they are UI integration hooks, not backend agent capabilities.

## Verify

```powershell
cargo check -p forge-agent-mcp
cargo test -p agent-harness-core
cargo test -p agent-writer
```
