# Forge Agent

[![CI](https://github.com/luo-cccc/ForClaw/actions/workflows/ci.yml/badge.svg)](https://github.com/luo-cccc/ForClaw/actions/workflows/ci.yml)

Forge Agent is a headless long-form fiction writing agent exposed through the Model Context Protocol (MCP). It provides project storage, chapter operations, story memory, Project Brain retrieval, proposal review, diagnostics, supervised chapter sprints, and model-backed writing workflows through a stable stdio server.

The repository is designed as a headless backend runtime. MCP clients and schedulers run the agent as a plugin without a desktop application or renderer process.

## Features

- **MCP Server** — 91 tools over newline-delimited JSON-RPC 2.0 stdio. Structured error kind classification (`backend` | `validation` | `provider` | `permission` | `budget` | `context_overflow` | `storage`).
- **Writer Agent Kernel** — Story ledger, proposals, typed operations, canon, promises, chapter missions, reader compensation, decision tracking, trace history.
- **Chapter Generation Pipeline** — Context assembly → provider-budget checks → craft prompt injection → draft → quality evaluation → targeted revision → repair/compression → revision-safe save → settlement → artifact persistence.
- **Writing Empowerment Engine** — 8-rule craft library, Prompt Compiler with scene-type inference, SceneCraftPlan artifact generation, 8-metric ChapterQualityReport with evidence gating, targeted revision with before/after quality comparison.
- **Execution Plans** — Coarse-grained step plans (preflight/draft/validate/save) with Stop/Retry/Skip failure actions, wired into writer agent kernel.
- **Observability** — Real TTFT metrics, provider latency tracking, tool inventory snapshots, provider guard decisions, context window checks, budget calibration with rolling EMA, runaway loop detection.
- **Project Assets** — Chapters, lorebook, outline, volume snapshots, book state, backups, storage diagnostics.
- **Project Brain** — Graph indexing, source revision comparison/restore, cross-reference suggestions, external research ingest.
- **Supervised Sprints** — Planning, progress, pause/resume/cancel, checkpointing, quality gate thresholds, budget accounting.
- **Craft Memory** — SQLite-backed craft rule feedback learning. Compiler adjusts rule priority by acceptance rate.
- **Recovery Strategies** — Structured failure bundles with retry/shrink-context/approval-required/stop actions.
- **Codex Plugin** — Plugin metadata under `plugins/forge-writer-agent/`.

## Repository Layout

```text
agent-harness-core/       Shared agent runtime — AgentLoop, ExecutionPlan, provider,
                           tool registry, context quality, budget calibration, recovery,
                           compaction, permission policy, credential pool, vector DB
agent-writer-backend/     Forge writer backend — HeadlessBackend, chapter generation
                           pipeline, empowerment engine (craft library, prompt compiler,
                           quality evaluator, targeted revision), writer agent kernel,
                           memory (canon, promises, craft feedback), brain service,
                           supervised sprint, storage
forge-agent-mcp/          MCP stdio server — main.rs (JSON-RPC core), dispatch.rs
                           (tool dispatch), tools.rs (91 tool definitions), smoke tests
plugins/forge-writer-agent/ Codex plugin bundle and skill metadata
scripts/                  Windows launchers and eval runner
docs/                     Protocol contract (CONTEXT_CONTRACT.md), design specs, plans
config/                   Runtime configuration — craft library, LLM profiles,
                           anchor heuristics, token calibration
fixtures/writing_eval/    Regression eval fixture — project.json, eval tasks, runner
```

## Requirements

- Rust stable toolchain
- Windows PowerShell or `cmd` for the bundled launch scripts
- Optional: `OPENAI_API_KEY` or a stored provider key for model-backed tools

Forge Agent is headless-only. The supported runtime is the MCP stdio server backed by `HeadlessBackend`; there is no Tauri desktop runtime or UI build target.

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

### Tool Categories

91 MCP tools across 13 categories:

| Category | Tools |
|----------|-------|
| Protocol/project | `forge_manifest`, `forge_project_manifest`, `forge_project_paths` |
| Agent/kernel | `forge_ask_agent`, `forge_agent_tools`, `forge_effective_tool_inventory`, `forge_agent_kernel_status`, `forge_agent_domain_profile`, `forge_status` |
| Chapters/generation | `forge_list_chapters`, `forge_create_chapter`, `forge_load_chapter`, `forge_save_chapter`, `forge_chapter_revision`, `forge_rename_chapter_file`, `forge_generate_chapter_autonomous`, `forge_batch_generate_chapter`, `forge_repair_chapter_state` |
| Generation resume | `forge_latest_chapter_generation_checkpoint`, `forge_chapter_generation_resume_candidates`, `forge_resume_chapter_generation` |
| Lore/outline/book | `forge_lorebook`, `forge_save_lore_entry`, `forge_delete_lore_entry`, `forge_outline`, `forge_save_outline_node`, `forge_delete_outline_node`, `forge_update_outline_status`, `forge_reorder_outline_nodes`, `forge_list_volumes`, `forge_save_volume`, `forge_delete_volume`, `forge_get_volume_snapshot`, `forge_save_volume_snapshot`, `forge_get_book_state`, `forge_save_book_state` |
| Writer memory | `forge_observe`, `forge_ledger`, `forge_today_five`, `forge_pending_proposals`, `forge_story_review_queue`, `forge_story_debt`, `forge_reader_compensation_review_chain`, `forge_trace`, `forge_inspector_timeline`, `forge_companion_timeline_summary` |
| Proposals/operations | `forge_apply_feedback`, `forge_record_implicit_ghost_rejection`, `forge_approve_writer_operation`, `forge_record_writer_operation_durable_save`, `forge_ambient_entity_hints` |
| Model-backed helpers | `forge_analyze_chapter`, `forge_generate_parallel_drafts`, `forge_analyze_pacing`, `forge_ask_project_brain`, `forge_run_metacognitive_recovery` |
| Project Brain | `forge_project_brain_knowledge_graph`, `forge_compare_project_brain_source_revisions`, `forge_restore_project_brain_source_revision`, `forge_cross_reference_brain_nodes`, `forge_ingest_external_research` |
| Supervised sprint | `forge_start_sprint`, `forge_sprint_plan`, `forge_sprint_progress`, `forge_pause_sprint`, `forge_resume_sprint`, `forge_cancel_sprint`, `forge_checkpoint_sprint`, `forge_record_sprint_budget_usage`, `forge_set_sprint_quality_gate` |
| Quality & craft | `forge_craft_library`, `forge_craft_memory_stats`, `forge_eval_trend_summary`, `forge_record_manual_craft_edit_feedback`, `forge_chapter_quality_report`, `forge_context_quality_report`, `forge_budget_calibration`, `forge_execution_plan` |
| World bible | `forge_list_world_assets`, `forge_approve_world_asset`, `forge_reject_world_asset`, `forge_world_bible_constraint_query` |
| Diagnostics | `forge_project_graph_data`, `forge_project_storage_diagnostics`, `forge_export_writer_agent_trajectory`, `forge_export_diagnostic_logs`, `forge_list_file_backups`, `forge_restore_file_backup` |
| Settings | `forge_set_api_key`, `forge_check_api_key` |

### Response Envelope

All `tools/call` responses use the stable Forge envelope in `result.structuredContent`:

```json
{
  "ok": true,
  "data": {},
  "error": null
}
```

Backend failures return `ok: false` with `error.kind` (`backend` | `validation` | `provider` | `permission` | `budget` | `context_overflow` | `storage`) and `error.message`. JSON-RPC parse, invalid request, and unknown method failures use standard JSON-RPC error responses.

For caller context requirements, budget approval rules, revision safety, error classification, quality pipeline details, and write-sensitive scheduling, see [docs/CONTEXT_CONTRACT.md](docs/CONTEXT_CONTRACT.md).

## Architecture

### Crate Map

```
agent-harness-core (239 tests)
├── AgentLoop (run + run_with_plan with plan/step events)
├── ExecutionPlan (compile_plan, Stop/Retry/Skip)
├── Intent Router (weighted scoring + confidence + fallback)
├── Tool Registry + Executor (permission, doom-loop detection)
├── Context Quality (4-dim scoring)
├── Budget Calibration (EMA rolling token-per-char)
├── Recovery (FailureBundle + classify_failure)
├── Compaction (water-level + event-driven)
├── VectorDB (BM25 + cosine hybrid search)
└── Provider (OpenAI-compat streaming + retry)

agent-writer-backend (396 lib tests + 39 eval tests)
├── HeadlessBackend (all MCP action dispatch)
├── Chapter Generation Pipeline
│   ├── Context assembly (async, parallelization-ready)
│   ├── Craft Prompt Compiler (scene-type inference)
│   ├── SceneCraftPlan (pre-writing plan artifact)
│   ├── ChapterQualityReport (8 metrics + evidence gating)
│   └── Targeted Revision (build_revision_prompt + auto-trigger)
├── Writer Agent Kernel (run_loop, proposals, memory, trace)
├── Craft Memory (SQLite feedback tables + compiler learning)
├── Supervised Sprint (quality gate + budget accounting)
├── Brain Service (embedding, retrieval, graph)
└── Storage (SQLite project persistence)

forge-agent-mcp (14 unit tests + 7 smoke tests)
├── main.rs (JSON-RPC core, 627 lines)
├── dispatch.rs (call_tool + call_backend_action, 320 lines)
├── tools.rs (91 tool definitions, 1034 lines)
└── smoke_test.rs (7 process-level integration tests)
```

## Development

Run the full workspace checks:

```powershell
cargo fmt --check
cargo check --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Individual crate checks:

```powershell
cargo test -p agent-harness-core        # 239 tests
cargo test -p agent-writer --lib        # 396 lib tests
cargo test -p agent-writer --test writing_eval_test  # 39 eval tests
cargo test -p forge-agent-mcp           # 14 unit + 7 smoke
```

The headless path does not require Tauri config, icons, generated desktop schemas, renderer assets, or desktop runtime dependencies.

## Configuration

| File | Purpose |
|------|---------|
| `config/craft-library.json` | 8 core writing craft rules for the Prompt Compiler |
| `config/llm-request-profiles.json` | 11 LLM call profiles (general, json, draft, revision, etc.) |
| `config/anchor-carry-heuristics.json` | Sentence-level anchor mode detection |
| `config/token-calibration.json` | Per-model token-per-char calibration seed |

## Plugin

The Codex plugin bundle lives at `plugins/forge-writer-agent/`. It exposes the MCP server named `forge-writer-agent` and defaults data storage to `.forge-agent-data` unless `FORGE_AGENT_DATA_DIR` is set.

## Security

Do not commit local project data, provider keys, or logs. See [SECURITY.md](SECURITY.md) for vulnerability reporting and operational guidance.

## Contributing

Contributions should preserve the MCP contract, keep the headless path buildable in a clean clone, and include focused tests for protocol or backend behavior changes. See [CONTRIBUTING.md](CONTRIBUTING.md).

## License

This project is currently unlicensed. Do not redistribute or reuse the code outside the permissions granted by the repository owner.
