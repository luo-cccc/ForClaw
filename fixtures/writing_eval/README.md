# Writing Eval Fixtures

Multi-profile regression testing for Forge Agent writing quality.

## Profiles

- `xianxia/` — Chinese xianxia novel (4 outline nodes, drafted multi-chapter fixture, lorebook, canon rules, promises, 53 eval tasks)
- `mystery/` — Detective/investigation story (drafted multi-chapter fixture, lorebook, canon rules, promises, 44 eval tasks)
- `scifi/` — Sci-fi corporate thriller (drafted multi-chapter fixture, lorebook, canon rules, promises, 44 eval tasks)

## Total Coverage

- **141 eval tasks** across 3 profiles
- Task types: chapter_generation, quality_evaluation, quality_signals, targeted_revision, craft_memory, manual_craft_edit, craft_memory_prompt, canon_conflict, canon_constraint, canon_forbidden_claim, canon_required_cost, canon_proposed_not_hard, continuity_diagnostic, extraction, hierarchy_confusion, planning_review, promise_progression, scene_contract_prompt, state_delta_trace, state_regression, unsupported_world_claim, world_asset_contract
- Negative cases: `negative_missing_anchor`, `negative_style_drift`, `negative_plot_stalled`, `negative_promise_stalled`, `negative_revision_no_change`, `negative_craft_memory_injection`

## Running

```powershell
# Full matrix (all 141 tasks)
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
