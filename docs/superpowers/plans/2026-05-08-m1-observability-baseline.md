# M1: Observability Baseline Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Establish observability baseline for Forge Agent Kernel: structured error classification, MCP process smoke test, real TTFT metrics, and enhanced run trace events.

**Architecture:** Four sequential tasks. Task 1 (error kind) is the foundation — Tasks 2/3/4 are independent of each other but all consume Task 1's error envelope. Each task touches 1-3 files max. All changes stay within existing module boundaries.

**Tech Stack:** Rust, serde_json, tokio, std::process::Command (for smoke test)

---

## File Map

| File | Create/Modify | Responsibility |
|------|--------------|----------------|
| `forge-agent-mcp/src/main.rs` | Modify | ErrorKind enum, tool_error_result signature, call_tool error classification |
| `agent-writer-backend/src/headless.rs` | Modify | dispatch() return type with error kind for validation errors |
| `forge-agent-mcp/tests/smoke_test.rs` | Create | Process-level MCP protocol smoke test |
| `agent-harness-core/src/agent_loop.rs` | Modify | TTFT capture, trace event emit points |
| `agent-harness-core/src/run_trace.rs` | Modify | AgentRunEventKind new variants |

---

### Task 1: Structured Error Kind

**Files:**
- Modify: `forge-agent-mcp/src/main.rs:619-639`
- Modify: `forge-agent-mcp/src/main.rs:260-270,510-523`
- Modify: `agent-writer-backend/src/headless.rs:2384-2388`

- [ ] **Step 1: Add ErrorKind enum in main.rs**

After the `BACKEND_ACTIONS` const array (line 91), insert:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
enum ErrorKind {
    Backend,
    Validation,
    Provider,
    Permission,
}
```

- [ ] **Step 2: Change tool_error_result signature in main.rs**

Replace existing function at line 619-639:

```rust
fn tool_error_result(kind: ErrorKind, message: String) -> Value {
    let structured_content = json!({
        "ok": false,
        "data": Value::Null,
        "error": {
            "kind": kind,
            "message": message,
        }
    });
    json!({
        "content": [
            {
                "type": "text",
                "text": structured_content["error"]["message"]
                    .as_str()
                    .unwrap_or("Backend error")
            }
        ],
        "structuredContent": structured_content,
        "isError": true
    })
}
```

- [ ] **Step 3: Add error classification helper in call_tool**

After the `call_tool` function's `match result` (line 520-523), replace:

```rust
    match result {
        Ok(value) => Ok(tool_result(value, false)),
        Err(error) => Ok(tool_error_result(error)),
    }
```

With:

```rust
    match result {
        Ok(value) => Ok(tool_result(value, false)),
        Err(error) => {
            let kind = classify_error(&call.name, &error);
            Ok(tool_error_result(kind, error))
        }
    }
```

Add the classification function before `call_tool`:

```rust
fn classify_error(tool_name: &str, error: &str) -> ErrorKind {
    let lower = error.to_ascii_lowercase();
    if lower.contains("invalid") && (lower.contains("request") || lower.contains("tool call")) {
        return ErrorKind::Validation;
    }
    if lower.contains("required")
        || lower.contains("must not be empty")
        || lower.contains("missing")
    {
        return ErrorKind::Validation;
    }
    if lower.contains("llm call failed")
        || lower.contains("http request failed")
        || lower.contains("stream read error")
        || lower.contains("rate limit")
        || lower.contains("429")
        || lower.contains("provider")
    {
        return ErrorKind::Provider;
    }
    if lower.contains("approval")
        || lower.contains("permission")
        || lower.contains("denied")
        || lower.contains("read-only")
    {
        return ErrorKind::Permission;
    }
    ErrorKind::Backend
}
```

- [ ] **Step 4: Update call_tool validation errors**

Replace line 517:
```rust
        other => return Err(format!("Unknown tool: {}", other)),
```
With:
```rust
        other => {
            return Ok(tool_error_result(
                ErrorKind::Validation,
                format!("Unknown tool: {}", other),
            ))
        }
```

Then replace the `Err(error)` return which now becomes unreachable for unknown tools — update the `call_tool` return type from `Result<Value, String>` to `Result<Value, Value>`, and change `Err(error)` at line 266 to pass through as `Result<Value, Value>`:

Actually, keeping return type simple: all errors go through `tool_error_result` now. Change `call_tool` signature to return `Result<Value, Value>` (never actually Err):

Wait — the simplest approach: change `call_tool` to always return `Ok(Value)`. Errors become `Ok(tool_error_result(...))`.

In `handle_message`, remove the `Err(error)` handling at line 266 since `call_tool` now always returns `Ok`:

```rust
        "tools/call" => {
            if let Some(id) = message.id {
                let result = call_tool(backend, message.params).await;
                Some(success_response(id, result))
            } else {
                None
            }
        }
