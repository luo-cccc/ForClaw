# Forge Agent Context Contract

This document defines the context boundary for MCP clients and schedulers. The MCP layer must pass intent, active editor state when available, and approval metadata; the Forge backend owns durable project context assembly from local storage and memory.

## Boundary

MCP callers do not build the final prompt. They provide a request envelope. Forge resolves the active project from `FORGE_AGENT_DATA_DIR`, reads chapters/lore/outline/Project Brain/writer memory, assembles the task-specific context pack, applies budget rules, records trace evidence, and returns structured telemetry.

Renderer context is not required. Headless callers may omit UI-only state, but they must not pretend the document is clean or current when they have unsaved text.

## Common Caller Fields

Use camelCase JSON fields.

- `chapterTitle`: title of the active chapter when the request is about a chapter.
- `chapterRevision`: content revision known by the caller. Use `forge_chapter_revision` before write-sensitive calls.
- `context`: full current editor text when available. For manual agent calls this is split around `cursorPosition` into prefix/suffix and hashed into the observation.
- `cursorPosition`: character index in `context`, not byte offset.
- `paragraph`: paragraph or local passage under inspection. If omitted for `forge_ask_agent`, Forge falls back to `selectedText`, then `message`.
- `selectedText`: selected text for rewrite, review, or targeted questions.
- `dirty`: true when caller has unsaved editor text. Write-sensitive generation uses this to avoid replacing a dirty open chapter.
- `providerBudgetApproval`: explicit approval for model spend when a previous response reports that approval is required.

## Manual Agent Contract

Tool: `forge_ask_agent`

Minimum:

```json
{
  "message": "检查这一段是否破坏前文承诺"
}
```

Full context-aware form:

```json
{
  "message": "基于当前章节，指出接下来最危险的连续性问题",
  "chapterTitle": "第十二章",
  "chapterRevision": "known-revision",
  "context": "full chapter text",
  "cursorPosition": 1200,
  "paragraph": "current paragraph",
  "selectedText": "",
  "dirty": false
}
```

Forge converts this into a `ManualRequest` observation, builds a `WritingContextPack`, and returns `run.contextPackSummary`, `run.sourceRefs`, `events`, proposals, typed operations, and provider budget data. The caller should treat typed operations as proposals unless it separately calls `forge_approve_writer_operation` with an approval context.

## Chapter Generation Contract

Tools: `forge_generate_chapter_autonomous`, `forge_batch_generate_chapter`

Minimum autonomous request:

```json
{
  "userInstruction": "生成下一章，承接上一章结尾的危机"
}
```

Write-safe autonomous request:

```json
{
  "requestId": "scheduler-20260508-001",
  "targetChapterTitle": "第十二章",
  "userInstruction": "按大纲生成本章，只写正文",
  "frontendState": {
    "openChapterTitle": "第十二章",
    "openChapterRevision": "known-revision",
    "dirty": false
  },
  "saveMode": "replace_if_clean",
  "chapterContract": {
    "targetChars": 3500,
    "minChars": 3000,
    "maxChars": 4000,
    "saveHardFloorChars": 2800,
    "saveHardCeilingChars": 4300
  }
}
```

`targetChapterTitle` or `targetChapterNumber` selects the chapter. If both are omitted, Forge resolves from outline context when possible. `chapterSummaryOverride` can override the outline summary for this run only.

`frontendState` is a safety contract:

- `openChapterTitle` identifies the caller's open document.
- `openChapterRevision` must match the stored revision for `replace_if_clean`.
- `dirty: true` blocks direct replacement of that open target.

`saveMode` values:

- `replace_if_clean`: default; save only when revision and dirty checks pass.
- `create_if_missing`: create the target chapter when absent.
- `save_as_draft`: write a draft copy on conflicts instead of replacing the target.

Forge builds chapter context from instruction, outline, adjacent chapters, target existing text, lorebook, Project Brain, author/user profile, writer memory, story contract, promises, canon, decisions, result feedback, and impact-scoped sources. It returns generation events including `sources`, `budget`, `receipt`, `selectedEvidence`, `ruleStack`, `lengthTelemetry`, `settlementDelta`, `settlementApply`, `artifactRefs`, `saved`, and conflict/error details when applicable.

## Context Budgets

Manual agent tasks use internal task budgets from `AgentTask`:

- `ManualRequest`: 4500 chars
- `InlineRewrite`: 4500 chars
- `PlanningReview`: 6000 chars
- `ContinuityDiagnostic`: 2500 chars
- `ChapterGeneration`: 20000 chars inside the writer kernel

Chapter generation has a separate `budget` object with defaults:

```json
{
  "totalChars": 24000,
  "instructionChars": 1000,
  "outlineChars": 6000,
  "previousChaptersChars": 5000,
  "nextChapterChars": 2000,
  "targetExistingChars": 3000,
  "lorebookChars": 5000,
  "userProfileChars": 4000,
  "ragChars": 4000,
  "previousChapterCount": 2,
  "nextChapterCount": 1,
  "lorebookEntryCount": 4,
  "userProfileEntryCount": 6,
  "ragChunkCount": 5
}
```

