---
name: forge-writer-agent
description: Use the local Forge writer-agent MCP backend for chapter storage, story memory, proposals, ledgers, traces, and supervised sprint state.
---

# Forge Writer Agent

Use this skill when the user asks to route writing-agent work through the Forge backend, inspect Forge memory, manage chapters, or schedule the writer agent as an MCP plugin.

The plugin exposes one MCP server named `forge-writer-agent`. Prefer specific MCP tools such as `forge_manifest`, `forge_project_paths`, `forge_ask_agent`, `forge_status`, `forge_ledger`, `forge_save_chapter`, `forge_generate_chapter_autonomous`, `forge_repair_chapter_state`, `forge_analyze_chapter`, and `forge_start_sprint`. Use `forge_backend_call` when a host wants a single generic dispatch surface.

The backend stores local project data under `FORGE_AGENT_DATA_DIR`. If it is not set, the launcher defaults to `.forge-agent-data` under the repository root.

For scheduled or plugin-driven calls, follow `docs/CONTEXT_CONTRACT.md`: pass caller intent, active chapter state, revision, dirty flag, and budget approval metadata; let Forge assemble durable project context from storage, memory, Project Brain, outline, lore, and chapter files.

This is a backend-only agent plugin for the Forge Agent project. It exposes the writer kernel, memory, storage, provider budget, Project Brain management, metacognitive recovery, chapter-generation pipeline, chapter-state repair, and headless model-backed agent loop. The MCP entrypoint runs `cmd /c scripts\forge-agent-mcp.cmd stdio` from the repository root, which launches the compiled `forge-agent-mcp.exe` over newline-delimited JSON-RPC stdio and keeps logs on stderr. Desktop UI and renderer hooks are outside the plugin contract.