```

- [ ] **Step 5: Update call_backend_action similarly**

Apply same pattern to `call_backend_action`: change from `Result<Value, String>` to `Value`, inline error-to-tool_error_result conversion.

- [ ] **Step 6: Verify existing unit tests pass**

```powershell
cargo test -p forge-agent-mcp
```

- [ ] **Step 7: Add error kind unit tests**

Add to `forge-agent-mcp/tests/` or inline in main.rs test module:

```rust
#[test]
fn validation_error_has_correct_kind() {
    let result = tool_error_result(ErrorKind::Validation, "chapterTitle is required".into());
    let sc = &result["structuredContent"];
    assert_eq!(sc["ok"], false);
    assert_eq!(sc["error"]["kind"], "validation");
    assert_eq!(sc["error"]["message"], "chapterTitle is required");
}

#[test]
fn provider_error_has_correct_kind() {
    let result = tool_error_result(ErrorKind::Provider, "LLM call failed (429): rate limited".into());
    let sc = &result["structuredContent"];
    assert_eq!(sc["error"]["kind"], "provider");
}

#[test]
fn classify_error_detects_validation() {
    assert_eq!(classify_error("forge_load_chapter", "chapterTitle is required"), ErrorKind::Validation);
}

#[test]
fn classify_error_detects_provider() {
    assert_eq!(classify_error("forge_ask_agent", "LLM call failed (500)"), ErrorKind::Provider);
}

#[test]
fn classify_error_defaults_to_backend() {
    assert_eq!(classify_error("forge_status", "something unexpected happened"), ErrorKind::Backend);
}
```

- [ ] **Step 8: Run tests and commit**

```powershell
cargo test -p forge-agent-mcp
cargo test -p agent-writer --lib
```

```bash
git add forge-agent-mcp/src/main.rs
git commit -m "feat: add structured error kind classification

Add ErrorKind enum (Backend, Validation, Provider, Permission) and
classify all MCP tool errors. tool_error_result now emits kind field
in structuredContent.error for caller differentiation.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>"
```

---

### Task 2: MCP Process Smoke Test

**Files:**
- Create: `forge-agent-mcp/tests/smoke_test.rs`

- [ ] **Step 1: Create smoke test file**

```rust
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command};

fn spawn_mcp() -> (Child, BufReader<ChildStdout>, ChildStdin) {
    let exe = env!("CARGO_BIN_EXE_forge-agent-mcp");
    let temp_dir = std::env::temp_dir().join("forge-smoke-test");
    let _ = std::fs::remove_dir_all(&temp_dir);

    let mut child = Command::new(exe)
        .arg("stdio")
        .env("FORGE_AGENT_DATA_DIR", &temp_dir)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("failed to start forge-agent-mcp");

    let stdout = BufReader::new(child.stdout.take().unwrap());
    let stdin = child.stdin.take().unwrap();
    (child, stdout, stdin)
}

fn send_request(stdin: &mut ChildStdin, request: &serde_json::Value) {
    let mut line = serde_json::to_string(request).unwrap();
    line.push('\n');
    stdin.write_all(line.as_bytes()).unwrap();
    stdin.flush().unwrap();
}

fn read_response(stdout: &mut BufReader<ChildStdout>) -> serde_json::Value {
    let mut line = String::new();
    stdout.read_line(&mut line).expect("no response from MCP server");
    serde_json::from_str(line.trim()).expect("invalid JSON response")
}

#[test]
fn smoke_initialize() {
    let (mut child, mut stdout, mut stdin) = spawn_mcp();

    send_request(&mut stdin, &serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {}
    }));

    let response = read_response(&mut stdout);
    assert_eq!(response["jsonrpc"], "2.0");
    assert_eq!(response["id"], 1);
    assert!(response["error"].is_null());
    let result = &response["result"];
    assert!(result["protocolVersion"].is_string());
    assert_eq!(result["serverInfo"]["name"], "forge-writer-agent");

    child.kill().ok();
}

