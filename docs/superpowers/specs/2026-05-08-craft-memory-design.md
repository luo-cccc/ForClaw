# Craft Memory Design

## Summary

让 Prompt Compiler 从修订反馈中学习——被多次接受的技法下次优先选，被多次拒绝的技法降权。新增 3 个 SQLite 表 + 3 个 Rust 方法。

## Decisions

| 决策 | 选择 |
|------|------|
| 存储 | SQLite（复用 rusqlite） |
| 数据结构 | 3 表：craft_rules / craft_examples / craft_bad_patterns |
| Compiler 接入 | 读取 accept/reject 统计，调整技法优先级 |

## Data Model

```sql
CREATE TABLE craft_rules (
    id TEXT PRIMARY KEY,
    rule_id TEXT NOT NULL,
    scope TEXT NOT NULL DEFAULT '',
    accepted_count INTEGER NOT NULL DEFAULT 0,
    rejected_count INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE craft_examples (
    id TEXT PRIMARY KEY,
    excerpt_ref TEXT NOT NULL,
    reason TEXT NOT NULL DEFAULT '',
    pattern TEXT NOT NULL DEFAULT '',
    scene_types TEXT NOT NULL DEFAULT ''
);

CREATE TABLE craft_bad_patterns (
    id TEXT PRIMARY KEY,
    pattern TEXT NOT NULL,
    correction TEXT NOT NULL DEFAULT '',
    rejected_count INTEGER NOT NULL DEFAULT 0
);
```

## Rust Types

```rust
pub struct CraftRuleStats {
    pub rule_id: String,
    pub accepted_count: u32,
    pub rejected_count: u32,
    pub acceptance_rate: f32,
}
```

## Methods (in writer_agent/memory/)

```rust
fn record_craft_accept(conn: &Connection, rule_id: &str, scope: &str)
fn record_craft_reject(conn: &Connection, rule_id: &str, scope: &str)
fn get_craft_rule_stats(conn: &Connection, rule_id: &str) -> Option<CraftRuleStats>
```

## Compiler Integration

In `compile_empowerment_prompt()`, before selecting rules, read stats:

```rust
let stats = get_craft_rule_stats(conn, &rule.id);
let acceptance_boost = stats.map(|s| s.acceptance_rate - 0.5).unwrap_or(0.0);
let adjusted_priority = base_priority + (acceptance_boost * 5.0) as u8;
```

- accepted/rejected 都为 0 → 不调整（首次使用）
- acceptance_rate > 0.5 → 升权
- acceptance_rate < 0.5（被多次拒绝）→ 降权

## Files

| 文件 | 操作 | 职责 |
|------|------|------|
| `agent-writer-backend/src/writer_agent/memory/craft_memory.rs` | 创建 | 3 个方法 + CraftRuleStats |
| `agent-writer-backend/src/storage.rs` | 修改 | 新增 migration（3 张表） |
| `agent-writer-backend/src/writer_agent/memory/mod.rs` | 修改 | 注册 craft_memory 子模块 |
| `agent-writer-backend/src/chapter_generation/craft_prompt.rs` | 修改 | Compiler 读取 stats 调整优先级 |

## Acceptance

```powershell
cargo fmt --check && cargo check --workspace && cargo clippy --workspace --all-targets -- -D warnings
cargo test -p agent-harness-core && cargo test -p agent-writer --lib && cargo test -p forge-agent-mcp
```

- 新表 migration 在打开新项目时自动创建
- `record_craft_accept` / `record_craft_reject` upsert 计数
- `get_craft_rule_stats` 对未知规则返回 None
- Compiler 用 0/0 统计时不调整优先级
- 所有已有测试不回退
