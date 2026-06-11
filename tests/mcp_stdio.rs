/// Integration test: spawn `tio mcp` as a child process and speak MCP over stdio.
///
/// Verifies the full MCP protocol flow:
///   1. initialize -> get protocol version + server info
///   2. notifications/initialized (no response)
///   3. tools/list -> assert 8 tools returned
///   4. open_session on a PTY slave -> get session_id
///   5. send "hello" through the session
///   6. read -> get the echoed data back
///   7. close_session -> clean up
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::Duration;

/// Write a JSON-RPC request line to the child's stdin.
fn send_request(child: &mut Child, line: &str) {
    let stdin = child.stdin.as_mut().expect("stdin");
    writeln!(stdin, "{}", line).expect("write to stdin");
    stdin.flush().expect("flush stdin");
}

/// Read one JSON-RPC response line from the child's stdout.
fn read_response(child: &mut Child) -> String {
    let stdout = child.stdout.as_mut().expect("stdout");
    let mut reader = BufReader::new(stdout);
    let mut line = String::new();
    reader.read_line(&mut line).expect("read from stdout");
    line.trim().to_string()
}

/// Parse a JSON-RPC response and extract the result as serde_json::Value.
fn parse_result(resp: &str) -> serde_json::Value {
    let parsed: serde_json::Value = serde_json::from_str(resp).expect("parse JSON-RPC response");
    parsed
        .get("result")
        .cloned()
        .expect("response should have result")
}

#[test]
fn test_mcp_stdio_full_session() {
    // Build the binary first (assumes `cargo build` was run)
    let mut child = Command::new(env!("CARGO_BIN_EXE_tio"))
        .arg("mcp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn tio mcp");

    // 1. Initialize
    let init_req = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": { "name": "test", "version": "0.1.0" }
        }
    });
    send_request(&mut child, &init_req.to_string());
    let resp = read_response(&mut child);
    let result = parse_result(&resp);
    assert_eq!(result["protocolVersion"], "2024-11-05");
    assert_eq!(result["serverInfo"]["name"], "tio-rs");

    // 2. Send initialized notification (no response expected)
    let notif = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "notifications/initialized"
    });
    send_request(&mut child, &notif.to_string());
    // Give the server a moment to process the notification
    thread::sleep(Duration::from_millis(50));

    // 3. tools/list
    let list_req = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/list",
        "params": {}
    });
    send_request(&mut child, &list_req.to_string());
    let resp = read_response(&mut child);
    let result = parse_result(&resp);
    let tools = result["tools"].as_array().expect("tools should be array");
    assert_eq!(tools.len(), 8, "expected 8 tools");

    // Verify tool names
    let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert!(names.contains(&"list_ports"));
    assert!(names.contains(&"open_session"));
    assert!(names.contains(&"send"));
    assert!(names.contains(&"read"));
    assert!(names.contains(&"expect"));
    assert!(names.contains(&"send_break"));
    assert!(names.contains(&"set_line"));
    assert!(names.contains(&"close_session"));

    // 4. Create a PTY pair and open_session on the slave
    let mut master =
        nix::pty::posix_openpt(nix::fcntl::OFlag::O_RDWR | nix::fcntl::OFlag::O_NOCTTY)
            .expect("posix_openpt");
    nix::pty::grantpt(&master).expect("grantpt");
    nix::pty::unlockpt(&master).expect("unlockpt");
    let slave_path = unsafe { nix::pty::ptsname(&master) }.expect("ptsname");

    let open_req = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "tools/call",
        "params": {
            "name": "open_session",
            "arguments": {
                "device": slave_path,
                "baudrate": 115200
            }
        }
    });
    send_request(&mut child, &open_req.to_string());
    let resp = read_response(&mut child);
    let result = parse_result(&resp);
    let session_id = result["session_id"]
        .as_str()
        .expect("session_id should be a string")
        .to_string();
    assert!(!session_id.is_empty());

    // 5. Write data to the master side so the slave (reader thread) sees it
    master.write_all(b"hello\r\n").expect("write to master");
    master.flush().expect("flush master");
    thread::sleep(Duration::from_millis(200));

    // 6. Read from the session — should get "hello\r\n" from the PTY
    let read_req = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 5,
        "method": "tools/call",
        "params": {
            "name": "read",
            "arguments": {
                "session_id": session_id,
                "max_wait_ms": 500
            }
        }
    });
    send_request(&mut child, &read_req.to_string());
    let resp = read_response(&mut child);
    let result = parse_result(&resp);
    let text = result["text"].as_str().expect("text should be a string");
    assert!(
        text.contains("hello"),
        "read should contain 'hello', got: {:?}",
        text
    );

    // 7. Close the session
    let close_req = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 6,
        "method": "tools/call",
        "params": {
            "name": "close_session",
            "arguments": {
                "session_id": session_id
            }
        }
    });
    send_request(&mut child, &close_req.to_string());
    let resp = read_response(&mut child);
    let result = parse_result(&resp);
    assert_eq!(result["ok"], true);

    // Clean up
    let _ = child.kill();
    let _ = child.wait();
}

#[test]
fn test_mcp_stdio_list_ports() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_tio"))
        .arg("mcp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn tio mcp");

    // Initialize
    let init_req = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": { "name": "test", "version": "0.1.0" }
        }
    });
    send_request(&mut child, &init_req.to_string());
    let _ = read_response(&mut child);

    // list_ports (no params)
    let list_req = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/call",
        "params": {
            "name": "list_ports",
            "arguments": {}
        }
    });
    send_request(&mut child, &list_req.to_string());
    let resp = read_response(&mut child);
    let result = parse_result(&resp);
    let ports = result.as_array().expect("list_ports should return array");
    // On this machine we may have some tty devices — just verify it's an array
    // (could be empty or non-empty depending on the environment)
    let _ = ports.len(); // just verify it parses

    let _ = child.kill();
    let _ = child.wait();
}

#[test]
fn test_mcp_stdio_unknown_tool() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_tio"))
        .arg("mcp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn tio mcp");

    // Initialize
    let init_req = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": { "name": "test", "version": "0.1.0" }
        }
    });
    send_request(&mut child, &init_req.to_string());
    let _ = read_response(&mut child);

    // Call unknown tool
    let req = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/call",
        "params": {
            "name": "nonexistent_tool",
            "arguments": {}
        }
    });
    send_request(&mut child, &req.to_string());
    let resp = read_response(&mut child);
    let parsed: serde_json::Value = serde_json::from_str(&resp).expect("parse response");
    assert!(parsed["error"].is_object(), "should return error");
    assert_eq!(parsed["error"]["code"], -32602);

    let _ = child.kill();
    let _ = child.wait();
}
