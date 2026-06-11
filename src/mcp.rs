/// MCP stdio server for AI agents.
///
/// Implements the Model Context Protocol (MCP) over stdio transport
/// (JSON-RPC 2.0, protocol version 2024-11-05). Exposes serial port
/// operations as MCP tools so AI agents can list, open, read, write,
/// and close serial sessions without a human at the TTY.
///
/// Hand-rolled (no rmcp dependency) for simplicity and reliability.
use std::collections::HashMap;
use std::io::{self, BufRead, Write};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::list;
use crate::oneshot::parse_escapes;
use crate::serial::{self, SerialConfig};

// ---------------------------------------------------------------------------
// JSON-RPC 2.0 types
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize)]
struct JsonRpcRequest {
    jsonrpc: String,
    #[serde(default)]
    id: Value,
    method: String,
    params: Option<Value>,
}

#[derive(Debug, Serialize)]
struct JsonRpcResponse {
    jsonrpc: &'static str,
    id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
}

#[derive(Debug, Serialize)]
struct JsonRpcError {
    code: i64,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<Value>,
}

#[derive(Debug, Serialize)]
#[allow(dead_code)]
struct JsonRpcNotification {
    jsonrpc: &'static str,
    method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    params: Option<Value>,
}

// ---------------------------------------------------------------------------
// MCP protocol types
// ---------------------------------------------------------------------------

const PROTOCOL_VERSION: &str = "2024-11-05";

#[derive(Debug, Serialize)]
struct InitializeResult {
    #[serde(rename = "protocolVersion")]
    protocol_version: &'static str,
    capabilities: ServerCapabilities,
    #[serde(rename = "serverInfo")]
    server_info: ServerInfo,
}

#[derive(Debug, Serialize)]
struct ServerCapabilities {
    tools: Option<ToolsCapability>,
}

#[derive(Debug, Serialize)]
struct ToolsCapability {
    #[serde(rename = "listChanged")]
    list_changed: Option<bool>,
}

#[derive(Debug, Serialize)]
struct ServerInfo {
    name: &'static str,
    version: &'static str,
}

#[derive(Debug, Serialize)]
struct ToolsListResult {
    tools: Vec<ToolDefinition>,
}

#[derive(Debug, Serialize)]
struct ToolDefinition {
    name: &'static str,
    description: &'static str,
    #[serde(rename = "inputSchema")]
    input_schema: Value,
}

// ---------------------------------------------------------------------------
// Session management
// ---------------------------------------------------------------------------

/// A serial session: holds the port handle and a ring buffer of incoming bytes.
struct Session {
    port: Box<dyn serialport::SerialPort>,
    buffer: Vec<u8>,
}

/// Shared session registry.
type SessionMap = Arc<Mutex<HashMap<String, Session>>>;

/// Generate a short session ID (8 hex chars).
fn generate_session_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("{:08x}", (nanos as u32))
}

// ---------------------------------------------------------------------------
// Tool definitions (schemas)
// ---------------------------------------------------------------------------