The caller may lower these values for cheaper scheduled runs. Raising them can trigger `providerBudgetApproval` requirements.

## Budget Approval Contract

When a model-backed tool needs approval, the response includes provider budget data or an error evidence bundle. Approval is valid only when it covers the same task, model, total tokens, and estimated cost.

```json
{
  "providerBudgetApproval": {
    "task": "manual_request",
    "model": "deepseek/deepseek-v4-flash",
    "approvedTotalTokens": 64000,
    "approvedCostMicros": 100000,
    "approvedAtMs": 1778220000000,
    "source": "scheduler"
  }
}
```

Use the exact `task` and `model` reported by Forge. A smaller approval does not cover a larger retry.

## Stable Response Contract

All successful `tools/call` JSON-RPC responses return a standard MCP tool result. The stable Forge business envelope is in `result.structuredContent`:

```json
{
  "ok": true,
  "data": {},
  "error": null
}
```

For backend failures:

```json
{
  "ok": false,
  "data": null,
  "error": {
    "kind": "backend",
    "message": "..."
  }
}
```

JSON-RPC parse, invalid request, and unknown method failures use JSON-RPC error responses rather than this tool envelope.

The MCP result also includes human-readable `result.content` and `result.isError`. Callers that need machine-readable state should read `result.structuredContent.ok`, `result.structuredContent.data`, and `result.structuredContent.error`.

Tool annotations in `tools/list` are scheduling hints only. Callers must still use the context and approval rules in this document for write-sensitive actions.

## Caller Rules

1. Read `forge_manifest` and `tools/list` at startup.
2. Use `forge_project_paths` and `forge_project_manifest` for project identity, not hard-coded paths.
3. Use `forge_chapter_revision` before write-sensitive chapter calls.
4. Set `dirty: true` when unsaved editor text exists.
5. Do not send renderer-only state; Forge Agent's MCP contract is backend-only.
6. Prefer specific tools for discoverability; use `forge_backend_call` for scheduler implementations that need one stable dispatch surface.
7. Treat returned proposals and operations as pending until explicitly approved and durably saved.

## Error Kinds

All `tools/call` structured errors include an `error.kind` field for programmatic handling:

| Kind | Meaning | Example |
|------|---------|---------|
| `validation` | Invalid parameters or missing required fields | `chapterTitle is required` |
| `provider` | LLM provider failure (HTTP errors, rate limits, timeouts) | `LLM call failed (429)` |
| `permission` | Tool denied by permission policy | `Tool requires explicit approval` |
| `budget` | Budget ceiling or approval limit exceeded | `Provider budget exceeded` |
| `context_overflow` | Request would exceed model context window | `Context overflow after tool schema expansion` |
| `storage` | Durable project storage failure | `Storage read failed` |
| `backend` | All other internal errors | `Internal dispatch failed` |

Callers should use `error.kind` (not `error.message`) for retry/recovery logic.

## Observability Tools (M1-M4)

Tools added for capability inspection and quality management:

| Tool | Purpose |
|------|---------|
| `forge_craft_library` | List all 8 craft rules with categories, instructions, and diagnostic signals |
| `forge_craft_memory_stats` | Query craft rule acceptance/rejection statistics from feedback memory |
| `forge_chapter_quality_report` | Retrieve the most recent ChapterQualityReport for a chapter |
| `forge_context_quality_report` | Retrieve the ContextQualityReport from the last preflight |
| `forge_budget_calibration` | Query current token-per-char calibration for a model |
| `forge_execution_plan` | Inspect the active ExecutionPlan step status and progress |
| `forge_set_sprint_quality_gate` | Configure sprint quality thresholds |

All follow the standard `{ok, data, error}` envelope.

## Quality Pipeline (Writer Agent)

Chapter generation now includes automated quality evaluation and targeted revision:

1. **Quality evaluation** — After draft generation, `evaluate_chapter_quality()` runs 8 heuristic metrics (dialogue function, exposition ratio, ending hook, scene causality, anchor carry, style drift, length compliance, promise progress). Results stored in `PipelineTerminal::Completed`.

2. **Targeted revision** — When major/fatal issues are detected, `build_revision_prompt()` constructs a directed revision prompt targeting the top 3 issues. The revision uses `LlmRequestProfile::ChapterTargetedRevision` (temperature 0.3, maxTokens 4096). Revised text replaces the draft only if quality score improves.

3. **Craft prompt** — Each chapter draft system prompt is augmented with selected craft rules from the `craft-library.json` library (8 rules covering scene objective, conflict pressure, dialogue function, setting integration, emotional externalization, promise advancement, ending hooks, and genre pleasure).

4. **SceneCraftPlan** — A pre-writing plan artifact (`craft_plan.json`) is generated and saved alongside each chapter generation.

## Protocol Version

```json
{
  "protocolVersion": "2025-11-25",
  "supportedProtocolVersions": ["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"]
}
```

The MCP server identifies as `forge-writer-agent` with transport `stdio-jsonrpc-lines`. All new tools respect the existing protocol version.