#[test]
fn smoke_tools_list() {
    let (mut child, mut stdout, mut stdin) = spawn_mcp();

    send_request(&mut stdin, &serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {}
    }));
    read_response(&mut stdout); // consume

    send_request(&mut stdin, &serde_json::json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/list",
        "params": {}
    }));

    let response = read_response(&mut stdout);
    let tools = response["result"]["tools"].as_array().expect("tools should be array");
    assert!(!tools.is_empty(), "tools list should not be empty");

    let tool_names: Vec<&str> = tools
        .iter()
        .filter_map(|t| t["name"].as_str())
        .collect();
    assert!(tool_names.contains(&"forge_status"), "should have forge_status");
    assert!(tool_names.contains(&"forge_backend_call"), "should have forge_backend_call");
    assert!(tool_names.contains(&"forge_list_chapters"), "should have forge_list_chapters");

    child.kill().ok();
}

#[test]
fn smoke_forge_status() {
    let (mut child, mut stdout, mut stdin) = spawn_mcp();
    initialize(&mut stdin, &mut stdout);

    call_tool(&mut stdin, &mut stdout, "forge_status", serde_json::json!({}));

    let response = read_response(&mut stdout);
    let sc = &response["result"]["structuredContent"];
    assert_eq!(sc["ok"], true);
    assert!(sc["data"]["project"].is_object());

    child.kill().ok();
}

#[test]
fn smoke_forge_backend_call_status() {
    let (mut child, mut stdout, mut stdin) = spawn_mcp();
    initialize(&mut stdin, &mut stdout);

    call_tool(&mut stdin, &mut stdout, "forge_backend_call", serde_json::json!({
        "action": "status",
        "params": {}
    }));

    let response = read_response(&mut stdout);
    let sc = &response["result"]["structuredContent"];
    assert_eq!(sc["ok"], true, "backend call status should succeed");

    child.kill().ok();
}

#[test]
fn smoke_forge_list_chapters_empty_project() {
    let (mut child, mut stdout, mut stdin) = spawn_mcp();
    initialize(&mut stdin, &mut stdout);

    call_tool(&mut stdin, &mut stdout, "forge_list_chapters", serde_json::json!({}));

    let response = read_response(&mut stdout);
    let sc = &response["result"]["structuredContent"];
    assert_eq!(sc["ok"], true, "list chapters should succeed on empty project");

    child.kill().ok();
}

#[test]
fn smoke_forge_agent_domain_profile() {
    let (mut child, mut stdout, mut stdin) = spawn_mcp();
    initialize(&mut stdin, &mut stdout);

    call_tool(&mut stdin, &mut stdout, "forge_agent_domain_profile", serde_json::json!({}));

    let response = read_response(&mut stdout);
    let sc = &response["result"]["structuredContent"];
    assert_eq!(sc["ok"], true);
    assert!(sc["data"]["id"].as_str() == Some("longform_writing"));

    child.kill().ok();
}

// ── helpers ──

fn initialize(stdin: &mut ChildStdin, stdout: &mut BufReader<ChildStdout>) {
    send_request(stdin, &serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {}
    }));
    let _ = read_response(stdout);
}

fn call_tool(stdin: &mut ChildStdin, stdout: &mut BufReader<ChildStdout>, name: &str, args: serde_json::Value) {
    send_request(stdin, &serde_json::json!({
        "jsonrpc": "2.0",
        "id": 99,
        "method": "tools/call",
        "params": {
            "name": name,
            "arguments": args
        }
    }));
}
```

- [ ] **Step 2: Add tempfile dev-dependency to forge-agent-mcp/Cargo.toml**

Actually — no external dep needed. `std::env::temp_dir()` works fine. No Cargo.toml change needed.

- [ ] **Step 3: Run smoke tests**

```powershell
cargo test -p forge-agent-mcp --test smoke_test -- --nocapture
```

- [ ] **Step 4: Commit**

```bash
git add forge-agent-mcp/tests/smoke_test.rs
git commit -m "test: add MCP process smoke test

Spawns forge-agent-mcp stdio, covers initialize, tools/list,
forge_status, forge_backend_call, forge_list_chapters, and
forge_agent_domain_profile with isolated temp data dir.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>"
```

---

### Task 3: Fix Provider Latency Metrics (TTFT)

**Files:**
- Modify: `agent-harness-core/src/agent_loop.rs:289-311`
- Modify: `agent-harness-core/src/agent_loop.rs:53-55` (Complete event)

- [ ] **Step 1: Add first/last call timing fields to Complete event**

Replace the `Complete` variant in `AgentLoopEvent` (line 45-56):

```rust
    #[serde(rename = "complete")]
    Complete {
        rounds: u32,
        tool_calls: u32,
        tokens_used: u64,
        cached_tokens: Option<u64>,
        input_tokens: Option<u64>,
        output_tokens: Option<u64>,
        ttft_ms: Option<u64>,
        total_provider_duration_ms: u64,
        first_provider_call_ms: u64,
        last_provider_call_ms: u64,
    },