fn tool_definitions() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name: "list_ports",
            description: "List available serial devices with driver, description, and by-id info.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {},
                "additionalProperties": false
            }),
        },
        ToolDefinition {
            name: "open_session",
            description: "Open a serial device and create a session for I/O operations.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "device": {"type": "string", "description": "Serial device path (e.g. /dev/ttyUSB0)"},
                    "baudrate": {"type": "integer", "description": "Baud rate (default 115200)"},
                    "databits": {"type": "integer", "description": "Data bits 5-8 (default 8)"},
                    "stopbits": {"type": "integer", "description": "Stop bits 1-2 (default 1)"},
                    "parity": {"type": "string", "enum": ["none", "odd", "even"], "description": "Parity (default none)"},
                    "flow": {"type": "string", "enum": ["none", "hard", "soft"], "description": "Flow control (default none)"}
                },
                "required": ["device"],
                "additionalProperties": false
            }),
        },
        ToolDefinition {
            name: "send",
            description:
                "Send data to a serial session. Supports escape sequences: \\r \\n \\t \\\\ \\xNN",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "session_id": {"type": "string"},
                    "data": {"type": "string", "description": "Data to send (with escape sequences)"}
                },
                "required": ["session_id", "data"],
                "additionalProperties": false
            }),
        },
        ToolDefinition {
            name: "read",
            description: "Read pending bytes from a session's receive buffer.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "session_id": {"type": "string"},
                    "max_wait_ms": {"type": "integer", "description": "Max milliseconds to wait for data (default 1000)"}
                },
                "required": ["session_id"],
                "additionalProperties": false
            }),
        },
        ToolDefinition {
            name: "expect",
            description: "Wait for a regex match in the session's receive buffer.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "session_id": {"type": "string"},
                    "pattern": {"type": "string", "description": "Regex pattern to match"},
                    "timeout_ms": {"type": "integer", "description": "Timeout in milliseconds (default 10000)"}
                },
                "required": ["session_id", "pattern"],
                "additionalProperties": false
            }),
        },
        ToolDefinition {
            name: "send_break",
            description: "Send a break signal on the serial session.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "session_id": {"type": "string"}
                },
                "required": ["session_id"],
                "additionalProperties": false
            }),
        },
        ToolDefinition {
            name: "set_line",
            description: "Set or toggle DTR/RTS line state.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "session_id": {"type": "string"},
                    "line": {"type": "string", "enum": ["dtr", "rts"]},
                    "state": {"type": "string", "enum": ["high", "low", "toggle"]}
                },
                "required": ["session_id", "line", "state"],
                "additionalProperties": false
            }),
        },
        ToolDefinition {
            name: "close_session",
            description: "Close a serial session and release the port.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "session_id": {"type": "string"}
                },
                "required": ["session_id"],
                "additionalProperties": false
            }),
        },
    ]
}

// ---------------------------------------------------------------------------
// MCP server state
// ---------------------------------------------------------------------------

struct McpServer {
    sessions: SessionMap,
}

