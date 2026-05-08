# M1: Observability Baseline Design

## Summary

为 Forge Agent Kernel 建立可观测性基线：结构化错误分类、进程级 MCP smoke test、真实 TTFT 指标、增强 run trace 事件。四个子任务按序交付，每步独立可验证。

## Decisions

| 决策点 | 选择 |
|--------|------|
| 交付顺序 | 按子任务逐个（error kind → smoke test → TTFT → trace） |
| Error kind 粒度 | 4 种高发类：backend / validation / provider / permission |
| Smoke test 范围 | 6 个关键场景，用临时 FORGE_AGENT_DATA_DIR |
| TTFT 方案 | streaming 回调中捕获首个 TextDelta 时间戳 |
| Trace 增强 | 4 个新事件：ToolInventory / ProviderGuard / ContextWindow / Compaction 增强 |

---

## Task 1: Structured Error Kind

### Problem

当前 `tool_error_result` 所有错误都标记 `kind=backend`，MCP caller 无法区分参数错误、provider 失败和权限拒绝，调度器无法做差异化重试。

### Design

新增 `ErrorKind` 枚举，改造 MCP 错误信封：

```rust
#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum ErrorKind {
    Backend,
    Validation,
    Provider,
    Permission,
}
```

**改动文件：**

1. **`forge-agent-mcp/src/main.rs`**
   - 新增 `ErrorKind` 枚举
   - `tool_error_result` 签名改为 `(kind: ErrorKind, message: String) -> Value`
   - 所有 `tool_error_result` 调用点按来源分类

2. **`agent-writer-backend/src/headless.rs`**
   - `dispatch()` 返回值增加 error kind 信息
   - 新增 `BackendResult<T>` = `Result<T, (ErrorKind, String)>`
   - 参数校验失败返回 `Validation`
   - 存储错误返回 `Backend`

3. **`agent-harness-core/src/provider/openai_compat.rs`**
   - 流式/非流式调用失败的错误字符串保持，不改变 provider trait
   - 分类在 MCP 层完成：HTTP 4xx/5xx 匹配 → Provider

**分类规则：**

- `Validation` ← `serde_json::from_value` 失败、`chapterTitle is required`、`action is required`、`Invalid tool call`
- `Provider` ← `LLM call failed`、`stream read error`、`HTTP request failed`、`rate limit`
- `Permission` ← `requires explicit approval`、`write access but agent is in ReadOnly`、sensitive path deny
- `Backend` ← 所有其他内部错误

**向后兼容：** `error.kind` 为新增字段，`error.message` 不变。不消费 kind 的 caller 行为不变。

### Response Envelope After

```json
{
  "content": [{"type": "text", "text": "..."}],
  "structuredContent": {
    "ok": false,
    "data": null,
    "error": {
      "kind": "validation",
      "message": "chapterTitle is required"
    }
  },
  "isError": true
}
```

---

## Task 2: MCP Process Smoke Test

### Problem

当前只有单元测试，没有验证 stdio 进程、JSON-RPC 协议握手、真实二进制可启动的进程级测试。

### Design

**新增文件：** `forge-agent-mcp/tests/smoke_test.rs`

**方式：** `std::process::Command` 启动 `forge-agent-mcp stdio` 子进程，stdin 写 JSON-RPC line，stdout 读 response line。

**环境：** `FORGE_AGENT_DATA_DIR` 指向 `tempfile::TempDir`，不依赖真实用户数据。

**二进制定位：** `env!("CARGO_BIN_EXE_forge-agent-mcp")` — `cargo test` 自动构建。

### Test Cases

| # | 测试 | 发 | 核心验证 |
|---|------|-----|---------|
| 1 | initialize | `{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}` | `protocolVersion` 存在，`serverInfo.name = "forge-writer-agent"` |
| 2 | tools/list | `{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}` | `tools` 非空数组，含 `forge_status`、`forge_backend_call` |
| 3 | forge_status | tools/call → `forge_status` | `structuredContent.ok = true` |
| 4 | forge_backend_call(status) | tools/call → `forge_backend_call` action=status | 同 forge_status |
| 5 | forge_list_chapters (空项目) | tools/call → `forge_list_chapters` | `structuredContent.ok = true`，空项目不报错 |
| 6 | forge_agent_domain_profile | tools/call → `forge_agent_domain_profile` | 返回 longform_writing domain profile |

