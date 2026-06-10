# tio-rs Porting Notes

> Clean-room Rust rewrite of [tio](https://github.com/tio/tio) (GPLv2).
> Behavior observed from upstream source at `/tmp/tio-ref` (commit `904a9a9`).
> No GPL code copied — this doc describes behavior only.

## Upstream Defaults

| Parameter        | Default          | Notes                               |
|------------------|------------------|-------------------------------------|
| Baud rate        | 115200           | Also supports non-standard rates    |
| Data bits        | 8 (range 5–8)    |                                     |
| Stop bits        | 1 (range 1–2)    |                                     |
| Parity           | none             | none/odd/even/mark/space            |
| Flow control     | none             | none/hard/soft                      |
| Reconnect        | on               | `-n` / `--no-reconnect` to disable  |
| Auto-connect     | direct           | new/latest/direct                   |
| Local echo       | off              | `-e` / `--local-echo`               |
| Timestamp        | off              | `-t` / `--timestamp`                |
| Timestamp format | 24hour           | Default when timestamps enabled     |
| Output mode      | normal           | normal/hex/hexN (N <= 4096)        |
| Input mode       | normal           | normal/hex/line                     |
| Color            | bold (code 256)  | 0-255 / bold / none / list          |
| Alert            | none             | none/bell/blink                     |
| Mute             | off              | `--mute` suppresses status msgs     |
| Prefix key       | ctrl-t (0x14)    | Remappable via `prefix-ctrl-key`    |

## Flag / Option Porting Status

| Upstream Flag                  | Short | tio-rs Status  | Notes                                          |
|--------------------------------|-------|----------------|------------------------------------------------|
| `--baudrate <bps>`             | `-b`  | **port**       | Default 115200, non-standard via u32            |
| `--databits 5|6|7|8`           | `-d`  | **port**       | Default 8                                      |
| `--stopbits 1|2`               | `-s`  | **port**       | Default 1                                      |
| `--parity odd|even|none|mark|space` | `-p` | **port**       | Default none. mark/space: pass through to serialport |
| `--flow hard|soft|none`         | `-f`  | **port**       | Default none                                   |
| `--output-delay <ms>`          | `-o`  | **port**       | Character delay on output                      |
| `--output-line-delay <ms>`     | `-O`  | **port**       | Line delay on output                           |
| `--line-pulse-duration <dur>`  |       | **port**       | Key=value pairs: DTR=200,RTS=150 etc.         |
| `--auto-connect new|latest|direct` | `-a` | **done**       | Default direct                                |
| `--exclude-devices <pattern>`  |       | **port**       | Glob `*` and `?`                               |
| `--exclude-drivers <pattern>`  |       | **port**       | Glob `*` and `?`                               |
| `--exclude-tids <pattern>`     |       | **port**       | Glob `*` and `?`                               |
| `--no-reconnect`               | `-n`  | **port**       | Default: reconnect on                          |
| `--local-echo`                 | `-e`  | **port**       | Default off                                    |
| `--input-mode normal|hex|line` |       | **port**       | Default normal                                |
| `--output-mode normal|hex|hexN`|       | **port**       | Default normal; hexN width <= 4096             |
| `--timestamp`                  | `-t`  | **port**       | Per-line timestamps                            |
| `--timestamp-format <fmt>`     |       | **port**       | 24hour/24hour-start/24hour-delta/iso8601/epoch/epoch-usec |
| `--timestamp-timeout <ms>`     |       | **port**       | Default 200; hex-mode idle timeout             |
| `--list`                       | `-l`  | **port**       | Device listing by device/id/path + profiles    |
| `--log`                        | `-L`  | **port**       | Enable session logging                         |
| `--log-file <name>`            |       | **port**       | Override auto-generated log filename           |
| `--log-directory <path>`       |       | **port**       | Directory for auto-named logs                  |
| `--log-append`                 |       | **port**       | Append instead of overwrite                    |
| `--log-strip`                  |       | **port**       | Strip control chars from log                   |
| `--map <flags>`                | `-m`  | **port**       | See mapping table below                        |
| `--color 0..255|bold|none|list` | `-c` | **port**       | Default bold                                  |
| `--socket <socket>`            | `-S`  | **drop**       | Replaced by MCP server mode for TTY sharing    |
| `--rs-485`                     |       | **drop**       | Non-goal: RS-485 not targeted for v0.1.0       |
| `--rs-485-config <config>`     |       | **drop**       | Non-goal                                      |
| `--alert bell|blink|none`      |       | **port**       | Visual/audible connect/disconnect alert        |
| `--mute`                       |       | **port**       | Suppress tio status messages                   |
| `--script <string>`            |       | **drop**       | Lua scripting replaced by one-shot mode + MCP  |
| `--script-file <file>`         |       | **drop**       | Non-goal                                      |
| `--script-run once|always|never`|      | **drop**       | Non-goal                                      |
| `--exec <command>`             |       | **port**       | Shell command with I/O redirect to device      |
| `--complete-profiles`          |       | **drop**       | Shell completion helper — not needed in Rust   |
| `--version`                    | `-v`  | **port**       |                                                |
| `--help`                       | `-h`  | **port**       |                                                |
| *(new)* `--json`               |       | **add**        | Structured JSON output for `--list` and one-shot |
| *(new)* `--send <data>`        |       | **add**        | One-shot send (with escape sequences)          |
| *(new)* `--expect <regex>`     |       | **add**        | One-shot expect with timeout                   |
| *(new)* `--timeout <secs>`     |       | **add**        | One-shot timeout (default 10)                  |
| *(new)* `mcp` subcommand       |       | **add**        | MCP stdio server for AI agents                 |

## Character Mapping Flags

All mapping flags from upstream are ported. They toggle on/off at runtime
(via ctrl-t m sub-command in upstream; CLI flag at startup in tio-rs).

| Flag        | Short | Direction | Description                                |
|-------------|-------|-----------|--------------------------------------------|
| `ICRNL`     | 0     | input     | Map CR to NL (unless IGNCR)                |
| `IGNCR`     | 1     | input     | Ignore CR                                  |
| `IFFESCC`   | 2     | input     | Map FF to ESC-c                            |
| `INLCR`     | 3     | input     | Map NL to CR                               |
| `INLCRNL`   | 4     | input     | Map NL to CR-NL                            |
| `ICRCRNL`   | 5     | input     | Map CR to CR-NL                            |
| `IMSB2LSB`  | 6     | input     | Map MSB bit order to LSB                   |
| `OCRNL`     | 7     | output    | Map CR to NL                               |
| `ODELBS`    | 8     | output    | Map DEL to BS                              |
| `ONLCRNL`   | 9     | output    | Map NL to CR-NL                            |
| `OLTU`      | a     | output    | Map lowercase to uppercase                 |
| `ONULBRK`   | b     | output    | Map NUL to break signal                    |
| `OIGNCR`    | c     | output    | Ignore CR                                  |

Comma-separated in `-m` flag (e.g. `-m INLCRNL,ODELBS`).

## ctrl-t Key Commands

All key commands use the prefix key (default: ctrl-t, remappable via
`prefix-ctrl-key` in config). Double prefix sends the prefix character literally.

| Key | Action                                         | Porting Status |
|-----|------------------------------------------------|----------------|
| `?` | List available key commands                    | **port**       |
| `b` | Send serial break                              | **port**       |
| `c` | Show configuration (baudrate, databits, etc.)   | **port**       |
| `e` | Toggle local echo                              | **port**       |
| `f` | Toggle log to file                             | **port**       |
| `F` | Flush data I/O buffers                         | **port**       |
| `g` | Toggle serial port line (prompts for 0-5)      | **port**       |
| `i` | Toggle input mode (normal→hex→line→normal)      | **port**       |
| `l` | Clear screen (ANSI reset)                      | **port**       |
| `L` | Show line states (DTR/RTS/CTS/DSR/DCD/RI)      | **port**       |
| `m` | Change mapping of characters (prompts 0-c)     | **port**       |
| `o` | Toggle output mode (normal→hex→normal)         | **port**       |
| `p` | Pulse serial port line (prompts for 0-5)       | **port**       |
| `q` | Quit                                           | **port**       |
| `r` | Run Lua script                                 | **drop** — replaced by one-shot + MCP |
| `R` | Execute shell command with I/O redirect        | **port**       |
| `s` | Show TX/RX statistics                         | **port**       |
| `t` | Toggle timestamp mode (cycles all formats)     | **port**       |
| `v` | Show version                                   | **port**       |
| `x` | Send/receive file via Xmodem                   | **drop** — non-goal |
| `y` | Send file via Ymodem                           | **drop** — non-goal |
| `ctrl-t` | Send literal ctrl-t character              | **port**       |

### Line toggle/pulse sub-commands (after ctrl-t g or ctrl-t p)

| Key | Line   |
|-----|--------|
| `0` | DTR    |
| `1` | RTS    |
| `2` | CTS    |
| `3` | DSR    |
| `4` | DCD    |
| `5` | RI     |

## Configuration File

**Upstream:** INI format via GLib GKeyFile.
**tio-rs:** TOML format (deliberate deviation — spec non-goal: INI compat).

### Search order (upstream)
1. `$XDG_CONFIG_HOME/tio/config`
2. `$HOME/.config/tio/config`
3. `$HOME/.tioconfig`

### tio-rs search order
1. `$XDG_CONFIG_HOME/tio/config.toml`
2. `~/.config/tio/config.toml`

### INI → TOML mapping

| INI section          | TOML equivalent       |
|----------------------|-----------------------|
| `[default]`          | `[default]`           |
| `[profile.<name>]`   | `[profile.<name>]`    |
| `include <file>`     | **drop** — no include support in v0.1.0 |

### Config keys

All keys present in upstream config files map 1:1 to TOML keys with the same
names and value types (strings, integers, booleans). Notable differences:
- INI uses bare `key = value`; TOML uses `key = "value"` (strings) or `key = 123` (integers)
- Boolean values: INI accepts `true`/`false`; TOML uses `true`/`false`
- No `include` directive in tio-rs TOML

## Timestamp Formats

| Format         | Example                        | Notes                            |
|----------------|--------------------------------|----------------------------------|
| `24hour`       | `14:02:53.269`                 | Default                          |
| `24hour-start` | Relative to session start      |                                  |
| `24hour-delta` | Relative to previous timestamp |                                  |
| `iso8601`      | `2026-06-10T14:02:53.269`     |                                  |
| `epoch`        | `1781127670.123`               | Seconds since Unix epoch         |
| `epoch-usec`   | `1781127670.123456`            | With microsecond subdivision     |

## Auto-Connect Strategies

| Strategy  | Behavior                                                  |
|-----------|-----------------------------------------------------------|
| `direct`  | Connect to specified device (default)                     |
| `new`     | Wait for first new device to appear, then connect         |
| `latest`  | Connect to most recently attached device (by mtime)       |

All strategies auto-reconnect on disconnect (unless `--no-reconnect`).

## Deliberate Deviations from Upstream

1. **TOML instead of INI** — cleaner syntax, widely used in Rust ecosystem.
2. **No Lua scripting** — replaced by one-shot send/expect mode + MCP server.
3. **No X/Y-modem** — non-goal for v0.1.0.
4. **No RS-485** — non-goal for v0.1.0.
5. **No socket redirection (`-S`)** — MCP server covers the TTY-sharing use case.
6. **No INI `include`** — simplifies config parsing.
7. **New AI features**: `--json`, `--send`, `--expect`, `--timeout`, `mcp` subcommand.
8. **No `--complete-profiles`** — upstream uses this for bash completion; clap handles completion natively in Rust.
9. **MIT license** — clean-room rewrite, no GPL code.
