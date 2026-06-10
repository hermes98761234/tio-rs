use std::io::{Read, Write};
use std::time::{Duration, Instant};

use regex::Regex;
use serde::Serialize;

use crate::serial::{self, SerialConfig};

/// Parse escape sequences in a --send string: \r \n \t \\ \xNN -> bytes.
pub fn parse_escapes(input: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len());
    let mut chars = input.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('r') => out.push(b'\r'),
                Some('n') => out.push(b'\n'),
                Some('t') => out.push(b'\t'),
                Some('\\') => out.push(b'\\'),
                Some('x') => {
                    let hi = chars.next();
                    let lo = chars.next();
                    if let (Some(h), Some(l)) = (hi, lo) {
                        let hex = format!("{}{}", h, l);
                        if let Ok(byte) = u8::from_str_radix(&hex, 16) {
                            out.push(byte);
                        } else {
                            // Invalid hex: emit literal \x and the chars
                            out.push(b'\\');
                            out.push(b'x');
                            out.push(h as u8);
                            out.push(l as u8);
                        }
                    } else {
                        out.push(b'\\');
                        out.push(b'x');
                        if let Some(h) = hi {
                            out.push(h as u8);
                        }
                    }
                }
                Some(other) => {
                    // Unknown escape: keep literal
                    out.push(b'\\');
                    out.push(other as u8);
                }
                None => {
                    out.push(b'\\');
                }
            }
        } else {
            // Encode the character as UTF-8
            let mut buf = [0u8; 4];
            let s = c.encode_utf8(&mut buf);
            out.extend_from_slice(s.as_bytes());
        }
    }
    out
}

/// JSON output for one-shot mode.
#[derive(Debug, Serialize)]
pub struct OneshotResult {
    pub sent: String,
    pub received: String,
    pub matched: Option<String>,
    pub elapsed_ms: u128,
    pub timeout: bool,
}

