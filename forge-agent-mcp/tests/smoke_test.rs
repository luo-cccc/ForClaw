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
    let _ = read_response(stdout);
}

fn call_tool(
    stdin: &mut ChildStdin,
    _stdout: &mut BufReader<ChildStdout>,
    name: &str,
    args: serde_json::Value,
) {
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
    let (mut child, mut stdout, mut stdin) = spawn_mcp();

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

    child.kill().ok();
}

#[test]
fn smoke_tools_list() {
    let (mut child, mut stdout, mut stdin) = spawn_mcp();
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

    child.kill().ok();
}

#[test]
fn smoke_forge_status() {
    let (mut child, mut stdout, mut stdin) = spawn_mcp();
    initialize(&mut stdin, &mut stdout);

    call_tool(
        &mut stdin,
        &mut stdout,
        "forge_status",
        serde_json::json!({}),
    );

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

    call_tool(
        &mut stdin,
        &mut stdout,
        "forge_backend_call",
        serde_json::json!({
            "action": "status",
            "params": {}
        }),
    );

    let response = read_response(&mut stdout);
    let sc = &response["result"]["structuredContent"];
    assert_eq!(sc["ok"], true, "backend call status should succeed");

    child.kill().ok();
}

#[test]
fn smoke_forge_list_chapters_empty_project() {
    let (mut child, mut stdout, mut stdin) = spawn_mcp();
    initialize(&mut stdin, &mut stdout);

    call_tool(
        &mut stdin,
        &mut stdout,
        "forge_list_chapters",
        serde_json::json!({}),
    );

    let response = read_response(&mut stdout);
    let sc = &response["result"]["structuredContent"];
    assert_eq!(
        sc["ok"], true,
        "list chapters should succeed on empty project"
    );

    child.kill().ok();
}

#[test]
fn smoke_forge_agent_domain_profile() {
    let (mut child, mut stdout, mut stdin) = spawn_mcp();
    initialize(&mut stdin, &mut stdout);

    call_tool(
        &mut stdin,
        &mut stdout,
        "forge_agent_domain_profile",
        serde_json::json!({}),
    );

    let response = read_response(&mut stdout);
    let sc = &response["result"]["structuredContent"];
    assert_eq!(sc["ok"], true);
    assert_eq!(sc["data"]["id"].as_str(), Some("longform_writing"));

    child.kill().ok();
}
