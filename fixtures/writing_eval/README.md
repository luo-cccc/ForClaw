# Writing Eval Fixtures

Multi-profile regression testing for Forge Agent writing quality.

## Profiles

- `xianxia/` — Chinese xianxia novel (4 outline nodes, 2 drafted chapters, 5 lore entries, canon rules, open promises, 18 eval tasks including negative cases)
- `mystery/` — Detective/investigation story (3 outline nodes, 2 drafted chapters, 5 lore entries, canon rules, open promises, 15 eval tasks including negative cases)
- `scifi/` — Sci-fi corporate thriller (3 outline nodes, 2 drafted chapters, 5 lore entries, canon rules, open promises, 15 eval tasks including negative cases)

## Total Coverage

- **48 eval tasks** across 3 profiles
- Task types: chapter_generation, quality_evaluation, quality_signals, targeted_revision, craft_memory, manual_craft_edit, craft_memory_prompt, canon_conflict, planning_review, promise_progression, continuity_diagnostic
- Negative cases: missing_anchor, style_drift, promise_stalled, revision_no_change, craft_memory_injection

## Running

```powershell
# Full matrix (all 48 tasks)
.\scripts\run-writing-eval.cmd

# Smoke mode (representative subset, faster feedback)
.\scripts\run-writing-eval.cmd --smoke

# Run specific profile(s)
cargo run -p agent-writer --bin eval_runner -- xianxia mystery
```

## Output

Per-profile files (ignored by git):
- `{profile}/eval_output.jsonl` — Per-task run output
- `{profile}/eval_trend.json` — Cross-run trend report with regression detection
- `eval_summary.json` — Aggregate summary across all profiles

## Regression Detection

The eval runner fails (exit code 1) when:
- Any task fails its assertions
- A previously passing task regresses to fail/skip
- Average metric score drops by more than 0.05
- Craft rule average score delta drops by more than 0.05

## Automated Tests

```powershell
cargo test -p agent-writer --test writing_eval_test
```

## Adding New Eval Tasks

1. Add JSONL lines to `{profile}/eval_tasks.jsonl`
2. Update `project.json` if new chapters/lore/canon/promises are needed
3. Add corresponding test assertions in `tests/writing_eval_test.rs`
4. Run the eval script and verify `eval_trend.json`