---

## Task 3: Fix Provider Latency Metrics

### Problem

`AgentLoop` 中 `ttft_ms` 记录的是整个 `stream_call` 耗时，不是真正的 time-to-first-token。

### Design

在 streaming 回调中捕获第一个 `TextDelta` 的时间戳：

```rust
let call_start = Instant::now();
let ttft = Arc::new(Mutex::new(None::<u64>));
let ttft_ref = ttft.clone();

// 在 on_event 回调中，第一个 TextDelta 时记录 TTFT
let on_event = move |ev| {
    if let StreamEvent::TextDelta { .. } = &ev {
        let mut ttft = ttft_ref.lock().unwrap();
        if ttft.is_none() {
            *ttft = Some(call_start.elapsed().as_millis() as u64);
        }
    }
    // 转发给原始 callback...
};

let response = self.provider.stream_call(request, on_event).await?;
let call_duration_ms = call_start.elapsed().as_millis() as u64;
self.total_provider_duration_ms += call_duration_ms;
self.ttft_ms = (*ttft.lock().unwrap()).or(Some(call_duration_ms));
```

**改动文件：** 仅 `agent-harness-core/src/agent_loop.rs`。

**Complete 事件扩展（新增 2 字段）：**
- `first_provider_call_ms: u64` — 本轮首次 provider call 耗时
- `last_provider_call_ms: u64` — 本轮末次 provider call 耗时

---

## Task 4: Enhanced Run Trace Events

### Problem

当前 trace 不记录工具清单摘要、provider guard 决策、context window guard 决策，导致无法复盘"为什么允许或阻止这次调用"。

### Design

**新增 4 个 AgentLoopEvent 变体：**

```rust
ToolInventory { tools: Vec<String>, generation: u64 },
ProviderGuard { allowed: bool, model: String, estimated_input_tokens: u64, requested_output_tokens: u64 },
ContextWindow { tokens: u64, estimated_input: u64, requested_output: u64, should_warn: bool, should_block: bool },
// Compaction 已有，增强 trigger 字段
```

**插入点（在 `run()` 方法中）：**

| 阶段 | 事件 | 位置 |
|------|------|------|
| 工具过滤后 | `ToolInventory` | `build_tools_async()` 返回后 |
| Provider guard 后 | `ProviderGuard` | `provider_call_guard()` 调用后 |
| Context window 检查后 | `ContextWindow` | `evaluate_context_window()` 返回后 |
| Compaction 后 | `Compaction` (增强 trigger) | 已有 emit 点 |

**改动文件：** `agent-harness-core/src/agent_loop.rs` 约 40 行；`AgentLoopEvent` 枚举约 25 行；`run_trace.rs` 的 `AgentRunEventKind` 新增变体。

---

## Files Changed Summary

| 文件 | Task | 改动量 |
|------|------|--------|
| `forge-agent-mcp/src/main.rs` | 1 | 新增 ErrorKind + 改造 tool_error_result 调用点，约 30 行 |
| `agent-writer-backend/src/headless.rs` | 1 | dispatch 返回带 kind 的错误，约 20 行 |
| `forge-agent-mcp/tests/smoke_test.rs` | 2 | 新增文件，约 150 行 |
| `agent-harness-core/src/agent_loop.rs` | 3, 4 | TTFT 修正 + trace 事件 emit，约 80 行 |
| `agent-harness-core/src/run_trace.rs` | 4 | AgentRunEventKind 新增变体，约 5 行 |

## Acceptance Criteria

```powershell
cargo fmt --check
cargo check --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p agent-harness-core
cargo test -p agent-writer --lib
cargo test -p forge-agent-mcp  # 包含 smoke test
```
