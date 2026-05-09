# Writing Eval Fixtures

Manual eval project for Forge Agent regression testing.

## Contents

- `project.json` — Small Chinese xianxia novel project (3 outline nodes, 2 drafted chapters, 5 lore entries)
- `eval_tasks.jsonl` — 9 eval task definitions covering generation, continuity, per-metric quality checks, targeted revision, craft memory, and manual author-edit feedback

## Running

```powershell
.\scripts\run-writing-eval.cmd
```

## Automated tests

The following test suites exercise the eval-relevant modules:

```powershell
cargo test -p agent-writer --lib chapter_generation::craft_quality_tests
cargo test -p agent-writer --lib chapter_generation::craft_prompt_tests
cargo test -p agent-writer --lib writer_agent::anchor_carry
cargo test -p agent-writer --lib writer_agent::diagnostics
```

## Adding new eval tasks

1. Add a JSONL line to `eval_tasks.jsonl`
2. Run the eval script
3. Compare output JSONL to expected values
