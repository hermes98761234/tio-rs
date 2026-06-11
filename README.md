# 📟 tio-rs

[![CI](https://github.com/hermes98761234/tio-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/hermes98761234/tio-rs/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/hermes98761234/tio-rs)](https://github.com/hermes98761234/tio-rs/releases/latest)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-2021-orange.svg)](https://www.rust-lang.org/)

## What is it

tio-rs is a clean-room Rust rewrite of [tio](https://github.com/tio/tio), the simple serial device I/O tool for embedded developers. It preserves the core interactive terminal experience — auto-reconnect, device listing, hex modes, timestamps, logging, and TOML profiles — while adding first-class AI agent support: a non-interactive one-shot send/expect mode with JSON output, and a built-in MCP server that lets AI agents discover, open, read, write, and close serial sessions programmatically. Licensed MIT, no GPL code.

## Features

- **Serial terminal** — interactive raw TTY with 115200 8n1 defaults
- **Auto-reconnect** — reconnects on disconnect (disable with `-n`)
- **Auto-connect strategies** — `direct` (default), `new` (wait for hotplug), `latest` (most recently attached)
- **Device listing** — `tio -l` with path, driver, description, by-id/by-path; JSON output via `--json`
- **Hex modes** — input mode (`normal`/`hex`) and output mode (`normal`/`hex`)
- **Timestamps** — per-line with 6 formats: `24hour`, `24hour-start`, `24hour-delta`, `iso8601`, `epoch`, `epoch-usec`
- **Session logging** — to file with append and ANSI-strip options
- **TOML profiles** — `[default]` and `[profile.<name>]` with pattern-based auto-match
- **One-shot AI mode** — `--send`, `--expect`, `--timeout` with `--json` output for scripting
- **MCP server** — `tio mcp` exposes 8 tools over stdio for AI agents (Claude Code, Cursor, etc.)
- **Character mapping** — 13 input/output mapping flags (ICRNL, IGNCR, ONLCRNL, etc.)
- **ctrl-t key commands** — break, echo toggle, log toggle, mode cycling, line control, and more

## Installation

| Target | Download |
|--------|----------|
| x86_64 Linux | [tio-x86_64-unknown-linux-gnu](https://github.com/hermes98761234/tio-rs/releases/latest) |
| x86_64 macOS | [tio-x86_64-apple-darwin](https://github.com/hermes98761234/tio-rs/releases/latest) |
| aarch64 macOS | [tio-aarch64-apple-darwin](https://github.com/hermes98761234/tio-rs/releases/latest) |

Or install from source:

```bash
cargo install --git https://github.com/hermes98761234/tio-rs
```

## Usage

### Interactive mode

```bash
# Connect to a device (115200 8n1 default)
tio /dev/ttyUSB0

# Custom baud rate and device
tio -b 9600 /dev/ttyACM0

# Auto-connect to the most recently attached device
tio -a latest

# List serial ports as JSON
tio -l --json
```

### ctrl-t key commands

All commands are prefixed with `ctrl-t` (default, remappable). Double `ctrl-t` sends the literal character.

| Key | Action |
|-----|--------|
| `?` | List available key commands |
| `b` | Send serial break |
| `c` | Show configuration |
| `e` | Toggle local echo |
| `f` | Toggle log to file |
| `F` | Flush data I/O buffers |
| `g` | Toggle serial line (DTR/RTS/CTS/DSR/DCD/RI) |
| `i` | Toggle input mode (normal → hex → normal) |
| `l` | Clear screen |
| `L` | Show line states |
| `m` | Change character mapping |
| `o` | Toggle output mode (normal → hex → normal) |
| `p` | Pulse serial line |
| `q` | Quit |
| `R` | Execute shell command with I/O redirect |
| `s` | Show TX/RX statistics |
| `t` | Toggle timestamp mode |
| `v` | Show version |
| `ctrl-t` | Send literal ctrl-t character |

## Configuration

tio-rs reads TOML config from `$XDG_CONFIG_HOME/tio/config.toml` or `~/.config/tio/config.toml`.

```toml
[default]
baudrate = 115200
databits = 8
stopbits = 1
parity = "none"
flow = "none"
auto-connect = "direct"
no-reconnect = false
local-echo = false
timestamp = false
timestamp-format = "24hour"
input-mode = "normal"
output-mode = "normal"
log = false
log-file = ""
log-append = false
log-strip = false
color = "auto"

[profile.esp32]
pattern = "ttyUSB"
baudrate = 921600
```

The `[profile.<name>]` section activates when the device path matches the `pattern` substring. All keys match the CLI flags.

## AI usage

### One-shot send/expect

Send a command, wait for a response, get JSON output — perfect for scripts and AI agents:

```bash
# AT command with JSON result
tio /dev/ttyUSB0 --send 'AT\r' --expect 'OK|ERROR' --timeout 5 --json

# Send hex bytes
tio /dev/ttyUSB0 --send '\x1b\x5b\x32\x4a' --expect '.*' --timeout 3 --json

# Fire-and-forget (no expect)
tio /dev/ttyUSB0 --send 'ATZ\r' --timeout 2
```

**JSON output shape:**

```json
{
  "sent": "AT\\r",
  "received": "AT\\r\\r\\nOK\\r\\n",
  "matched": "AT\\r\\r\\nOK\\r\\n",
  "elapsed_ms": 42,
  "timeout": false
}
```

**Exit codes:** `0` = matched, `1` = timeout, `2` = error.

### MCP server

`tio mcp` runs a stdio MCP server (JSON-RPC 2.0, protocol version `2024-11-05`) with 8 tools:

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

**Claude Code registration:**

```bash
claude mcp add tio -- tio mcp
```

**Cursor** — add to `.cursor/mcp.json`:

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

## Architecture

```
tio-rs
├── cli          (clap argument parsing, subcommands)
├── config       (TOML config loading, profile matching)
├── serial       (serial port open/read/write/control)
├── list         (device enumeration, JSON output)
├── terminal     (interactive raw TTY, ctrl-t command loop)
├── format       (hex, timestamp, color, mapping pipeline)
├── log          (session logging, file management)
├── oneshot      (send/expect one-shot mode, JSON output)
└── mcp          (MCP stdio server, JSON-RPC 2.0, session registry)
```

## Differences from upstream tio

| Feature | Upstream tio | tio-rs |
|---------|-------------|--------|
| License | GPLv2 | MIT (clean-room) |
| Config format | INI (GKeyFile) | TOML |
| Lua scripting | Yes (`--script`) | Dropped — replaced by one-shot + MCP |
| X/Y-modem file transfer | Yes | Dropped (non-goal) |
| RS-485 | Yes | Dropped (non-goal) |
| Socket redirection (`-S`) | Yes | Dropped — MCP covers TTY sharing |
| Config `include` | Yes | Dropped |
| Shell completion helper | `--complete-profiles` | Dropped (clap handles natively) |
| JSON output | No | Yes (`--json` for list and one-shot) |
| One-shot send/expect | No | Yes (`--send`, `--expect`, `--timeout`) |
| MCP server | No | Yes (`tio mcp`, 8 tools) |

## Testing

All tests are hardware-free — serial port tests use PTY pairs via `nix::pty::openpty`.

```bash
cargo test
```

## License

MIT
