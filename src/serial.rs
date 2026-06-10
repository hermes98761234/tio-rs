use std::collections::HashSet;
use std::io::{Read, Write};
use std::path::Path;
use std::thread;
use std::time::Duration;

use serialport::{DataBits, FlowControl, Parity, StopBits};

use std::os::fd::AsFd;

use nix::sys::termios::tcgetattr;

use crate::cli::AutoConnect;

/// Get the current termios for a file descriptor.
pub fn get_termios(fd: &dyn AsFd) -> nix::Result<nix::sys::termios::Termios> {
    tcgetattr(fd)
}

/// Configuration for opening a serial port.
#[derive(Debug, Clone)]
pub struct SerialConfig {
    pub device: String,
    pub baudrate: u32,
    pub databits: u8,
    pub stopbits: u8,
    pub parity: Parity,
    pub flow: FlowControl,
    pub reconnect: bool,
}

impl SerialConfig {
    pub fn new(device: impl Into<String>) -> Self {
        Self {
            device: device.into(),
            baudrate: 115200,
            databits: 8,
            stopbits: 1,
            parity: Parity::None,
            flow: FlowControl::None,
            reconnect: true,
        }
    }
}

fn databits(d: u8) -> DataBits {
    match d {
        5 => DataBits::Five,
        6 => DataBits::Six,
        7 => DataBits::Seven,
        _ => DataBits::Eight,
    }
}

fn stopbits(s: u8) -> StopBits {
    match s {
        2 => StopBits::Two,
        _ => StopBits::One,
    }
}

/// Timestamped status line like `[tio HH:MM:SS] Connected`.
pub fn status_line(msg: &str) {
    let now = std::time::SystemTime::now();
    let dur = now
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = dur.as_secs();
    let h = (secs / 3600) % 24;
    let m = (secs / 60) % 60;
    let s = secs % 60;
    eprintln!("[tio {:02}:{:02}:{:02}] {}", h, m, s, msg);
}

/// Open a serial port from the given config.
pub fn open(cfg: &SerialConfig) -> Result<Box<dyn serialport::SerialPort>, serialport::Error> {
    let port = serialport::new(&cfg.device, cfg.baudrate)
        .data_bits(databits(cfg.databits))
        .stop_bits(stopbits(cfg.stopbits))
        .parity(cfg.parity)
        .flow_control(cfg.flow)
        .open()?;
    Ok(port)
}

/// Open with reconnect loop and auto-connect strategy.
///
/// For `AutoConnect::Direct` (the default), behaves like the original
/// `open_with_reconnect`: tries to open the given device, reconnects on error.
///
/// For `AutoConnect::New`, snapshots the current /dev tty devices, then polls
/// every 200ms for a new device to appear and connects to the first one found.
///
/// For `AutoConnect::Latest`, picks the most recently attached existing device
/// (by mtime of the device node).
pub fn open_with_reconnect(
    cfg: &SerialConfig,
    strategy: &AutoConnect,
) -> Result<Box<dyn serialport::SerialPort>, serialport::Error> {
    match strategy {
        AutoConnect::Direct => open_direct(cfg),
        AutoConnect::New => open_auto_new(cfg),
        AutoConnect::Latest => open_auto_latest(cfg),
    }
}

/// Direct strategy: open the given device, with reconnect loop.
fn open_direct(cfg: &SerialConfig) -> Result<Box<dyn serialport::SerialPort>, serialport::Error> {
    loop {
        match open(cfg) {
            Ok(port) => {
                status_line("Connected");
                return Ok(port);
            }
            Err(e) => {
                if !cfg.reconnect {
                    return Err(e);
                }
                status_line(&format!("Disconnected — {}", e));
                wait_for_device(&cfg.device);
            }
        }
    }
}

/// New strategy: snapshot /dev tty devices, poll every 200ms for a new device.
fn open_auto_new(cfg: &SerialConfig) -> Result<Box<dyn serialport::SerialPort>, serialport::Error> {
    let before = snapshot_tty_devices();
    status_line("Waiting for new device...");

    loop {
        thread::sleep(Duration::from_millis(200));
        let after = snapshot_tty_devices();
        let new_devices: Vec<String> = after.difference(&before).cloned().collect();

        for dev in &new_devices {
            let test_cfg = SerialConfig {
                device: dev.clone(),
                ..cfg.clone()
            };
            match open(&test_cfg) {
                Ok(port) => {
                    status_line(&format!("Connected to new device {}", dev));
                    return Ok(port);
                }
                Err(_) => {
                    // Keep polling
                }
            }
        }
    }
}

