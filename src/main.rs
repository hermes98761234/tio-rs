use clap::Parser;
use tio_rs::cli::Cli;
use tio_rs::format::{self, OutputFormatter, OutputMode};
use tio_rs::log::SessionLogger;
use tio_rs::oneshot;
use tio_rs::serial;
use tio_rs::terminal;

fn main() {
    let args = Cli::parse();

    if args.list {
        cmd_list(&args);
        return;
    }

    if args.json && args.send.is_none() {
        cmd_json(&args);
        return;
    }

    if args.send.is_some() || args.expect.is_some() {
        cmd_send_expect(&args);
        return;
    }

    if let Some(tio_rs::cli::Commands::Mcp) = &args.command {
        cmd_mcp(&args);
        return;
    }

    cmd_interactive(&args);
}

fn cmd_list(args: &Cli) {
    let devices = tio_rs::list::enumerate_devices();
    if args.json {
        print!("{}", tio_rs::list::render_json(&devices));
    } else {
        print!("{}", tio_rs::list::render_table(&devices));
        // Load and display configured profiles
        let (_, profiles) = tio_rs::config::load_config();
        print!("{}", tio_rs::list::render_profiles(&profiles));
    }
}

fn cmd_json(_args: &Cli) {
    eprintln!("not implemented");
    std::process::exit(1);
}

fn cmd_send_expect(args: &Cli) {
    use tio_rs::cli::{FlowControl, Parity};

    let serial_cfg = serial::SerialConfig {
        device: args.device.clone().unwrap_or_default(),
        baudrate: args.baudrate,
        databits: args.databits,
        stopbits: args.stopbits,
        parity: match args.parity {
            Parity::None => serialport::Parity::None,
            Parity::Odd => serialport::Parity::Odd,
            Parity::Even => serialport::Parity::Even,
        },
        flow: match args.flow {
            FlowControl::None => serialport::FlowControl::None,
            FlowControl::Hard => serialport::FlowControl::Hardware,
            FlowControl::Soft => serialport::FlowControl::Software,
        },
        reconnect: false,
    };

    // Parse the send string (or use empty if only --expect was given)
    let send_str = args.send.as_deref().unwrap_or("");
    let send_bytes = oneshot::parse_escapes(send_str);

    let expect = args.expect.as_deref();
    let timeout = args.timeout;
    let json = args.json;

    match oneshot::run(&serial_cfg, &send_bytes, send_str, expect, timeout, json) {
        Ok(_) => std::process::exit(0),
        Err(code) => std::process::exit(code),
    }
}

fn cmd_mcp(_args: &Cli) {
    if let Err(e) = tio_rs::mcp::run_mcp_server() {
        eprintln!("MCP server error: {}", e);
        std::process::exit(1);
    }
}

fn cmd_interactive(args: &Cli) {
    use tio_rs::cli::{FlowControl, OutputMode as CliOutputMode, Parity};

    // Build serial config from CLI args
    let serial_cfg = serial::SerialConfig {
        device: args.device.clone().unwrap_or_default(),
        baudrate: args.baudrate,
        databits: args.databits,
        stopbits: args.stopbits,
        parity: match args.parity {
            Parity::None => serialport::Parity::None,
            Parity::Odd => serialport::Parity::Odd,
            Parity::Even => serialport::Parity::Even,
        },
        flow: match args.flow {
            FlowControl::None => serialport::FlowControl::None,
            FlowControl::Hard => serialport::FlowControl::Hardware,
            FlowControl::Soft => serialport::FlowControl::Software,
        },
        reconnect: !args.no_reconnect,
    };

    // Build output formatter
    let mut formatter = OutputFormatter::new();
    formatter.timestamps = args.timestamp;
    formatter.timestamp_format = args.timestamp_format.clone();
    formatter.local_echo = args.local_echo;
    if let Some(map_str) = args.map.as_deref() {
        formatter.char_maps = format::parse_char_map(map_str);
    }
    formatter.mode = match args.output_mode {
        CliOutputMode::Normal => OutputMode::Normal,
        CliOutputMode::Hex => OutputMode::Hex,
    };

    // Build logger
    let log_file = args.log_file.as_deref().map(std::path::Path::new);
    let mut logger = match SessionLogger::new(log_file, args.log_append, args.log_strip) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("Failed to create log file: {}", e);
            std::process::exit(1);
        }
    };

    // Auto-name log if -L is set and no explicit log file
    if args.log && args.log_file.is_none() {
        if let Some(device) = args.device.as_deref() {
            let _ = logger.auto_name(device, args.log_append, args.log_strip);
        }
    }

    // Run interactive session
    if let Err(e) = terminal::run_interactive(
        &serial_cfg,
        formatter,
        logger,
        args.timestamp,
        args.timestamp_format.clone(),
        args.auto_connect
            .as_ref()
            .unwrap_or(&tio_rs::cli::AutoConnect::Direct),
    ) {
        eprintln!("Session error: {}", e);
        std::process::exit(1);
    }
}