```

- [ ] **Step 2: Add tracking fields to AgentLoop struct**

After `total_provider_duration_ms` at line 107, add:

```rust
    pub first_provider_call_ms: u64,
    pub last_provider_call_ms: u64,
```

Initialize in `new()` at line 125-126:

```rust
            total_provider_duration_ms: 0,
            first_provider_call_ms: 0,
            last_provider_call_ms: 0,
```

- [ ] **Step 3: Capture real TTFT in streaming callback**

Replace lines 289-311:

```rust
            // Call LLM with streaming — forward text chunks to UI
            let event_cb = self.on_event.clone();
            let call_start = std::time::Instant::now();
            let ttft_cell = std::sync::Arc::new(std::sync::Mutex::new(None::<u64>));
            let ttft_clone = ttft_cell.clone();
            let response = self
                .provider
                .stream_call(
                    request,
                    Box::new(move |ev| {
                        if let StreamEvent::TextDelta { content } = &ev {
                            let mut ttft = ttft_clone.lock().unwrap();
                            if ttft.is_none() {
                                *ttft = Some(call_start.elapsed().as_millis() as u64);
                            }
                            if let Some(ref cb) = &event_cb {
                                cb(AgentLoopEvent::TextChunk {
                                    content: content.clone(),
                                });
                            }
                        }
                    }),
                )
                .await
                .inspect_err(|e| {
                    self.emit(AgentLoopEvent::Error { message: e.clone() });
                })?;
            let call_duration_ms = call_start.elapsed().as_millis() as u64;
            let ttft_ms = *ttft_cell.lock().unwrap();
            self.total_provider_duration_ms += call_duration_ms;
            self.ttft_ms = ttft_ms.or(Some(call_duration_ms));
            if self.first_provider_call_ms == 0 {
                self.first_provider_call_ms = call_duration_ms;
            }
            self.last_provider_call_ms = call_duration_ms;
            self.last_usage = response.usage.clone();
```

- [ ] **Step 4: Update Complete event emission**

Replace lines 426-435 where `Complete` is emitted:

```rust
        self.emit(AgentLoopEvent::Complete {
            rounds,
            tool_calls: total_tool_calls,
            tokens_used: self.estimate_tokens(),
            cached_tokens: usage.as_ref().and_then(|u| u.cached_tokens),
            input_tokens: usage.as_ref().map(|u| u.input_tokens),
            output_tokens: usage.as_ref().map(|u| u.output_tokens),
            ttft_ms: self.ttft_ms,
            total_provider_duration_ms: self.total_provider_duration_ms,
            first_provider_call_ms: self.first_provider_call_ms,
            last_provider_call_ms: self.last_provider_call_ms,
        });
```

- [ ] **Step 5: Run existing tests**

```powershell
cargo test -p agent-harness-core
cargo clippy -p agent-harness-core --all-targets -- -D warnings
```

- [ ] **Step 6: Add TTFT unit test**

Append to agent_loop.rs test module:

```rust
    #[test]
    fn complete_event_serializes_ttft_fields() {
        let event = AgentLoopEvent::Complete {
            rounds: 3,
            tool_calls: 5,
            tokens_used: 12000,
            cached_tokens: Some(8000),
            input_tokens: Some(10000),
            output_tokens: Some(2000),
            ttft_ms: Some(320),
            total_provider_duration_ms: 4500,
            first_provider_call_ms: 1500,
            last_provider_call_ms: 3000,
        };
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["kind"], "complete");
        assert_eq!(json["ttft_ms"], 320);
        assert_eq!(json["first_provider_call_ms"], 1500);
        assert_eq!(json["last_provider_call_ms"], 3000);
    }
```

- [ ] **Step 7: Run tests and commit**

```powershell
cargo test -p agent-harness-core
cargo clippy -p agent-harness-core --all-targets -- -D warnings
```

```bash
git add agent-harness-core/src/agent_loop.rs
git commit -m "feat: capture real TTFT and per-call provider latency

