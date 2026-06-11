# tio-rs MCP Server

`tio mcp` runs an MCP (Model Context Protocol) stdio server, exposing serial
port operations as tools so AI agents can list, open, read, write, and close
serial sessions without a human at the TTY.

## Protocol

- Transport: stdio (JSON-RPC 2.0)
- Protocol version: `2024-11-05`
- No external dependencies (hand-rolled JSON-RPC, no `rmcp` crate)

## Quick Start

### Claude Code

```bash
claude mcp add tio -- tio mcp
```

### Cursor

Add to `.cursor/mcp.json`:

```json
{
  "mcpServers": {
    "tio": {
      "command": "tio",
      "args": ["mcp"],
      "transport": "stdio"
    }
  }
}
```

### Generic (any MCP client)

```bash
tio mcp
```

The server reads JSON-RPC requests from stdin and writes responses to stdout.

## Tools

| Tool | Description |
|------|-------------|
| `list_ports` | Enumerate serial devices (path, driver, description, by-id) |
| `open_session` | Open a device with serial params, returns a session ID |
| `send` | Write bytes/text to a session (supports `\r` `\n` `\t` `\\` `\xNN` escapes) |
| `read` | Read pending bytes from a session buffer (with optional wait) |
| `expect` | Wait for a regex match in the receive stream |
| `send_break` | Send a break signal |
| `set_line` | Set/toggle DTR or RTS line state |
| `close_session` | Close a session and release the port |

### list_ports

```json
{ "name": "list_ports", "arguments": {} }
```

Returns a JSON array of devices:
```json
[
  {
    "path": "/dev/ttyUSB0",
    "tid": "a1b2",
    "uptime_s": 120,
    "driver": "pl2303",
    "description": "USB Serial",
    "by_id": "usb-FTDI_FT232R_A12345-if00-port0",
    "by_path": "platform-xhci-hcd.0-usb-0:1.2:1.0-port0"
  }
]
```

### open_session

```json
{
  "name": "open_session",
  "arguments": {
    "device": "/dev/ttyUSB0",
    "baudrate": 115200,
    "databits": 8,
    "stopbits": 1,
    "parity": "none",
    "flow": "none"
  }
}
```

Returns: `{ "session_id": "a1b2c3d4" }`

### send

```json
{
  "name": "send",
  "arguments": {
    "session_id": "a1b2c3d4",
    "data": "AT\\r\\n"
  }
}
```

Returns: `{ "sent": 3 }`

### read

```json
{
  "name": "read",
  "arguments": {
    "session_id": "a1b2c3d4",
    "max_wait_ms": 1000
  }
}
```

Returns:
```json
{
  "text": "AT\\r\\nOK\\r\\n",
  "base64": "QVRcDm9LXA==",
  "bytes": 7
}
```

### expect

```json
{
  "name": "expect",
  "arguments": {
    "session_id": "a1b2c3d4",
    "pattern": "OK|ERROR",
    "timeout_ms": 5000
  }
}
```

Returns:
```json
{
  "matched": "OK",
  "buffer": "AT\\r\\nOK\\r\\n"
}
```

If the timeout expires without a match:
```json
{ "matched": null, "buffer": "" }
```

### send_break

```json
{ "name": "send_break", "arguments": { "session_id": "a1b2c3d4" } }
```

### set_line

```json
{
  "name": "set_line",
  "arguments": {
    "session_id": "a1b2c3d4",
    "line": "dtr",
    "state": "high"
  }
}
```

`line`: `"dtr"` or `"rts"`
`state`: `"high"`, `"low"`, or `"toggle"`

### close_session

```json
{ "name": "close_session", "arguments": { "session_id": "a1b2c3d4" } }
```

Returns: `{ "ok": true }`

## Error Handling

All tool errors return a JSON-RPC error with a clear message:

| Code | Meaning |
|------|---------|
| `-32602` | Invalid params (missing/unknown session, bad arguments) |
| `-32603` | Internal error (device open failure, port I/O error) |

## Architecture

Sessions live in-process in a `HashMap<String, Session>` behind an `Arc<Mutex>`.
Opening a session spawns a background reader thread that continuously reads
from the port and appends incoming bytes to a shared ring buffer. This ensures
`read` and `expect` never lose data between tool calls.

```
tio mcp (stdio)
  |
  +-- Session Registry (Arc<Mutex<HashMap<String, Session>>>)
  |     |
  |     +-- Session { port, buffer } -- background reader thread
  |     +-- Session { port, buffer } -- background reader thread
  |     +-- ...
  |
  +-- JSON-RPC 2.0 dispatcher
        |
        +-- initialize
        +-- tools/list
        +-- tools/call --> list_ports | open_session | send | read | expect | ...
```
