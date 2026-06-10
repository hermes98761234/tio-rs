/// Interactive terminal mode.
///
/// When a device is given and no --send/--expect and stdin is a TTY:
/// - Put stdin in raw mode via nix termios, restore on exit (including panic hook).
/// - Two threads: stdin -> port (through keys.rs) and port -> stdout (through format.rs + log.rs).
/// - Print connect/disconnect status lines.
/// - Maintain RX/TX byte counters for the 's' command.
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;

use nix::sys::termios::{tcsetattr, SetArg, SpecialCharacterIndices, Termios};

use crate::format::OutputFormatter;
use crate::keys::{CtrlTStateMachine, TtyAction};
use crate::log::SessionLogger;
use crate::serial::{self, SerialConfig};

/// Restore termios on drop or panic.
struct TermiosGuard {
    original: Termios,
}

impl TermiosGuard {
    fn new() -> nix::Result<Self> {
        let stdin = std::io::stdin();
        let original = serial::get_termios(&stdin)?;
        Ok(Self { original })
    }
}

impl Drop for TermiosGuard {
    fn drop(&mut self) {
        let stdin_fd = std::io::stdin();
        let _ = tcsetattr(&stdin_fd, SetArg::TCSANOW, &self.original);
    }
}

/// Put stdin into raw mode, returning the guard that restores it.
fn enter_raw_mode() -> nix::Result<TermiosGuard> {
    let guard = TermiosGuard::new()?;
    let mut raw = guard.original.clone();

    // Disable canonical mode, echo, signal generation (LocalFlags)
    raw.local_flags &= !(nix::sys::termios::LocalFlags::ICANON
        | nix::sys::termios::LocalFlags::ECHO
        | nix::sys::termios::LocalFlags::ISIG
        | nix::sys::termios::LocalFlags::IEXTEN);

    // Disable input processing (InputFlags)
    raw.input_flags &= !(nix::sys::termios::InputFlags::IXON
        | nix::sys::termios::InputFlags::IXOFF
        | nix::sys::termios::InputFlags::ICRNL
        | nix::sys::termios::InputFlags::INLCR
        | nix::sys::termios::InputFlags::IGNCR);

    // Disable output processing
    raw.output_flags &= !nix::sys::termios::OutputFlags::OPOST;

    // Set 8-bit chars
    raw.control_flags &= !nix::sys::termios::ControlFlags::CSIZE;
    raw.control_flags |= nix::sys::termios::ControlFlags::CS8;

    // Read returns immediately with available data
    raw.control_chars[SpecialCharacterIndices::VMIN as usize] = 0;
    raw.control_chars[SpecialCharacterIndices::VTIME as usize] = 1;

    let stdin_fd = std::io::stdin();
    tcsetattr(&stdin_fd, SetArg::TCSANOW, &raw)?;
    Ok(guard)
}

/// Session statistics.
pub struct SessionStats {
    pub rx_bytes: AtomicU64,
    pub tx_bytes: AtomicU64,
}

impl SessionStats {
    pub fn new() -> Self {
        Self {
            rx_bytes: AtomicU64::new(0),
            tx_bytes: AtomicU64::new(0),
        }
    }

    pub fn rx_add(&self, n: u64) {
        self.rx_bytes.fetch_add(n, Ordering::Relaxed);
    }

    pub fn tx_add(&self, n: u64) {
        self.tx_bytes.fetch_add(n, Ordering::Relaxed);
    }

    pub fn rx(&self) -> u64 {
        self.rx_bytes.load(Ordering::Relaxed)
    }

    pub fn tx(&self) -> u64 {
        self.tx_bytes.load(Ordering::Relaxed)
    }
}

impl Default for SessionStats {
    fn default() -> Self {
        Self::new()
    }
}

