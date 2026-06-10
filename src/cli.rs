use clap::{Parser, Subcommand, ValueEnum};

#[derive(Debug, Clone, ValueEnum)]
pub enum Parity {
    None,
    Odd,
    Even,
}

#[derive(Debug, Clone, ValueEnum)]
pub enum FlowControl {
    None,
    Hard,
    Soft,
}

#[derive(Debug, Clone, ValueEnum)]
pub enum AutoConnect {
    New,
    Latest,
    Direct,
}

#[derive(Debug, Clone, ValueEnum)]
pub enum InputMode {
    Normal,
    Hex,
}

#[derive(Debug, Clone, ValueEnum)]
pub enum OutputMode {
    Normal,
    Hex,
}

#[derive(Debug, Clone, ValueEnum)]
pub enum ColorMode {
    Always,
    Never,
    Auto,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Run as MCP server for AI agents
    Mcp,
}

#[derive(Parser)]
#[command(name = "tio", about = "The simple serial device I/O tool", version)]
pub struct Cli {
    /// Serial device to connect to
    pub device: Option<String>,

    /// Baud rate
    #[arg(short = 'b', long = "baudrate", default_value_t = 115200)]
    pub baudrate: u32,

    /// Data bits (5-8)
    #[arg(short = 'd', long = "databits", default_value_t = 8, value_parser = clap::value_parser!(u8).range(5..=8))]
    pub databits: u8,

    /// Stop bits (1-2)
    #[arg(short = 's', long = "stopbits", default_value_t = 1, value_parser = clap::value_parser!(u8).range(1..=2))]
    pub stopbits: u8,

    /// Parity
    #[arg(short = 'p', long = "parity", value_enum, default_value_t = Parity::None)]
    pub parity: Parity,

    /// Flow control
    #[arg(short = 'f', long = "flow", value_enum, default_value_t = FlowControl::None)]
    pub flow: FlowControl,

    /// Auto-connect strategy
    #[arg(short = 'a', long = "auto-connect", value_enum)]
    pub auto_connect: Option<AutoConnect>,

    /// Disable automatic reconnection
    #[arg(short = 'n', long = "no-reconnect")]
    pub no_reconnect: bool,

    /// Enable local echo
    #[arg(short = 'e', long = "local-echo")]
    pub local_echo: bool,

    /// Enable timestamps
    #[arg(short = 't', long = "timestamp")]
    pub timestamp: bool,

    /// Timestamp format string
    #[arg(long = "timestamp-format")]
    pub timestamp_format: Option<String>,

    /// Input mode
    #[arg(long = "input-mode", value_enum, default_value_t = InputMode::Normal)]
    pub input_mode: InputMode,

    /// Output mode
    #[arg(long = "output-mode", value_enum, default_value_t = OutputMode::Normal)]
    pub output_mode: OutputMode,

    /// Character mapping (e.g., "lf:cr,cr:lf")
    #[arg(short = 'm', long = "map")]
    pub map: Option<String>,

    /// Enable logging
    #[arg(short = 'L', long = "log")]
    pub log: bool,

    /// Log file path
    #[arg(long = "log-file")]
    pub log_file: Option<String>,

    /// Append to log file instead of overwriting
    #[arg(long = "log-append")]
    pub log_append: bool,

    /// Strip ANSI escape sequences from log
    #[arg(long = "log-strip")]
    pub log_strip: bool,

    /// List available serial ports
    #[arg(short = 'l', long = "list")]
    pub list: bool,

    /// Output port list as JSON
    #[arg(long = "json")]
    pub json: bool,

    /// Send string and expect response (one-shot mode)
    #[arg(long = "send")]
    pub send: Option<String>,

    /// Expected regex pattern (used with --send)
    #[arg(long = "expect")]
    pub expect: Option<String>,

    /// Timeout in seconds for --expect (default 10)
    #[arg(long = "timeout", default_value_t = 10)]
    pub timeout: u64,

    /// Mute interactive output
    #[arg(long = "mute")]
    pub mute: bool,

    /// Color mode
    #[arg(short = 'c', long = "color", value_enum, default_value_t = ColorMode::Auto)]
    pub color: ColorMode,

    /// Subcommands
    #[command(subcommand)]
    pub command: Option<Commands>,
}