/// Latest strategy: pick the most recently attached device by mtime.
fn open_auto_latest(
    cfg: &SerialConfig,
) -> Result<Box<dyn serialport::SerialPort>, serialport::Error> {
    loop {
        let devices = snapshot_tty_devices();
        if let Some(latest) = pick_latest_device(&devices) {
            let test_cfg = SerialConfig {
                device: latest,
                ..cfg.clone()
            };
            match open(&test_cfg) {
                Ok(port) => {
                    status_line(&format!("Connected to latest device {}", test_cfg.device));
                    return Ok(port);
                }
                Err(_) => {
                    thread::sleep(Duration::from_millis(200));
                    continue;
                }
            }
        }
        status_line("Waiting for device...");
        thread::sleep(Duration::from_millis(200));
    }
}

/// Snapshot all /dev/tty* devices into a HashSet.
pub fn snapshot_tty_devices() -> HashSet<String> {
    let mut devices = HashSet::new();
    if let Ok(entries) = std::fs::read_dir("/dev") {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with("tty") {
                devices.insert(format!("/dev/{}", name));
            }
        }
    }
    devices
}

/// Pick the most recently attached device from a set, by mtime.
/// Pure function — easily testable.
pub fn pick_latest_device(devices: &HashSet<String>) -> Option<String> {
    let mut best: Option<(String, std::time::SystemTime)> = None;

    for dev in devices {
        if let Ok(meta) = std::fs::metadata(dev) {
            if let Ok(mtime) = meta.modified() {
                if best.as_ref().is_none_or(|(_, t)| mtime > *t) {
                    best = Some((dev.clone(), mtime));
                }
            }
        }
    }

    best.map(|(d, _)| d)
}

/// Auto-connect strategy selection — pure function for testability.
/// Given a strategy, a "before" snapshot, and an "after" snapshot,
/// returns the device to connect to, or None.
pub fn auto_connect_choice(
    strategy: &AutoConnect,
    before: &HashSet<String>,
    after: &HashSet<String>,
) -> Option<String> {
    match strategy {
        AutoConnect::Direct => None,
        AutoConnect::New => {
            // Pick the first new device (sorted for determinism in tests)
            let mut new_devs: Vec<String> = after.difference(before).cloned().collect();
            new_devs.sort();
            new_devs.first().cloned()
        }
        AutoConnect::Latest => pick_latest_device(after),
    }
}

/// Poll for device reappearance every 500ms.
fn wait_for_device(device: &str) {
    loop {
        if Path::new(device).exists() {
            return;
        }
        thread::sleep(Duration::from_millis(500));
    }
}

/// Set RTS line.
pub fn set_rts(port: &mut Box<dyn serialport::SerialPort>, high: bool) -> serialport::Result<()> {
    if high {
        port.write_request_to_send(true)
    } else {
        port.write_request_to_send(false)
    }
}

/// Set DTR line.
pub fn set_dtr(port: &mut Box<dyn serialport::SerialPort>, high: bool) -> serialport::Result<()> {
    port.write_data_terminal_ready(high)
}

/// Send a break signal: set_break, sleep 100ms, clear_break.
pub fn send_break(port: &mut Box<dyn serialport::SerialPort>) -> serialport::Result<()> {
    port.set_break()?;
    thread::sleep(Duration::from_millis(100));
    port.clear_break()
}

/// Read with reconnect: on error, if reconnect enabled, wait and reopen.
pub fn read_with_reconnect(
    port: &mut Box<dyn serialport::SerialPort>,
    buf: &mut [u8],
    cfg: &SerialConfig,
) -> std::io::Result<usize> {
    loop {
        match port.read(buf) {
            Ok(n) => return Ok(n),
            Err(e) => {
                if !cfg.reconnect {
                    return Err(e);
                }
                status_line(&format!("Disconnected — {}", e));
                wait_for_device(&cfg.device);
                match open(cfg) {
                    Ok(new_port) => {
                        *port = new_port;
                        status_line("Connected");
                    }
                    Err(_) => {
                        wait_for_device(&cfg.device);
                    }
                }
            }
        }
    }
}