/// Run the one-shot send/expect engine.
///
/// Triggered when --send is given or stdin is not a TTY (piped).
///
/// Exit codes:
///   0 — regex matched (or no --expect and write succeeded)
///   1 — timeout without match
///   2 — device/open error
pub fn run(
    cfg: &SerialConfig,
    send_bytes: &[u8],
    send_original: &str,
    expect_regex: Option<&str>,
    timeout_secs: u64,
    json: bool,
) -> Result<OneshotResult, i32> {
    let start = Instant::now();

    // Open the port
    let mut port = match serial::open(cfg) {
        Ok(p) => p,
        Err(e) => {
            if !json {
                eprintln!("Failed to open {}: {}", cfg.device, e);
            }
            return Err(2);
        }
    };

    // Write the send bytes
    if let Err(e) = port.write_all(send_bytes) {
        if !json {
            eprintln!("Write error: {}", e);
        }
        return Err(2);
    }
    let _ = port.flush();

    // If no --expect, we're done after writing
    let expect = match expect_regex {
        Some(pattern) => match Regex::new(pattern) {
            Ok(re) => Some(re),
            Err(e) => {
                if !json {
                    eprintln!("Invalid regex: {}", e);
                }
                return Err(2);
            }
        },
        None => None,
    };

    let expect = match expect {
        Some(re) => re,
        None => {
            // No expect pattern — just write and exit 0
            return Ok(OneshotResult {
                sent: send_original.to_string(),
                received: String::new(),
                matched: None,
                elapsed_ms: start.elapsed().as_millis(),
                timeout: false,
            });
        }
    };

    // Read loop: accumulate buffer, check regex, timeout
    let timeout = Duration::from_secs(timeout_secs);
    let mut accumulated = Vec::new();
    let mut read_buf = [0u8; 1024];

    loop {
        if start.elapsed() >= timeout {
            let received = String::from_utf8_lossy(&accumulated).to_string();
            let result = OneshotResult {
                sent: send_original.to_string(),
                received,
                matched: None,
                elapsed_ms: start.elapsed().as_millis(),
                timeout: true,
            };
            if json {
                println!("{}", serde_json::to_string(&result).unwrap());
            }
            return Err(1);
        }

        // Calculate remaining timeout for this read attempt
        let remaining = timeout - start.elapsed();

        // Set read timeout on the port
        let _ = port.set_timeout(remaining);

        match port.read(&mut read_buf) {
            Ok(0) => {
                // No data this iteration — loop back and check timeout
                continue;
            }
            Ok(n) => {
                accumulated.extend_from_slice(&read_buf[..n]);

                let text = String::from_utf8_lossy(&accumulated);
                if let Some(m) = expect.find(&text) {
                    let received = text.to_string();
                    let result = OneshotResult {
                        sent: send_original.to_string(),
                        received,
                        matched: Some(m.as_str().to_string()),
                        elapsed_ms: start.elapsed().as_millis(),
                        timeout: false,
                    };
                    if json {
                        println!("{}", serde_json::to_string(&result).unwrap());
                    } else {
                        // Plain mode: write received bytes to stdout
                        let _ = std::io::stdout().write_all(&accumulated);
                    }
                    return Ok(result);
                }
            }
            Err(e)
                if e.kind() == std::io::ErrorKind::TimedOut
                    || e.kind() == std::io::ErrorKind::WouldBlock =>
            {
                // Read timed out — check overall timeout
                if start.elapsed() >= timeout {
                    let received = String::from_utf8_lossy(&accumulated).to_string();
                    let result = OneshotResult {
                        sent: send_original.to_string(),
                        received,
                        matched: None,
                        elapsed_ms: start.elapsed().as_millis(),
                        timeout: true,
                    };
                    if json {
                        println!("{}", serde_json::to_string(&result).unwrap());
                    }
                    return Err(1);
                }
                continue;
            }
            Err(e) => {
                if !json {
                    eprintln!("Read error: {}", e);
                }
                return Err(2);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- escape parser tests ---

    #[test]
    fn test_parse_escapes_plain() {
        assert_eq!(parse_escapes("hello"), b"hello");
    }

    #[test]
    fn test_parse_escapes_crlf() {
        assert_eq!(parse_escapes("AT\\r\\n"), b"AT\r\n");
    }

    #[test]
    fn test_parse_escapes_cr() {
        assert_eq!(parse_escapes("AT\\r"), b"AT\r");
    }

    #[test]
    fn test_parse_escapes_lf() {
        assert_eq!(parse_escapes("AT\\n"), b"AT\n");
    }

    #[test]
    fn test_parse_escapes_tab() {
        assert_eq!(parse_escapes("A\\tB"), b"A\tB");
    }

    #[test]
    fn test_parse_escapes_backslash() {
        assert_eq!(parse_escapes("A\\\\B"), b"A\\B");
    }

    #[test]
    fn test_parse_escapes_hex() {
        assert_eq!(parse_escapes("A\\x41B"), b"AAB");
    }

    #[test]
    fn test_parse_escapes_hex_00() {
        assert_eq!(parse_escapes("\\x00"), b"\x00");
    }

    #[test]
    fn test_parse_escapes_hex_ff() {
        assert_eq!(parse_escapes("\\xff"), b"\xff");
    }

    #[test]
    fn test_parse_escapes_mixed() {
        assert_eq!(parse_escapes("AT\\r\\n\\x00\\tOK"), b"AT\r\n\x00\tOK");
    }

    #[test]
    fn test_parse_escapes_empty() {
        assert!(parse_escapes("").is_empty());
    }

    #[test]
    fn test_parse_escapes_trailing_backslash() {
        assert_eq!(parse_escapes("A\\"), b"A\\");
    }

    #[test]
    fn test_parse_escapes_unknown_escape() {
        assert_eq!(parse_escapes("\\z"), b"\\z");
    }

    #[test]
    fn test_parse_escapes_hex_incomplete() {
        // \x4 followed by end of string
        let result = parse_escapes("\\x4");
        assert_eq!(result, b"\\x4");
    }

    #[test]
    fn test_parse_escapes_hex_invalid() {
        // \xGG is not valid hex
        let result = parse_escapes("\\xGG");
        assert_eq!(result, b"\\xGG");
    }

    #[test]
    fn test_parse_escapes_utf8() {
        let result = parse_escapes("привет");
        assert_eq!(result, "привет".as_bytes());
    }

    // --- OneshotResult JSON shape test ---

    #[test]
    fn test_oneshot_result_json_shape() {
        let result = OneshotResult {
            sent: "AT\\r".to_string(),
            received: "AT\\r\\r\\nOK\\r\\n".to_string(),
            matched: Some("AT\\r\\r\\nOK\\r\\n".to_string()),
            elapsed_ms: 42,
            timeout: false,
        };
        let json = serde_json::to_string(&result).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["sent"], "AT\\r");
        assert_eq!(parsed["received"], "AT\\r\\r\\nOK\\r\\n");
        assert_eq!(parsed["matched"], "AT\\r\\r\\nOK\\r\\n");
        assert_eq!(parsed["elapsed_ms"], 42);
        assert_eq!(parsed["timeout"], false);
    }

    #[test]
    fn test_oneshot_result_json_null_match() {
        let result = OneshotResult {
            sent: "AT\\r".to_string(),
            received: "".to_string(),
            matched: None,
            elapsed_ms: 10000,
            timeout: true,
        };
        let json = serde_json::to_string(&result).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(parsed["matched"].is_null());
        assert_eq!(parsed["timeout"], true);
    }
}
