use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command};

/// Kills and waits on the child process on drop, preventing orphaned
/// subprocess leaks when a test panics before reaching `child.kill()`.
struct ChildGuard(Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn spawn_mcp() -> (ChildGuard, BufReader<ChildStdout>, ChildStdin) {
    let exe = env!("CARGO_BIN_EXE_forge-agent-mcp");
    let temp_dir = std::env::temp_dir().join(format!("forge-smoke-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_dir);

    let mut child = Command::new(exe)
        .arg("stdio")
        .env("FORGE_AGENT_DATA_DIR", &temp_dir)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        // Null stderr — piping it without a reader can deadlock the process
        // when the OS pipe buffer (64 KB) fills up.
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("failed to start forge-agent-mcp");

    let stdout = BufReader::new(child.stdout.take().unwrap());
    let stdin = child.stdin.take().unwrap();
    (ChildGuard(child), stdout, stdin)
}

fn send_request(stdin: &mut ChildStdin, request: &serde_json::Value) {
    let mut line = serde_json::to_string(request).unwrap();
    line.push('\n');
    stdin.write_all(line.as_bytes()).unwrap();
    stdin.flush().unwrap();
}

/// NOTE: `read_response` uses `read_line` which will block indefinitely if
/// the server crashes silently. This is a known limitation of the smoke-test
/// harness — adding a read timeout would require refactoring to use
/// `TcpStream` or a background thread with a channel, which is overkill
/// for a smoke suite. The `ChildGuard` ensures the process is reaped on
/// test panic, so a hung read will at least not leak the subprocess.
fn read_response(stdout: &mut BufReader<ChildStdout>) -> serde_json::Value {
    let mut line = String::new();
    stdout
        .read_line(&mut line)
        .expect("no response from MCP server");
    serde_json::from_str(line.trim()).expect("invalid JSON response")
}

fn initialize(stdin: &mut ChildStdin, stdout: &mut BufReader<ChildStdout>) {
    send_request(
        stdin,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {}
        }),
    );
    let response = read_response(stdout);
    assert!(
        response["error"].is_null(),
        "initialize failed: {:?}",
        response["error"]
    );
}

fn call_tool(stdin: &mut ChildStdin, name: &str, args: serde_json::Value) {
    send_request(
        stdin,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 99,
            "method": "tools/call",
            "params": {
                "name": name,
                "arguments": args
            }
        }),
    );
}

#[test]
fn smoke_initialize() {
    let (_child, mut stdout, mut stdin) = spawn_mcp();

    send_request(
        &mut stdin,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {}
        }),
    );

    let response = read_response(&mut stdout);
    assert_eq!(response["jsonrpc"], "2.0");
    assert_eq!(response["id"], 1);
    assert!(response["error"].is_null(), "initialize should not error");
    let result = &response["result"];
    assert!(result["protocolVersion"].is_string());
    assert_eq!(result["serverInfo"]["name"], "forge-writer-agent");
}

#[test]
fn smoke_tools_list() {
    let (_child, mut stdout, mut stdin) = spawn_mcp();
    initialize(&mut stdin, &mut stdout);

    send_request(
        &mut stdin,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/list",
            "params": {}
        }),
    );

    let response = read_response(&mut stdout);
    let tools = response["result"]["tools"]
        .as_array()
        .expect("tools should be array");
    assert!(!tools.is_empty(), "tools list should not be empty");

    let tool_names: Vec<&str> = tools.iter().filter_map(|t| t["name"].as_str()).collect();
    assert!(
        tool_names.contains(&"forge_status"),
        "should have forge_status"
    );
    assert!(
        tool_names.contains(&"forge_backend_call"),
        "should have forge_backend_call"
    );
    assert!(
        tool_names.contains(&"forge_list_chapters"),
        "should have forge_list_chapters"
    );
}

#[test]
fn smoke_forge_status() {
    let (_child, mut stdout, mut stdin) = spawn_mcp();
    initialize(&mut stdin, &mut stdout);

    call_tool(&mut stdin, "forge_status", serde_json::json!({}));

    let response = read_response(&mut stdout);
    let sc = &response["result"]["structuredContent"];
    assert_eq!(sc["ok"], true);
    assert!(sc["data"]["project"].is_object());
}

#[test]
fn smoke_forge_backend_call_status() {
    let (_child, mut stdout, mut stdin) = spawn_mcp();
    initialize(&mut stdin, &mut stdout);

    call_tool(
        &mut stdin,
        "forge_backend_call",
        serde_json::json!({
            "action": "status",
            "params": {}
        }),
    );

    let response = read_response(&mut stdout);
    let sc = &response["result"]["structuredContent"];
    assert_eq!(sc["ok"], true, "backend call status should succeed");
}

#[test]
fn smoke_forge_list_chapters_empty_project() {
    let (_child, mut stdout, mut stdin) = spawn_mcp();
    initialize(&mut stdin, &mut stdout);

    call_tool(&mut stdin, "forge_list_chapters", serde_json::json!({}));

    let response = read_response(&mut stdout);
    let sc = &response["result"]["structuredContent"];
    assert_eq!(
        sc["ok"], true,
        "list chapters should succeed on empty project"
    );
}

#[test]
fn smoke_forge_agent_domain_profile() {
    let (_child, mut stdout, mut stdin) = spawn_mcp();
    initialize(&mut stdin, &mut stdout);

    call_tool(
        &mut stdin,
        "forge_agent_domain_profile",
        serde_json::json!({}),
    );

    let response = read_response(&mut stdout);
    let sc = &response["result"]["structuredContent"];
    assert_eq!(sc["ok"], true);
    assert_eq!(sc["data"]["id"].as_str(), Some("longform_writing"));
}
