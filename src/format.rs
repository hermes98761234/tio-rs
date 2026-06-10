/// Output formatting pipeline.
///
/// Pure functions over byte chunks: normal/hex output, per-line timestamps,
/// character mapping (ICRNL, INLCR, OCRNL, ONLCR, ODELBS), local echo.
use std::collections::HashMap;

/// Output mode for received data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputMode {
    Normal,
    Hex,
}

/// Character mapping directions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CharMap {
    ICRNL,  // CR -> NL on input
    INLCR,  // NL -> CR on input
    OCRNL,  // CR -> NL on output
    ONLCR,  // NL -> CR on output
    ODELBS, // DEL -> BS on output
}

/// Parses a comma-separated map string like "ICRNL,INLCR,OCRNL,ONLCR,ODELBS".
/// Also supports short forms: "cr:nl" for ICRNL, "nl:cr" for INLCR, "del:bs" for ODELBS.
pub fn parse_char_map(s: &str) -> HashMap<CharMap, CharMap> {
    let mut result = HashMap::new();
    for item in s.split(',') {
        let item = item.trim();
        if item.is_empty() {
            continue;
        }
        match item.to_lowercase().as_str() {
            "icrnl" | "cr:nl" | "crnl" => {
                result.insert(CharMap::ICRNL, CharMap::ICRNL);
            }
            "inlcr" | "nl:cr" | "nlcr" => {
                result.insert(CharMap::INLCR, CharMap::INLCR);
            }
            "ocrnl" => {
                result.insert(CharMap::OCRNL, CharMap::OCRNL);
            }
            "onlcr" => {
                result.insert(CharMap::ONLCR, CharMap::ONLCR);
            }
            "odelbs" | "del:bs" => {
                result.insert(CharMap::ODELBS, CharMap::ODELBS);
            }
            _ => {}
        }
    }
    result
}

/// Apply output-side character mappings to a byte.
pub fn apply_output_char_map(byte: u8, maps: &HashMap<CharMap, CharMap>) -> u8 {
    if maps.contains_key(&CharMap::OCRNL) && byte == b'\r' {
        return b'\n';
    }
    if maps.contains_key(&CharMap::ONLCR) && byte == b'\n' {
        return b'\r';
    }
    if maps.contains_key(&CharMap::ODELBS) && byte == 0x7f {
        return 0x08; // DEL -> BS
    }
    byte
}

/// Apply input-side character mappings to a byte.
pub fn apply_input_char_map(byte: u8, maps: &HashMap<CharMap, CharMap>) -> u8 {
    if maps.contains_key(&CharMap::ICRNL) && byte == b'\r' {
        return b'\n';
    }
    if maps.contains_key(&CharMap::INLCR) && byte == b'\n' {
        return b'\r';
    }
    byte
}

/// Format a line of bytes as hex: lowercase pairs "ff " 16 per line.
pub fn format_hex_line(bytes: &[u8]) -> String {
    let mut result = String::with_capacity(bytes.len() * 3);
    for (i, b) in bytes.iter().enumerate() {
        if i > 0 {
            result.push(' ');
        }
        result.push_str(&format!("{:02x}", b));
    }
    result
}

/// Format a complete hex dump of a byte buffer, 16 bytes per line.
pub fn format_hex_dump(bytes: &[u8]) -> String {
    let mut result = String::new();
    for chunk in bytes.chunks(16) {
        if !result.is_empty() {
            result.push('\n');
        }
        result.push_str(&format_hex_line(chunk));
    }
    result
}

/// Generate a timestamp prefix "[HH:MM:SS.mmm]" from a SystemTime.
pub fn timestamp_prefix(time: &std::time::SystemTime, format_str: Option<&str>) -> String {
    let duration = time
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = duration.as_secs();
    let millis = duration.subsec_millis();
    let h = (secs / 3600) % 24;
    let m = (secs / 60) % 60;
    let s = secs % 60;

    if let Some(fmt) = format_str {
        // Simple strftime-like formatting: %H=hour, %M=min, %S=sec, %f=millis
        let mut result = String::new();
        let mut chars = fmt.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '%' {
                match chars.next() {
                    Some('H') => result.push_str(&format!("{:02}", h)),
                    Some('M') => result.push_str(&format!("{:02}", m)),
                    Some('S') => result.push_str(&format!("{:02}", s)),
                    Some('f') => result.push_str(&format!("{:03}", millis)),
                    Some(other) => {
                        result.push('%');
                        result.push(other);
                    }
                    None => {
                        result.push('%');
                    }
                }
            } else {
                result.push(c);
            }
        }
        format!("[{}]", result)
    } else {
        format!("[{:02}:{:02}:{:02}.{:03}]", h, m, s, millis)
    }
}

/// The output formatter state.
#[derive(Debug, Clone)]
pub struct OutputFormatter {
    pub mode: OutputMode,
    pub timestamps: bool,
    pub timestamp_format: Option<String>,
    pub char_maps: HashMap<CharMap, CharMap>,
    pub local_echo: bool,
    hex_buf: Vec<u8>,
}

