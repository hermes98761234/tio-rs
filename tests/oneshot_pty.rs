use std::io::{Read, Write};
use std::sync::mpsc;
use std::sync::Mutex;
use std::thread;

use tio_rs::oneshot;

/// Serializes PTY-based tests to prevent FD inheritance issues.
static PTY_LOCK: Mutex<()> = Mutex::new(());

/// Helper: open a PTY pair, return (master_fd, slave_path).
fn open_pty_pair() -> (nix::pty::PtyMaster, String) {
    let master = nix::pty::posix_openpt(nix::fcntl::OFlag::O_RDWR | nix::fcntl::OFlag::O_NOCTTY)
        .expect("posix_openpt failed");
    nix::pty::grantpt(&master).expect("grantpt failed");
    nix::pty::unlockpt(&master).expect("unlockpt failed");
    let slave_path = unsafe { nix::pty::ptsname(&master) }.expect("ptsname failed");
    (master, slave_path)
}

/// Test: send "AT\r", expect "OK" pattern, responder replies "OK\r\n" when it sees "AT\r".
#[test]
fn test_oneshot_match_path() {
    let _guard = PTY_LOCK.lock().unwrap();
    let (mut master, slave_path) = open_pty_pair();

    // Spawn a responder thread on the master side
    let (tx, rx) = mpsc::channel::<()>();
    let handle = thread::spawn(move || {
        let mut buf = [0u8; 64];
        // Wait for data from the slave
        let n = (&master).read(&mut buf).expect("read from master");
        let received = String::from_utf8_lossy(&buf[..n]);
        assert!(received.contains("AT"), "responder got: {}", received);

        // Reply with OK\r\n
        master
            .write_all(b"OK\r\n")
            .expect("write response to master");
        master.flush().expect("flush master");

        // Wait for the oneshot engine to finish before dropping master
        let _ = rx.recv();
    });

    // Use the oneshot engine (opens the slave side itself)
    let send_str = "AT\\r";
    let send_bytes = oneshot::parse_escapes(send_str);
    let result = oneshot::run(
        &tio_rs::serial::SerialConfig::new(&slave_path),
        &send_bytes,
        send_str,
        Some("OK"),
        5,
        false,
    );

    // Signal the responder to finish
    let _ = tx.send(());
    handle.join().expect("responder thread panicked");

    let result = result.expect("oneshot run should succeed");
    assert!(!result.timeout, "should not timeout");
    assert!(result.matched.is_some(), "should have matched");
    assert_eq!(result.matched.unwrap(), "OK");
    assert!(result.received.contains("OK"), "received should contain OK");
}

/// Test: JSON output shape when --json is used.
#[test]
fn test_oneshot_json_output_shape() {
    let _guard = PTY_LOCK.lock().unwrap();
    let (mut master, slave_path) = open_pty_pair();

    let (tx, rx) = mpsc::channel::<()>();
    let handle = thread::spawn(move || {
        let mut buf = [0u8; 64];
        let n = (&master).read(&mut buf).expect("read from master");
        let received = String::from_utf8_lossy(&buf[..n]);
        assert!(received.contains("AT"), "responder got: {}", received);

        master
            .write_all(b"OK\r\n")
            .expect("write response to master");
        master.flush().expect("flush master");

        // Wait for the oneshot engine to finish
        let _ = rx.recv();
    });

    let send_str = "AT\\r";
    let send_bytes = oneshot::parse_escapes(send_str);
    let result = oneshot::run(
        &tio_rs::serial::SerialConfig::new(&slave_path),
        &send_bytes,
        send_str,
        Some("OK"),
        5,
        true, // json mode
    );

    let _ = tx.send(());
    handle.join().expect("responder thread panicked");

    let result = result.expect("oneshot run should succeed");

    // Verify JSON-serializable shape
    let json_str = serde_json::to_string(&result).expect("serialize to JSON");
    let parsed: serde_json::Value = serde_json::from_str(&json_str).expect("parse JSON output");

    assert_eq!(parsed["sent"], "AT\\r");
    assert!(parsed["received"].as_str().unwrap().contains("OK"));
    assert_eq!(parsed["matched"], "OK");
    assert_eq!(parsed["timeout"], false);
    assert!(parsed["elapsed_ms"].as_u64().is_some());
}

/// Test: timeout exit path — no response from responder.
#[test]
fn test_oneshot_timeout_path() {
    let _guard = PTY_LOCK.lock().unwrap();
    let (_master, slave_path) = open_pty_pair();
    // Keep master open (so slave can be opened) but don't respond
    let (_tx, rx) = mpsc::channel::<()>();
    // The master is moved into this thread and kept alive until the test ends
    let _handle = thread::spawn(move || {
        // Block forever — keeps master FD open
        let _ = rx.recv();
    });

    // Don't respond — just let it time out
    let send_str = "AT\\r";
    let send_bytes = oneshot::parse_escapes(send_str);
    let result = oneshot::run(
        &tio_rs::serial::SerialConfig::new(&slave_path),
        &send_bytes,
        send_str,
        Some("OK"),
        1,    // 1 second timeout
        true, // json mode
    );

    assert!(result.is_err(), "should fail with timeout");
    assert_eq!(result.unwrap_err(), 1, "exit code should be 1 (timeout)");
    // _handle is dropped here, which drops _tx, causing the thread to exit
}

/// Test: open error (non-existent device) returns exit code 2.
#[test]
fn test_oneshot_open_error() {
    let send_str = "AT\\r";
    let send_bytes = oneshot::parse_escapes(send_str);
    let result = oneshot::run(
        &tio_rs::serial::SerialConfig::new("/dev/nonexistent_ttyXYZ"),
        &send_bytes,
        send_str,
        Some("OK"),
        1,
        true,
    );

    assert!(result.is_err(), "should fail with open error");
    assert_eq!(
        result.unwrap_err(),
        2,
        "exit code should be 2 (device error)"
    );
}

/// Test: no --expect pattern — just write and succeed.
#[test]
fn test_oneshot_no_expect() {
    let _guard = PTY_LOCK.lock().unwrap();
    let (_master, slave_path) = open_pty_pair();
    // Keep master open so slave can be opened
    let (_tx, rx) = mpsc::channel::<()>();
    let _handle = thread::spawn(move || {
        let _ = rx.recv();
    });

    let send_str = "AT\\r";
    let send_bytes = oneshot::parse_escapes(send_str);
    let result = oneshot::run(
        &tio_rs::serial::SerialConfig::new(&slave_path),
        &send_bytes,
        send_str,
        None, // no expect
        5,
        false,
    );

    let result = result.expect("oneshot run should succeed without expect");
    assert!(!result.timeout);
    assert!(result.matched.is_none());
    assert_eq!(result.sent, "AT\\r");
}