impl McpServer {
    fn new() -> Self {
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn handle_request(&self, req: JsonRpcRequest) -> Option<JsonRpcResponse> {
        let id = req.id.clone();
        // Notifications have no id (or null id) — no response needed
        if id.is_null() {
            return None;
        }
        match req.method.as_str() {
            "initialize" => Some(self.handle_initialize(id)),
            "notifications/initialized" => {
                // Notification — no response needed
                None
            }
            "tools/list" => Some(self.handle_tools_list(id)),
            "tools/call" => self.handle_tools_call(id, req.params),
            other => Some(error_response(
                id,
                -32601,
                &format!("Method not found: {}", other),
            )),
        }
    }

    fn handle_initialize(&self, id: Value) -> JsonRpcResponse {
        let result = serde_json::to_value(InitializeResult {
            protocol_version: PROTOCOL_VERSION,
            capabilities: ServerCapabilities {
                tools: Some(ToolsCapability {
                    list_changed: Some(false),
                }),
            },
            server_info: ServerInfo {
                name: "tio-rs",
                version: env!("CARGO_PKG_VERSION"),
            },
        })
        .unwrap();
        success_response(id, result)
    }

    fn handle_tools_list(&self, id: Value) -> JsonRpcResponse {
        let result = serde_json::to_value(ToolsListResult {
            tools: tool_definitions(),
        })
        .unwrap();
        success_response(id, result)
    }

    fn handle_tools_call(&self, id: Value, params: Option<Value>) -> Option<JsonRpcResponse> {
        let params = match params {
            Some(v) => v,
            None => {
                return Some(error_response(id, -32602, "Missing tool call params"));
            }
        };

        let name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let arguments = params
            .get("arguments")
            .cloned()
            .unwrap_or(serde_json::json!({}));

        match name {
            "list_ports" => Some(self.tool_list_ports(id)),
            "open_session" => Some(self.tool_open_session(id, arguments)),
            "send" => Some(self.tool_send(id, arguments)),
            "read" => Some(self.tool_read(id, arguments)),
            "expect" => Some(self.tool_expect(id, arguments)),
            "send_break" => Some(self.tool_send_break(id, arguments)),
            "set_line" => Some(self.tool_set_line(id, arguments)),
            "close_session" => Some(self.tool_close_session(id, arguments)),
            other => Some(error_response(
                id,
                -32602,
                &format!("Unknown tool: {}", other),
            )),
        }
    }

    // --- tool implementations ---

    fn tool_list_ports(&self, id: Value) -> JsonRpcResponse {
        let devices = list::enumerate_devices();
        match serde_json::to_value(&devices) {
            Ok(v) => success_response(id, v),
            Err(e) => error_response(id, -32603, &format!("Serialization error: {}", e)),
        }
    }

    fn tool_open_session(&self, id: Value, args: Value) -> JsonRpcResponse {
        let device = args
            .get("device")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        if device.is_empty() {
            return error_response(id, -32602, "Missing 'device' parameter");
        }

        let baudrate = args
            .get("baudrate")
            .and_then(|v| v.as_u64())
            .unwrap_or(115200) as u32;

        let databits = args.get("databits").and_then(|v| v.as_u64()).unwrap_or(8) as u8;

        let stopbits = args.get("stopbits").and_then(|v| v.as_u64()).unwrap_or(1) as u8;

        let parity = match args.get("parity").and_then(|v| v.as_str()) {
            Some("odd") => serialport::Parity::Odd,
            Some("even") => serialport::Parity::Even,
            _ => serialport::Parity::None,
        };

        let flow = match args.get("flow").and_then(|v| v.as_str()) {
            Some("hard") => serialport::FlowControl::Hardware,
            Some("soft") => serialport::FlowControl::Software,
            _ => serialport::FlowControl::None,
        };

        let cfg = SerialConfig {
            device: device.clone(),
            baudrate,
            databits,
            stopbits,
            parity,
            flow,
            reconnect: false,
        };

        let mut port = match serial::open(&cfg) {
            Ok(p) => p,
            Err(e) => {
                return error_response(id, -32603, &format!("Failed to open {}: {}", device, e));
            }
        };

        // Set a reasonable read timeout so the background reader doesn't block forever
        let _ = port.set_timeout(Duration::from_millis(100));

        let session_id = generate_session_id();
        let sid = session_id.clone();

        let mut sessions = self.sessions.lock().unwrap();
        sessions.insert(
            sid,
            Session {
                port,
                buffer: Vec::new(),
            },
        );

        // Spawn background reader thread
        let sessions_arc = Arc::clone(&self.sessions);
        let session_id_clone = session_id.clone();
        thread::spawn(move || {
            let mut read_buf = [0u8; 1024];
            loop {
                let mut sessions = sessions_arc.lock().unwrap();
                if !sessions.contains_key(&session_id_clone) {
                    break; // Session closed
                }
                match sessions
                    .get_mut(&session_id_clone)
                    .unwrap()
                    .port
                    .read(&mut read_buf)
                {
                    Ok(n) if n > 0 => {
                        sessions
                            .get_mut(&session_id_clone)
                            .unwrap()
                            .buffer
                            .extend_from_slice(&read_buf[..n]);
                    }
                    Ok(_) => {
                        // No data — release lock and sleep briefly
                        drop(sessions);
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(_) => {
                        // Port error — session is likely dead
                        break;
                    }
                }
            }
        });

        success_response(id, serde_json::json!({"session_id": session_id}))
    }

    fn tool_send(&self, id: Value, args: Value) -> JsonRpcResponse {
        let session_id = args
            .get("session_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let data = args.get("data").and_then(|v| v.as_str()).unwrap_or("");

        let bytes = parse_escapes(data);

        let mut sessions = self.sessions.lock().unwrap();
        match sessions.get_mut(&session_id) {
            Some(session) => match session.port.write_all(&bytes) {
                Ok(_) => {
                    let _ = session.port.flush();
                    success_response(id, serde_json::json!({"sent": bytes.len()}))
                }
                Err(e) => error_response(id, -32603, &format!("Write error: {}", e)),
            },
            None => error_response(id, -32602, &format!("Unknown session: {}", session_id)),
        }
    }

    fn tool_read(&self, id: Value, args: Value) -> JsonRpcResponse {
        let session_id = args
            .get("session_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let max_wait_ms = args
            .get("max_wait_ms")
            .and_then(|v| v.as_u64())
            .unwrap_or(1000);

        // Check if session exists and drain buffer if data is available
        let needs_wait = {
            let mut sessions = self.sessions.lock().unwrap();
            match sessions.get_mut(&session_id) {
                Some(session) => {
                    if session.buffer.is_empty() {
                        true
                    } else {
                        let data: Vec<u8> = session.buffer.drain(..).collect();
                        let text = String::from_utf8_lossy(&data).to_string();
                        let base64 = base64_encode(&data);
                        return success_response(
                            id,
                            serde_json::json!({
                                "text": text,
                                "base64": base64,
                                "bytes": data.len()
                            }),
                        );
                    }
                }
                None => {
                    return error_response(id, -32602, &format!("Unknown session: {}", session_id));
                }
            }
        };

        if needs_wait {
            let deadline = std::time::Instant::now() + Duration::from_millis(max_wait_ms);
            loop {
                thread::sleep(Duration::from_millis(10));
                if std::time::Instant::now() >= deadline {
                    break;
                }
                let mut sessions = self.sessions.lock().unwrap();
                if !sessions.contains_key(&session_id) {
                    return error_response(id, -32603, "Session was closed during read");
                }
                if !sessions.get(&session_id).unwrap().buffer.is_empty() {
                    let data: Vec<u8> = sessions
                        .get_mut(&session_id)
                        .unwrap()
                        .buffer
                        .drain(..)
                        .collect();
                    let text = String::from_utf8_lossy(&data).to_string();
                    let base64 = base64_encode(&data);
                    return success_response(
                        id,
                        serde_json::json!({
                            "text": text,
                            "base64": base64,
                            "bytes": data.len()
                        }),
                    );
                }
            }
        }

        success_response(
            id,
            serde_json::json!({
                "text": "",
                "base64": "",
                "bytes": 0
            }),
        )
    }

    fn tool_expect(&self, id: Value, args: Value) -> JsonRpcResponse {
        let session_id = args
            .get("session_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let pattern = args.get("pattern").and_then(|v| v.as_str()).unwrap_or("");

        let timeout_ms = args
            .get("timeout_ms")
            .and_then(|v| v.as_u64())
            .unwrap_or(10000);

        let regex = match Regex::new(pattern) {
            Ok(re) => re,
            Err(e) => {
                return error_response(id, -32602, &format!("Invalid regex: {}", e));
            }
        };

        let deadline = std::time::Instant::now() + Duration::from_millis(timeout_ms);

        loop {
            if std::time::Instant::now() >= deadline {
                return success_response(
                    id,
                    serde_json::json!({
                        "matched": Value::Null,
                        "buffer": ""
                    }),
                );
            }

            let mut sessions = self.sessions.lock().unwrap();
            if !sessions.contains_key(&session_id) {
                return error_response(id, -32603, "Session was closed during expect");
            }

            let text =
                String::from_utf8_lossy(&sessions.get(&session_id).unwrap().buffer).to_string();

            if let Some(m) = regex.find(&text) {
                // Drain the buffer up to and including the match
                let match_end = m.end();
                let _drained: Vec<u8> = sessions
                    .get_mut(&session_id)
                    .unwrap()
                    .buffer
                    .drain(..match_end)
                    .collect();

                return success_response(
                    id,
                    serde_json::json!({
                        "matched": m.as_str(),
                        "buffer": &text[..match_end]
                    }),
                );
            }

            drop(sessions);
            thread::sleep(Duration::from_millis(20));
        }
    }

    fn tool_send_break(&self, id: Value, args: Value) -> JsonRpcResponse {
        let session_id = args
            .get("session_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let mut sessions = self.sessions.lock().unwrap();
        match sessions.get_mut(&session_id) {
            Some(session) => match serial::send_break(&mut session.port) {
                Ok(_) => success_response(id, serde_json::json!({"ok": true})),
                Err(e) => error_response(id, -32603, &format!("Break error: {}", e)),
            },
            None => error_response(id, -32602, &format!("Unknown session: {}", session_id)),
        }
    }

    fn tool_set_line(&self, id: Value, args: Value) -> JsonRpcResponse {
        let session_id = args
            .get("session_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let line = args.get("line").and_then(|v| v.as_str()).unwrap_or("");

        let state = args.get("state").and_then(|v| v.as_str()).unwrap_or("");

        let mut sessions = self.sessions.lock().unwrap();
        match sessions.get_mut(&session_id) {
            Some(session) => {
                let result = match (line, state) {
                    ("dtr", "high") => serial::set_dtr(&mut session.port, true),
                    ("dtr", "low") => serial::set_dtr(&mut session.port, false),
                    ("dtr", "toggle") => {
                        // Read current state and toggle — serialport doesn't expose
                        // read_data_terminal_ready, so just set high then low
                        let _ = serial::set_dtr(&mut session.port, true);
                        serial::set_dtr(&mut session.port, false)
                    }
                    ("rts", "high") => serial::set_rts(&mut session.port, true),
                    ("rts", "low") => serial::set_rts(&mut session.port, false),
                    ("rts", "toggle") => {
                        let _ = serial::set_rts(&mut session.port, true);
                        serial::set_rts(&mut session.port, false)
                    }
                    _ => {
                        return error_response(
                            id,
                            -32602,
                            &format!("Invalid line/state: {}/{}", line, state),
                        );
                    }
                };
                match result {
                    Ok(_) => success_response(id, serde_json::json!({"ok": true})),
                    Err(e) => error_response(id, -32603, &format!("Line control error: {}", e)),
                }
            }
            None => error_response(id, -32602, &format!("Unknown session: {}", session_id)),
        }
    }

    fn tool_close_session(&self, id: Value, args: Value) -> JsonRpcResponse {
        let session_id = args
            .get("session_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let mut sessions = self.sessions.lock().unwrap();
        match sessions.remove(&session_id) {
            Some(_session) => {
                // Port is dropped when Session is dropped, closing the device
                success_response(id, serde_json::json!({"ok": true}))
            }
            None => error_response(id, -32602, &format!("Unknown session: {}", session_id)),
        }
    }
}

// ---------------------------------------------------------------------------
// Response helpers
// ---------------------------------------------------------------------------

fn success_response(id: Value, result: Value) -> JsonRpcResponse {
    JsonRpcResponse {
        jsonrpc: "2.0",
        id,
        result: Some(result),
        error: None,
    }
}

fn error_response(id: Value, code: i64, message: &str) -> JsonRpcResponse {
    JsonRpcResponse {
        jsonrpc: "2.0",
        id,
        result: None,
        error: Some(JsonRpcError {
            code,
            message: message.to_string(),
            data: None,
        }),
    }
}

// ---------------------------------------------------------------------------
// Base64 encoding (no external dep)
// ---------------------------------------------------------------------------

fn base64_encode(data: &[u8]) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = match chunk.len() {
            3 => ((chunk[0] as u32) << 16) | ((chunk[1] as u32) << 8) | (chunk[2] as u32),
            2 => ((chunk[0] as u32) << 16) | ((chunk[1] as u32) << 8),
            1 => (chunk[0] as u32) << 16,
            _ => 0,
        };
        result.push(ALPHABET[((b >> 18) & 0x3F) as usize] as char);
        result.push(ALPHABET[((b >> 12) & 0x3F) as usize] as char);
        if chunk.len() >= 2 {
            result.push(ALPHABET[((b >> 6) & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
        if chunk.len() >= 3 {
            result.push(ALPHABET[(b & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
    }
    result
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Run the MCP stdio server. Reads JSON-RPC requests from stdin,
/// dispatches them, and writes responses to stdout.
pub fn run_mcp_server() -> io::Result<()> {
    let server = McpServer::new();
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut stdout_lock = stdout.lock();

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };

        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let req: JsonRpcRequest = match serde_json::from_str(line) {
            Ok(r) => r,
            Err(e) => {
                let resp = error_response(Value::Null, -32700, &format!("Parse error: {}", e));
                let _ = writeln!(stdout_lock, "{}", serde_json::to_string(&resp).unwrap());
                stdout_lock.flush()?;
                continue;
            }
        };

        if let Some(resp) = server.handle_request(req) {
            let json = serde_json::to_string(&resp).unwrap();
            writeln!(stdout_lock, "{}", json)?;
            stdout_lock.flush()?;
        }
        // Notifications (no response) are silently consumed
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_base64_encode_empty() {
        assert_eq!(base64_encode(b""), "");
    }

    #[test]
    fn test_base64_encode_hello() {
        assert_eq!(base64_encode(b"hello"), "aGVsbG8=");
    }

    #[test]
    fn test_base64_encode_binary() {
        assert_eq!(base64_encode(&[0x00, 0xFF, 0x42]), "AP9C");
    }

    #[test]
    fn test_tool_definitions_count() {
        assert_eq!(tool_definitions().len(), 8);
    }

    #[test]
    fn test_tool_names() {
        let names: Vec<&str> = tool_definitions().iter().map(|t| t.name).collect();
        assert_eq!(
            names,
            vec![
                "list_ports",
                "open_session",
                "send",
                "read",
                "expect",
                "send_break",
                "set_line",
                "close_session",
            ]
        );
    }

    #[test]
    fn test_initialize_response() {
        let server = McpServer::new();
        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: serde_json::json!(1),
            method: "initialize".to_string(),
            params: None,
        };
        let resp = server.handle_request(req).unwrap();
        assert!(resp.result.is_some());
        let result = resp.result.unwrap();
        assert_eq!(result["protocolVersion"], PROTOCOL_VERSION);
        assert_eq!(result["serverInfo"]["name"], "tio-rs");
    }

    #[test]
    fn test_tools_list_response() {
        let server = McpServer::new();
        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: serde_json::json!(2),
            method: "tools/list".to_string(),
            params: None,
        };
        let resp = server.handle_request(req).unwrap();
        let result = resp.result.unwrap();
        let tools = result["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 8);
    }

    #[test]
    fn test_unknown_session_error() {
        let server = McpServer::new();
        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: serde_json::json!(3),
            method: "tools/call".to_string(),
            params: Some(serde_json::json!({
                "name": "read",
                "arguments": {
                    "session_id": "nonexistent",
                    "max_wait_ms": 100
                }
            })),
        };
        let resp = server.handle_request(req).unwrap();
        assert!(resp.error.is_some());
        let err_msg = resp.error.as_ref().unwrap().message.clone();
        assert!(
            err_msg.contains("Unknown session"),
            "Actual error: {}",
            err_msg
        );
    }

    #[test]
    fn test_unknown_method() {
        let server = McpServer::new();
        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: serde_json::json!(4),
            method: "unknown_method".to_string(),
            params: None,
        };
        let resp = server.handle_request(req).unwrap();
        assert!(resp.error.is_some());
        assert_eq!(resp.error.as_ref().unwrap().code, -32601);
    }

    #[test]
    fn test_generate_session_id_unique() {
        let id1 = generate_session_id();
        let id2 = generate_session_id();
        assert_ne!(id1, id2);
        assert_eq!(id1.len(), 8);
    }
}