impl OutputFormatter {
    pub fn new() -> Self {
        Self {
            mode: OutputMode::Normal,
            timestamps: false,
            timestamp_format: None,
            char_maps: HashMap::new(),
            local_echo: false,
            hex_buf: Vec::new(),
        }
    }

    /// Process a chunk of received bytes, returning formatted output lines.
    /// Each line is returned as a complete string ready to write to stdout.
    pub fn format_received(&mut self, bytes: &[u8]) -> Vec<String> {
        let mut lines = Vec::new();
        let timestamp = if self.timestamps {
            Some(timestamp_prefix(
                &std::time::SystemTime::now(),
                self.timestamp_format.as_deref(),
            ))
        } else {
            None
        };

        match self.mode {
            OutputMode::Normal => {
                // Process byte by byte, splitting on newlines for timestamps
                let mut current_line = Vec::new();
                for &b in bytes {
                    let mapped = apply_output_char_map(b, &self.char_maps);
                    current_line.push(mapped);
                    if mapped == b'\n' {
                        let mut line = String::new();
                        if let Some(ref ts) = timestamp {
                            line.push_str(ts);
                            line.push(' ');
                        }
                        line.push_str(&String::from_utf8_lossy(&current_line));
                        lines.push(line);
                        current_line.clear();
                    }
                }
                if !current_line.is_empty() {
                    let mut line = String::new();
                    if let Some(ref ts) = timestamp {
                        line.push_str(ts);
                        line.push(' ');
                    }
                    line.push_str(&String::from_utf8_lossy(&current_line));
                    lines.push(line);
                }
            }
            OutputMode::Hex => {
                self.hex_buf.extend_from_slice(bytes);
                while self.hex_buf.len() >= 16 {
                    let chunk: Vec<u8> = self.hex_buf.drain(..16).collect();
                    let mut line = String::new();
                    if let Some(ref ts) = timestamp {
                        line.push_str(ts);
                        line.push(' ');
                    }
                    line.push_str(&format_hex_line(&chunk));
                    lines.push(line);
                }
            }
        }
        lines
    }

    /// Flush any remaining hex buffer (called at end of session).
    pub fn flush_hex(&mut self) -> Option<String> {
        if self.mode == OutputMode::Hex && !self.hex_buf.is_empty() {
            let mut line = String::new();
            if self.timestamps {
                line.push_str(&timestamp_prefix(
                    &std::time::SystemTime::now(),
                    self.timestamp_format.as_deref(),
                ));
                line.push(' ');
            }
            line.push_str(&format_hex_line(&self.hex_buf));
            self.hex_buf.clear();
            Some(line)
        } else {
            None
        }
    }

    /// Format an echoed byte for local echo.
    pub fn format_echo(&self, byte: u8) -> Option<String> {
        if !self.local_echo {
            return None;
        }
        let mapped = apply_output_char_map(byte, &self.char_maps);
        Some(String::from_utf8_lossy(&[mapped]).to_string())
    }
}

