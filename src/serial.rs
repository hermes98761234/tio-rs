use std::io::{Read, Write};
use std::path::Path;
use std::thread;
use std::time::Duration;

use serialport::{DataBits, FlowControl, Parity, StopBits};

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
fn status_line(msg: &str) {
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

/// Open with reconnect loop. On read/write error, if reconnect is enabled,
/// polls for device reappearance every 500ms and reopens.
pub fn open_with_reconnect(
    cfg: &SerialConfig,
) -> Result<Box<dyn serialport::SerialPort>, serialport::Error> {
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