/// Run the interactive session.
pub fn run_interactive(
    cfg: &SerialConfig,
    formatter: OutputFormatter,
    logger: SessionLogger,
    _timestamps: bool,
    _timestamp_format: Option<String>,
) -> std::io::Result<()> {
    let running = Arc::new(AtomicBool::new(true));
    let stats = Arc::new(SessionStats::new());

    // Open the serial port
    serial::status_line("Connected");

    let mut port = match serial::open_with_reconnect(cfg) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("Failed to open {}: {}", cfg.device, e);
            return Err(std::io::Error::other(e));
        }
    };

    // Enter raw mode
    let _guard = enter_raw_mode()
        .map_err(|e| std::io::Error::other(format!("raw mode: {}", e)))?;

    // Clone port for the read thread
    let mut read_port = port.try_clone().map_err(|e| {
        std::io::Error::other(format!("port clone: {}", e))
    })?;

    let cfg_clone = cfg.clone();
    let running_clone = Arc::clone(&running);
    let stats_clone = Arc::clone(&stats);

    // Move formatter and logger into the read thread
    let mut opt_formatter = Some(formatter);
    let mut opt_logger = Some(logger);

    // Read thread: port -> stdout
    let read_thread = thread::spawn(move || {
        let mut buf = [0u8; 256];
        let mut local_formatter = opt_formatter.take().unwrap();
        let mut local_logger = opt_logger.take().unwrap();
        while running_clone.load(Ordering::Relaxed) {
            match serial::read_with_reconnect(&mut read_port, &mut buf, &cfg_clone) {
                Ok(0) => continue,
                Ok(n) => {
                    stats_clone.rx_add(n as u64);

                    // Log
                    let _ = local_logger.write(&buf[..n]);
                    let _ = local_logger.flush();

                    // Format and output
                    let lines = local_formatter.format_received(&buf[..n]);
                    for line in lines {
                        let _ = std::io::stdout().write_all(line.as_bytes());
                    }
                    let _ = std::io::stdout().flush();
                }
                Err(_) => {
                    if !cfg_clone.reconnect {
                        break;
                    }
                }
            }
        }
        // Flush remaining hex buffer
        if let Some(line) = local_formatter.flush_hex() {
            let _ = std::io::stdout().write_all(line.as_bytes());
            let _ = std::io::stdout().flush();
        }
    });

    // Main thread: stdin -> port
    let mut stdin_buf = [0u8; 1];
    let mut key_sm = CtrlTStateMachine::new();
    // Track echo and logging state in the main thread
    let mut local_echo = false;
    let mut log_active = false;

    while running.load(Ordering::Relaxed) {
        match std::io::stdin().read(&mut stdin_buf) {
            Ok(0) => break,
            Ok(1) => {
                let action = key_sm.feed(stdin_buf[0]);
                handle_action(
                    action,
                    &mut key_sm,
                    &mut port,
                    cfg,
                    &mut local_echo,
                    &mut log_active,
                    &stats,
                    &running,
                    &mut stdin_buf,
                );
            }
            Ok(_) => {
                // Shouldn't happen with 1-byte buf, but handle gracefully
            }
            Err(_) => break,
        }
    }

    // Signal read thread to stop and wait
    running.store(false, Ordering::Relaxed);
    let _ = read_thread.join();

    serial::status_line("Disconnected");
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn handle_action(
    action: TtyAction,
    _key_sm: &mut CtrlTStateMachine,
    port: &mut Box<dyn serialport::SerialPort>,
    cfg: &SerialConfig,
    local_echo: &mut bool,
    log_active: &mut bool,
    stats: &SessionStats,
    running: &Arc<AtomicBool>,
    stdin_buf: &mut [u8; 1],
) {
    match action {
        TtyAction::None => {}
        TtyAction::Pass(byte) => {
            // Local echo
            if *local_echo {
                let buf = [byte];
                let _ = std::io::stdout().write_all(&buf);
                let _ = std::io::stdout().flush();
            }
            let _ = serial::write_with_reconnect(port, &[byte], cfg);
            stats.tx_add(1);
        }
        TtyAction::Quit => {
            running.store(false, Ordering::Relaxed);
        }
        TtyAction::SendBreak => {
            let _ = serial::send_break(port);
        }
        TtyAction::ShowConfig => {
            let msg = format!(
                "[tio] {} baud={} data={} stop={} parity={:?} flow={:?}\n",
                cfg.device, cfg.baudrate, cfg.databits, cfg.stopbits, cfg.parity, cfg.flow
            );
            let _ = std::io::stderr().write_all(msg.as_bytes());
        }
        TtyAction::ToggleEcho => {
            *local_echo = !*local_echo;
        }
        TtyAction::ToggleTimestamps => {
            // Note: timestamps are handled in the read thread's formatter.
            // For now this is a no-op in the main thread.
            // A channel or shared state would be needed for full support.
        }
        TtyAction::ToggleInputHex => {}
        TtyAction::ToggleOutputHex => {
            // Note: output mode is in the read thread's formatter.
        }
        TtyAction::ClearScreen => {
            let _ = std::io::stdout().write_all(b"\x1b[2J\x1b[H");
            let _ = std::io::stdout().flush();
        }
        TtyAction::PromptSignal => {
            let _ = std::io::stderr().write_all(b"[tio] Toggle (d)DTR (r)RTS: ");
            let _ = std::io::stderr().flush();
            if let Ok(1) = std::io::stdin().read(stdin_buf) {
                match stdin_buf[0] {
                    b'd' => {
                        let _ = serial::set_dtr(port, true);
                    }
                    b'r' => {
                        let _ = serial::set_rts(port, true);
                    }
                    _ => {}
                }
            }
        }
        TtyAction::ShowStats => {
            let msg = format!("[tio] RX: {} bytes, TX: {} bytes\n", stats.rx(), stats.tx());
            let _ = std::io::stderr().write_all(msg.as_bytes());
        }
        TtyAction::ToggleLogging => {
            *log_active = !*log_active;
        }
        TtyAction::FlushBuffers => {
            let _ = serial::flush_with_reconnect(port, cfg);
        }
        TtyAction::ShowVersion => {
            let _ = std::io::stderr().write_all(b"[tio] tio-rs 0.1.0\n");
        }
        TtyAction::Help => {
            let help = "\
[tio] ctrl-t commands:
  ?  help       q  quit       b  send break
  c  config     e  toggle echo
  t  timestamps i  input hex  o  output hex
  l  clear      g  DTR/RTS    s  stats
  f  log        F  flush      v  version
  ctrl-t ctrl-t  send literal ctrl-t
";
            let _ = std::io::stderr().write_all(help.as_bytes());
        }
        TtyAction::LiteralCtrlT => {
            let _ = serial::write_with_reconnect(port, &[0x14], cfg);
            stats.tx_add(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_stats() {
        let stats = SessionStats::new();
        assert_eq!(stats.rx(), 0);
        assert_eq!(stats.tx(), 0);
        stats.rx_add(42);
        stats.tx_add(10);
        assert_eq!(stats.rx(), 42);
        assert_eq!(stats.tx(), 10);
    }
}
