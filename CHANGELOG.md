# Changelog

All notable changes to this project will be documented in this file.

This project follows a simple human-readable changelog. Versions are created when repository tags are published.

## Unreleased

### M1: Observability Baseline
- Structured error kind classification (`backend` | `validation` | `provider` | `permission` | `budget` | `context_overflow` | `storage`).
- MCP process smoke tests (7 scenarios with isolated temp data dir).
- Real TTFT capture on first TextDelta in streaming callback.
- AgentLoop trace events: `ToolInventory`, `ProviderGuard`, `ContextWindow`.
- Per-call provider latency tracking (`first_provider_call_ms`, `last_provider_call_ms`).

### M2: Planner-Aware AgentLoop
- `ExecutionPlan` data structures with coarse-grained step plans (preflight/draft/validate/save).
- `AgentLoop::run_with_plan()` with Stop/Retry/Skip failure actions and `max_retries` loop.
- 5 TaskScope plan templates (Chapter, Selection, CursorWindow, Project, default).
- Plan/step trace events: `PlanStarted`, `StepStarted`, `StepCompleted`, `StepFailed`, `PlanCompleted`.
- Writer agent kernel wired to `run_with_plan()` via `compile_execution_plan()`.

### M3: Context Quality + Budget Calibration
- `ContextQualityReport` with 4 dimensions: source coverage, truncation risk, grounding quality, overall score.
- `BudgetCalibration` with EMA rolling token-per-char estimation, global `LazyLock<Mutex<CalibrationStore>>`.
- `ContextQualityRecommendation`: Sufficient / Supplement / Critical.
- `record_usage()` and `estimate_tokens()` wired into AgentLoop Complete event.

### Empowerment Engine MVP
- Craft Library: `config/craft-library.json` with 8 core writing craft rules.
- Prompt Compiler: scene type inference (dialogue/action/revelation/turning-point), greedy rule selection with token budget.
- SceneCraftPlan: pre-writing plan with conflict pressure, character choice, emotional curve, ending hook. Persisted as `craft_plan.json` artifact.
- ChapterQualityReport: 8 heuristic metrics (dialogue function, exposition ratio, ending hook, scene causality, promise progress, anchor carry, style drift, length compliance) with mandatory evidence gating.

### Targeted Revision
- `build_revision_prompt()` constructs directed revision prompt from QualityReport top issues.
- `LlmRequestProfile::ChapterTargetedRevision` (temperature 0.3, maxTokens 4096).
- Pipeline auto-trigger: revision runs when major/fatal issues detected, replaces draft only if quality improves.

### Craft Memory
- 3 SQLite tables: `craft_rules`, `craft_examples`, `craft_bad_patterns`.
- `CraftRuleStats` with acceptance rate calculation.
- `record_craft_accept()` / `record_craft_reject()` for feedback learning.
- Compiler adjusts rule priority by acceptance rate via `BuiltChapterContext.craft_rule_stats`.

### Sprint Quality Gate
- `SupervisedSprintPlan` fields: `minimum_quality_score` (default 0.4), `stop_on_fatal_issue` (default true).
- `check_sprint_quality_gate()` blocks sprint advancement when quality below threshold.
- New MCP tool: `forge_set_sprint_quality_gate`.

### M4: Recovery + Eval
- `FailureBundle` with `classify_failure()` mapping errors to `RecoveryAction` (Retry/ShrinkContext/ApprovalRequired/Stop).
- `FailureBundle` trace event emitted in `run_with_plan()` error paths.
- Writing eval harness: fixture project (`project.json`), JSONL task definitions, and regression assertions covering 39 `writing_eval_test` cases.

### Protocol & Entrypoint
- `forge-agent-mcp/src/main.rs` split from 1963 → 627 lines. Extracted `dispatch.rs` (320 lines) and `tools.rs` (1034 lines).
- 6 new MCP tools: `forge_craft_library`, `forge_craft_memory_stats`, `forge_chapter_quality_report`, `forge_context_quality_report`, `forge_budget_calibration`, `forge_execution_plan`.
- CONTEXT_CONTRACT.md updated with error kinds, observability tools, quality pipeline documentation.
- Intent router enhanced with weighted scoring, confidence, evidence, and fallback classification.
- `build_chapter_context` converted to async for future parallelization.

### Test Coverage
- 656 total tests (`agent-harness-core` 239 + `agent-writer` lib 396 + `writing_eval_test` 39 + `forge-agent-mcp` 14 unit + 7 smoke).
- Zero clippy warnings. Zero `unsafe` blocks. Zero regressions.