/// Write with reconnect: on error, if reconnect enabled, wait and reopen.
pub fn write_with_reconnect(
    port: &mut Box<dyn serialport::SerialPort>,
    buf: &[u8],
    cfg: &SerialConfig,
) -> std::io::Result<usize> {
    loop {
        match port.write(buf) {
            Ok(n) => return Ok(n),
            Err(e) => {
                if !cfg.reconnect {
                    return Err(e);
                }
                status_line(&format!("Disconnected — {}", e));
                wait_for_device(&cfg.device);
                match open(cfg) {
                    Ok(new_port) => {
                        *port = new_port;
                        status_line("Connected");
                    }
                    Err(_) => {
                        wait_for_device(&cfg.device);
                    }
                }
            }
        }
    }
}

/// Flush with reconnect.
pub fn flush_with_reconnect(
    port: &mut Box<dyn serialport::SerialPort>,
    cfg: &SerialConfig,
) -> std::io::Result<()> {
    loop {
        match port.flush() {
            Ok(()) => return Ok(()),
            Err(e) => {
                if !cfg.reconnect {
                    return Err(e);
                }
                status_line(&format!("Disconnected — {}", e));
                wait_for_device(&cfg.device);
                match open(cfg) {
                    Ok(new_port) => {
                        *port = new_port;
                        status_line("Connected");
                    }
                    Err(_) => {
                        wait_for_device(&cfg.device);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_set(items: &[&str]) -> HashSet<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn test_auto_connect_direct_returns_none() {
        let before = make_set(&["/dev/ttyUSB0"]);
        let after = make_set(&["/dev/ttyUSB0", "/dev/ttyUSB1"]);
        let result = auto_connect_choice(&AutoConnect::Direct, &before, &after);
        assert!(result.is_none());
    }

    #[test]
    fn test_auto_connect_new_detects_new_device() {
        let before = make_set(&["/dev/ttyUSB0"]);
        let after = make_set(&["/dev/ttyUSB0", "/dev/ttyUSB1"]);
        let result = auto_connect_choice(&AutoConnect::New, &before, &after);
        assert_eq!(result, Some("/dev/ttyUSB1".to_string()));
    }

    #[test]
    fn test_auto_connect_new_no_new_devices() {
        let before = make_set(&["/dev/ttyUSB0"]);
        let after = make_set(&["/dev/ttyUSB0"]);
        let result = auto_connect_choice(&AutoConnect::New, &before, &after);
        assert!(result.is_none());
    }

    #[test]
    fn test_auto_connect_new_multiple_new_picks_first_sorted() {
        let before = make_set(&["/dev/ttyUSB0"]);
        let after = make_set(&["/dev/ttyUSB0", "/dev/ttyACM0", "/dev/ttyUSB1"]);
        let result = auto_connect_choice(&AutoConnect::New, &before, &after);
        // Sorted: ttyACM0 < ttyUSB1
        assert_eq!(result, Some("/dev/ttyACM0".to_string()));
    }

    #[test]
    fn test_auto_connect_latest_empty() {
        let before = make_set(&[]);
        let after = make_set(&[]);
        let result = auto_connect_choice(&AutoConnect::Latest, &before, &after);
        assert!(result.is_none());
    }

    #[test]
    fn test_pick_latest_device_empty() {
        let devices: HashSet<String> = HashSet::new();
        assert!(pick_latest_device(&devices).is_none());
    }

    #[test]
    fn test_pick_latest_device_returns_some() {
        // Real /dev/tty devices exist on Linux; just verify it returns Some for non-empty set
        let devices = make_set(&["/dev/tty", "/dev/tty0"]);
        let result = pick_latest_device(&devices);
        // Should return one of them (the one with the newest mtime)
        assert!(result.is_some());
        let result = result.unwrap();
        assert!(devices.contains(&result));
    }
}
