//! PTY-backed integration tests for the output pipeline.

#[test]
fn test_pty_format_pipeline_normal() {
    // Test the format pipeline directly with byte slices (no PTY needed)
    let mut formatter = tio_rs::format::OutputFormatter::new();
    let lines = formatter.format_received(b"test data\n");
    assert_eq!(lines.len(), 1);
    assert!(lines[0].contains("test data"));
}

#[test]
fn test_pty_format_pipeline_hex() {
    let mut formatter = tio_rs::format::OutputFormatter::new();
    formatter.mode = tio_rs::format::OutputMode::Hex;

    let data: Vec<u8> = (0..16).collect();
    let lines = formatter.format_received(&data);
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0], "00 01 02 03 04 05 06 07 08 09 0a 0b 0c 0d 0e 0f");
}

#[test]
fn test_pty_format_pipeline_hex_partial() {
    let mut formatter = tio_rs::format::OutputFormatter::new();
    formatter.mode = tio_rs::format::OutputMode::Hex;

    // Write 20 bytes: should produce 1 full line + 4 bytes buffered
    let data: Vec<u8> = (0..20).collect();
    let lines = formatter.format_received(&data);
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0], "00 01 02 03 04 05 06 07 08 09 0a 0b 0c 0d 0e 0f");

    // Flush remaining
    let flushed = formatter.flush_hex();
    assert!(flushed.is_some());
    assert!(flushed.unwrap().contains("10 11 12 13"));
}

#[test]
fn test_pty_char_mapping_output() {
    let mut formatter = tio_rs::format::OutputFormatter::new();
    let mut maps = std::collections::HashMap::new();
    maps.insert(
        tio_rs::format::CharMap::OCRNL,
        tio_rs::format::CharMap::OCRNL,
    );
    formatter.char_maps = maps;

    let lines = formatter.format_received(b"ab\r\n");
    // \r maps to \n via OCRNL, so "ab\r\n" becomes "ab\n\n" which is 2 lines
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0], "ab\n");
    assert_eq!(lines[1], "\n");
}

#[test]
fn test_pty_format_with_timestamps() {
    let mut formatter = tio_rs::format::OutputFormatter::new();
    formatter.timestamps = true;

    let lines = formatter.format_received(b"hello\n");
    assert_eq!(lines.len(), 1);
    // Should start with [HH:MM:SS.mmm]
    assert!(lines[0].starts_with('['));
    assert!(lines[0].contains(" hello\n"));
}

#[test]
fn test_pty_key_state_machine() {
    let mut sm = tio_rs::keys::CtrlTStateMachine::new();

    assert_eq!(sm.feed(b'A'), tio_rs::keys::TtyAction::Pass(b'A'));
    assert_eq!(sm.feed(0x14), tio_rs::keys::TtyAction::None);
    assert_eq!(sm.feed(b'q'), tio_rs::keys::TtyAction::Quit);

    assert_eq!(sm.feed(0x14), tio_rs::keys::TtyAction::None);
    assert_eq!(sm.feed(0x14), tio_rs::keys::TtyAction::LiteralCtrlT);
}

#[test]
fn test_pty_log_write_strip() {
    let path = "/tmp/tio_pty_test_strip.log";
    {
        let mut logger =
            tio_rs::log::SessionLogger::new(Some(std::path::Path::new(path)), false, true).unwrap();
        logger.write(b"abc\x01\x02\n\x03def\n").unwrap();
    }

    let content = std::fs::read_to_string(path).unwrap();
    assert_eq!(content, "abc\ndef\n");

    let _ = std::fs::remove_file(path);
}

#[test]
fn test_pty_session_stats() {
    let stats = tio_rs::terminal::SessionStats::new();
    assert_eq!(stats.rx(), 0);
    assert_eq!(stats.tx(), 0);
    stats.rx_add(100);
    stats.tx_add(50);
    assert_eq!(stats.rx(), 100);
    assert_eq!(stats.tx(), 50);
}

#[test]
fn test_pty_parse_char_map_combined() {
    let maps = tio_rs::format::parse_char_map("ICRNL,OCRNL,ODELBS");
    assert_eq!(maps.len(), 3);
    assert!(maps.contains_key(&tio_rs::format::CharMap::ICRNL));
    assert!(maps.contains_key(&tio_rs::format::CharMap::OCRNL));
    assert!(maps.contains_key(&tio_rs::format::CharMap::ODELBS));
}

#[test]
fn test_pty_auto_name_format() {
    let mut logger = tio_rs::log::SessionLogger::new(None, false, false).unwrap();
    logger.auto_name("/dev/ttyUSB0", false, false).unwrap();
    let path = logger.path().unwrap();
    let filename = path.file_name().unwrap().to_str().unwrap();
    assert!(filename.starts_with("tio_ttyUSB0_"));
    assert!(filename.ends_with(".log"));
    let _ = std::fs::remove_file(path);
}
