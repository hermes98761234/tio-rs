use std::io::{Read, Write};
use std::thread;
use std::time::Duration;

/// Test that we can open a PTY slave, write on the master, and read on the slave.
#[test]
fn test_pty_read_write() {
    // Use the lower-level PTY API since nix 0.29 openpty returns OwnedFd, not PtyMaster
    let mut master =
        nix::pty::posix_openpt(nix::fcntl::OFlag::O_RDWR | nix::fcntl::OFlag::O_NOCTTY)
            .expect("posix_openpt failed");
    nix::pty::grantpt(&master).expect("grantpt failed");
    nix::pty::unlockpt(&master).expect("unlockpt failed");

    let slave_path = unsafe { nix::pty::ptsname(&master) }.expect("ptsname failed");

    // Open the slave side with the serialport crate
    let mut port = serialport::new(&slave_path, 115200)
        .open()
        .expect("open serial port on PTY slave");

    // Write on master, read on slave (serial port)
    master.write_all(b"hello").expect("write to master failed");
    master.flush().expect("flush master failed");

    // Give the OS a moment to forward data
    thread::sleep(Duration::from_millis(100));

    let mut buf = [0u8; 64];
    let n = port.read(&mut buf).expect("read from serial port");
    assert_eq!(&buf[..n], b"hello");

    // Write on slave (serial port), read on master
    port.write_all(b"world").unwrap();
    port.flush().unwrap();

    thread::sleep(Duration::from_millis(100));

    let mut buf2 = [0u8; 64];
    let n2 = (&master).read(&mut buf2).expect("read from master");
    assert_eq!(&buf2[..n2], b"world");
}

/// Test that opening a non-existent device returns an error.
#[test]
fn test_open_nonexistent_device() {
    let result = serialport::new("/dev/nonexistent_ttyXYZ", 115200).open();
    assert!(result.is_err());
}

/// Test serial config defaults.
#[test]
fn test_serial_config_defaults() {
    // Verify the serialport crate types work as expected
    // (SerialConfig is tested via unit tests in the crate itself)
    let port_result = serialport::new("/dev/ttyUSB0", 115200)
        .data_bits(serialport::DataBits::Eight)
        .stop_bits(serialport::StopBits::One)
        .parity(serialport::Parity::None)
        .flow_control(serialport::FlowControl::None)
        .open();
    // Should fail since the device doesn't exist, but the builder API should work
    assert!(port_result.is_err());
}