TTFT is now recorded on first TextDelta in streaming callback.
Added first_provider_call_ms and last_provider_call_ms to
Complete event. Previously ttft_ms recorded total call duration.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>"
```

---

### Task 4: Enhanced Run Trace Events

**Files:**
- Modify: `agent-harness-core/src/agent_loop.rs:13-56` (AgentLoopEvent)
- Modify: `agent-harness-core/src/agent_loop.rs:197-240,247-287` (run method emit points)
- Modify: `agent-harness-core/src/run_trace.rs:14-24` (AgentRunEventKind)

- [ ] **Step 1: Add new AgentLoopEvent variants**

Insert after existing `DoomLoopWarning` at line 32:

```rust
    #[serde(rename = "tool_inventory")]
    ToolInventory {
        tools: Vec<String>,
        generation: u64,
    },
    #[serde(rename = "provider_guard")]
    ProviderGuard {
        allowed: bool,
        model: String,
        estimated_input_tokens: u64,
        requested_output_tokens: u64,
    },
    #[serde(rename = "context_window")]
    ContextWindow {
        tokens: u64,
        estimated_input: u64,
        requested_output: u64,
        should_warn: bool,
        should_block: bool,
    },
```

- [ ] **Step 2: Add new AgentRunEventKind variants**

In `agent-harness-core/src/run_trace.rs`, add to `AgentRunEventKind`:

```rust
    ToolInventoryBuilt,
    ProviderGuardCheck,
    ContextWindowCheck,
```

Insert them after `ContextBuilt`:

```rust
pub enum AgentRunEventKind {
    Started,
    Observation,
    ContextBuilt,
    ToolInventoryBuilt,
    ProviderGuardCheck,
    ContextWindowCheck,
    ToolSelected,
    ToolFinished,
    LlmDelta,
    Completed,
    Failed,
    Cancelled,
}
```

- [ ] **Step 3: Emit ToolInventory event in run()**

After `let tools = self.build_tools_async(&intent).await;` at line 204, add:

```rust
        let tool_names: Vec<String> = tools
            .iter()
            .filter_map(|t| t["function"]["name"].as_str().map(String::from))
            .collect();
        let generation = {
            let registry = self.executor.registry.lock().await;
            registry.generation()
        };
        self.emit(AgentLoopEvent::ToolInventory {
            tools: tool_names,
            generation,
        });
```

- [ ] **Step 4: Emit ProviderGuard event in run()**

After the `provider_call_guard` call block (after line 287 in current code), inside the `if let Some(provider_call_guard) = &self.provider_call_guard` block, add after the Err check:

```rust
                self.emit(AgentLoopEvent::ProviderGuard {
                    allowed: true,
                    model: model.clone(),
                    estimated_input_tokens,
                    requested_output_tokens,
                });
```

And in the Err branch (before `return Err(message);`):

```rust
                    self.emit(AgentLoopEvent::ProviderGuard {
                        allowed: false,
                        model,
                        estimated_input_tokens,
                        requested_output_tokens,
                    });
```

- [ ] **Step 5: Emit ContextWindow event in run()**

After the `evaluate_context_window` block (line 241 in current code), add:

```rust
            self.emit(AgentLoopEvent::ContextWindow {
                tokens: guard.tokens,
                estimated_input: guard.estimated_input_tokens,
                requested_output: guard.requested_output_tokens,
                should_warn: guard.should_warn,
                should_block: guard.should_block,
            });
```

- [ ] **Step 6: Run tests and verify**

```powershell
cargo test -p agent-harness-core
cargo clippy -p agent-harness-core --all-targets -- -D warnings
cargo check -p agent-writer
```

- [ ] **Step 7: Commit**

```bash
git add agent-harness-core/src/agent_loop.rs agent-harness-core/src/run_trace.rs
git commit -m "feat: emit ToolInventory, ProviderGuard, and ContextWindow trace events

AgentLoop now records which tools were exposed, provider guard
decisions, and context window checks as structured trace events
for post-run observability and debugging.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>"
```

---

### Task 5: Final Integration Check

- [ ] **Step 1: Run full workspace validation**

```powershell
cargo fmt --check
cargo check --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p agent-harness-core
cargo test -p agent-writer --lib
cargo test -p forge-agent-mcp
```

- [ ] **Step 2: Verify smoke test passes with real binary**

```powershell
cargo test -p forge-agent-mcp --test smoke_test -- --nocapture
```

Expected: All 6 tests pass.

- [ ] **Step 3: Run final verification**

No changes needed — step 1 and 2 are the verification.

---

## Task Summary

| Task | Files Changed | New Lines | Est. Time |
|------|-------------|-----------|-----------|
| 1. Error Kind | main.rs, headless.rs | ~80 | 30 min |
| 2. Smoke Test | smoke_test.rs (new) | ~160 | 30 min |
| 3. TTFT Fix | agent_loop.rs | ~30 | 20 min |
| 4. Trace Events | agent_loop.rs, run_trace.rs | ~60 | 25 min |
| 5. Integration Check | — | — | 10 min |
| **Total** | **5 files** | **~330** | **~2 hrs** |