impl Default for OutputFormatter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- parse_char_map ---

    #[test]
    fn test_parse_char_map_empty() {
        let m = parse_char_map("");
        assert!(m.is_empty());
    }

    #[test]
    fn test_parse_char_map_single() {
        let m = parse_char_map("cr:nl");
        assert!(m.contains_key(&CharMap::ICRNL));
    }

    #[test]
    fn test_parse_char_map_multiple() {
        let m = parse_char_map("cr:nl,nl:cr,del:bs");
        assert!(m.contains_key(&CharMap::ICRNL));
        assert!(m.contains_key(&CharMap::INLCR));
        assert!(m.contains_key(&CharMap::ODELBS));
    }

    // --- apply_output_char_map ---

    #[test]
    fn test_output_map_ocrnl() {
        let mut maps = HashMap::new();
        maps.insert(CharMap::OCRNL, CharMap::OCRNL);
        assert_eq!(apply_output_char_map(b'\r', &maps), b'\n');
        assert_eq!(apply_output_char_map(b'A', &maps), b'A');
    }

    #[test]
    fn test_output_map_onlcr() {
        let mut maps = HashMap::new();
        maps.insert(CharMap::ONLCR, CharMap::ONLCR);
        assert_eq!(apply_output_char_map(b'\n', &maps), b'\r');
        assert_eq!(apply_output_char_map(b'B', &maps), b'B');
    }

    #[test]
    fn test_output_map_odelbs() {
        let mut maps = HashMap::new();
        maps.insert(CharMap::ODELBS, CharMap::ODELBS);
        assert_eq!(apply_output_char_map(0x7f, &maps), 0x08);
        assert_eq!(apply_output_char_map(b'C', &maps), b'C');
    }

    #[test]
    fn test_output_map_empty() {
        let maps = HashMap::new();
        assert_eq!(apply_output_char_map(b'\r', &maps), b'\r');
    }

    // --- apply_input_char_map ---

    #[test]
    fn test_input_map_icrnl() {
        let mut maps = HashMap::new();
        maps.insert(CharMap::ICRNL, CharMap::ICRNL);
        assert_eq!(apply_input_char_map(b'\r', &maps), b'\n');
    }

    #[test]
    fn test_input_map_inlcr() {
        let mut maps = HashMap::new();
        maps.insert(CharMap::INLCR, CharMap::INLCR);
        assert_eq!(apply_input_char_map(b'\n', &maps), b'\r');
    }

    // --- format_hex_line ---

    #[test]
    fn test_format_hex_line_empty() {
        assert_eq!(format_hex_line(&[]), "");
    }

    #[test]
    fn test_format_hex_line_single() {
        assert_eq!(format_hex_line(&[0xff]), "ff");
    }

    #[test]
    fn test_format_hex_line_multiple() {
        assert_eq!(format_hex_line(&[0x0a, 0x1b, 0xff]), "0a 1b ff");
    }

    #[test]
    fn test_format_hex_line_16_bytes() {
        let bytes: Vec<u8> = (0..16).collect();
        let result = format_hex_line(&bytes);
        assert_eq!(result, "00 01 02 03 04 05 06 07 08 09 0a 0b 0c 0d 0e 0f");
    }

    // --- format_hex_dump ---

    #[test]
    fn test_format_hex_dump_17_bytes() {
        let bytes: Vec<u8> = (0..17).collect();
        let result = format_hex_dump(&bytes);
        let lines: Vec<&str> = result.lines().collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], "00 01 02 03 04 05 06 07 08 09 0a 0b 0c 0d 0e 0f");
        assert_eq!(lines[1], "10");
    }

    // --- timestamp_prefix ---

    #[test]
    fn test_timestamp_default_format() {
        // 2h 1m 1s = 7261 seconds
        let t = std::time::UNIX_EPOCH
            + std::time::Duration::from_secs(7261)
            + std::time::Duration::from_millis(123);
        let prefix = timestamp_prefix(&t, None);
        assert_eq!(prefix, "[02:01:01.123]");
    }

    #[test]
    fn test_timestamp_custom_format() {
        // 3h 1m 1s = 10861 seconds
        let t = std::time::UNIX_EPOCH + std::time::Duration::from_secs(10861);
        let prefix = timestamp_prefix(&t, Some("%H:%M:%S"));
        assert_eq!(prefix, "[03:01:01]");
    }

    #[test]
    fn test_timestamp_custom_format_with_millis() {
        let t = std::time::UNIX_EPOCH + std::time::Duration::from_millis(7_000);
        let prefix = timestamp_prefix(&t, Some("%S.%f"));
        assert_eq!(prefix, "[07.000]");
    }

    // --- OutputFormatter ---

    #[test]
    fn test_formatter_normal_mode() {
        let mut fmt = OutputFormatter::new();
        let lines = fmt.format_received(b"hello\n");
        assert_eq!(lines, vec!["hello\n"]);
    }

    #[test]
    fn test_formatter_normal_no_newline() {
        let mut fmt = OutputFormatter::new();
        let lines = fmt.format_received(b"hello");
        assert_eq!(lines, vec!["hello"]);
    }

    #[test]
    fn test_formatter_hex_mode() {
        let mut fmt = OutputFormatter::new();
        fmt.mode = OutputMode::Hex;
        let bytes: Vec<u8> = (0..16).collect();
        let lines = fmt.format_received(&bytes);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0], "00 01 02 03 04 05 06 07 08 09 0a 0b 0c 0d 0e 0f");
    }

    #[test]
    fn test_formatter_hex_partial_no_output() {
        let mut fmt = OutputFormatter::new();
        fmt.mode = OutputMode::Hex;
        let lines = fmt.format_received(&[0x01, 0x02, 0x03]);
        assert!(lines.is_empty());
    }

    #[test]
    fn test_formatter_hex_flush() {
        let mut fmt = OutputFormatter::new();
        fmt.mode = OutputMode::Hex;
        fmt.format_received(&[0x01, 0x02]);
        let flushed = fmt.flush_hex();
        assert!(flushed.is_some());
        assert!(flushed.unwrap().contains("01 02"));
    }

    #[test]
    fn test_formatter_timestamps() {
        let mut fmt = OutputFormatter::new();
        fmt.timestamps = true;
        let lines = fmt.format_received(b"hello\n");
        assert_eq!(lines.len(), 1);
        // Should start with [HH:MM:SS.mmm]
        assert!(lines[0].starts_with('['));
        assert!(lines[0].contains(" hello\n"));
    }

    #[test]
    fn test_formatter_echo_on() {
        let fmt = OutputFormatter {
            local_echo: true,
            ..OutputFormatter::new()
        };
        let echo = fmt.format_echo(b'A');
        assert_eq!(echo, Some("A".to_string()));
    }

    #[test]
    fn test_formatter_echo_off() {
        let fmt = OutputFormatter::new();
        let echo = fmt.format_echo(b'A');
        assert!(echo.is_none());
    }

    #[test]
    fn test_formatter_output_map_integration() {
        let mut fmt = OutputFormatter::new();
        let mut maps = HashMap::new();
        maps.insert(CharMap::OCRNL, CharMap::OCRNL);
        fmt.char_maps = maps;
        let lines = fmt.format_received(b"ab\r");
        assert_eq!(lines, vec!["ab\n"]);
    }
}
