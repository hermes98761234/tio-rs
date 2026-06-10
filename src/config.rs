use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use regex::Regex;
use serde::Deserialize;

use crate::cli::{FlowControl, Parity};

/// A single profile from the TOML config.
#[derive(Debug, Default, Deserialize, Clone)]
pub struct Profile {
    pub device: Option<String>,
    pub baudrate: Option<u32>,
    pub databits: Option<u8>,
    pub stopbits: Option<u8>,
    pub parity: Option<String>,
    pub flow: Option<String>,
    #[serde(rename = "local-echo")]
    pub local_echo: Option<bool>,
    pub timestamp: Option<bool>,
    pub log: Option<bool>,
    #[serde(rename = "log-file")]
    pub log_file: Option<String>,
    pub pattern: Option<String>,
}

/// Top-level TOML config structure.
#[derive(Debug, Default, Deserialize)]
struct RawConfig {
    default: Option<Profile>,
    #[serde(rename = "profile")]
    profiles: Option<HashMap<String, Profile>>,
}

/// Resolved config after merging: CLI flags > matched profile > [default] > built-in defaults.
#[derive(Debug, Clone)]
pub struct Config {
    pub device: String,
    pub baudrate: u32,
    pub databits: u8,
    pub stopbits: u8,
    pub parity: Parity,
    pub flow: FlowControl,
    pub local_echo: bool,
    pub timestamp: bool,
    pub log: bool,
    pub log_file: Option<String>,
    /// All loaded profiles (name -> profile), for --list display.
    pub profiles: HashMap<String, Profile>,
    /// The name of the matched profile, if any.
    pub matched_profile: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            device: String::new(),
            baudrate: 115_200,
            databits: 8,
            stopbits: 1,
            parity: Parity::None,
            flow: FlowControl::None,
            local_echo: false,
            timestamp: false,
            log: false,
            log_file: None,
            profiles: HashMap::new(),
            matched_profile: None,
        }
    }
}

/// Load TOML config from `$XDG_CONFIG_HOME/tio/config.toml` or `~/.config/tio/config.toml`.
pub fn load_config() -> (Option<Profile>, HashMap<String, Profile>) {
    let path = config_path();
    let Some(path) = path else {
        return (None, HashMap::new());
    };
    if !path.exists() {
        return (None, HashMap::new());
    }
    let Ok(contents) = fs::read_to_string(&path) else {
        return (None, HashMap::new());
    };
    let raw: RawConfig = match toml::from_str(&contents) {
        Ok(r) => r,
        Err(_) => return (None, HashMap::new()),
    };
    (raw.default, raw.profiles.unwrap_or_default())
}

/// Resolve the config: CLI flags take precedence, then matched profile, then [default], then built-ins.
pub fn resolve_config(
    cli: &crate::cli::Cli,
    default_profile: Option<Profile>,
    profiles: HashMap<String, Profile>,
) -> Config {
    let mut cfg = Config {
        profiles: profiles.clone(),
        ..Default::default()
    };

    // Start with built-in defaults (already in Config::default())

    // Layer 1: [default] section
    if let Some(ref dp) = default_profile {
        apply_profile(&mut cfg, dp);
    }

    // Layer 2: matching profile (by name or by pattern)
    if let Some(ref device) = cli.device {
        let matched = find_matching_profile(device, &profiles);
        if let Some((name, profile)) = matched {
            apply_profile(&mut cfg, &profile);
            cfg.matched_profile = Some(name);
        }
    }

    // Layer 3: explicit CLI flags (only override if they differ from built-in defaults)
    if let Some(ref device) = cli.device {
        cfg.device = device.clone();
    }
    if cli.baudrate != 115_200 {
        cfg.baudrate = cli.baudrate;
    }
    if cli.databits != 8 {
        cfg.databits = cli.databits;
    }
    if cli.stopbits != 1 {
        cfg.stopbits = cli.stopbits;
    }
    if !matches!(cli.parity, Parity::None) {
        cfg.parity = cli.parity.clone();
    }
    if !matches!(cli.flow, FlowControl::None) {
        cfg.flow = cli.flow.clone();
    }
    if cli.local_echo {
        cfg.local_echo = true;
    }
    if cli.timestamp {
        cfg.timestamp = true;
    }
    if cli.log {
        cfg.log = true;
    }
    if cli.log_file.is_some() {
        cfg.log_file = cli.log_file.clone();
    }

    cfg
}

/// Find a profile matching the device argument.
/// First checks for exact profile name match, then pattern regex match.
fn find_matching_profile(
    device: &str,
    profiles: &HashMap<String, Profile>,
) -> Option<(String, Profile)> {
    // Exact name match first
    for (name, profile) in profiles {
        if name == device {
            return Some((name.clone(), profile.clone()));
        }
    }
    // Pattern match
    for (name, profile) in profiles {
        if let Some(ref pattern) = profile.pattern {
            if let Ok(re) = Regex::new(pattern) {
                if re.is_match(device) {
                    return Some((name.clone(), profile.clone()));
                }
            }
        }
    }
    None
}

/// Apply profile fields onto a Config (only sets fields that are Some).
fn apply_profile(cfg: &mut Config, profile: &Profile) {
    if let Some(ref v) = profile.device {
        cfg.device = v.clone();
    }
    if let Some(v) = profile.baudrate {
        cfg.baudrate = v;
    }
    if let Some(v) = profile.databits {
        cfg.databits = v;
    }
    if let Some(v) = profile.stopbits {
        cfg.stopbits = v;
    }
    if let Some(ref v) = profile.parity {
        cfg.parity = match v.as_str() {
            "odd" => Parity::Odd,
            "even" => Parity::Even,
            _ => Parity::None,
        };
    }
    if let Some(ref v) = profile.flow {
        cfg.flow = match v.as_str() {
            "hard" => FlowControl::Hard,
            "soft" => FlowControl::Soft,
            _ => FlowControl::None,
        };
    }
    if let Some(v) = profile.local_echo {
        cfg.local_echo = v;
    }
    if let Some(v) = profile.timestamp {
        cfg.timestamp = v;
    }
    if let Some(v) = profile.log {
        cfg.log = v;
    }
    if let Some(ref v) = profile.log_file {
        cfg.log_file = Some(v.clone());
    }
}

fn config_path() -> Option<PathBuf> {
    // $XDG_CONFIG_HOME/tio/config.toml
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        let p = PathBuf::from(&xdg).join("tio/config.toml");
        if p.exists() {
            return Some(p);
        }
    }
    // ~/.config/tio/config.toml
    if let Some(dir) = dirs::config_dir() {
        let p = dir.join("tio/config.toml");
        if p.exists() {
            return Some(p);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::{FlowControl, Parity};
    use std::io::Write;

    fn write_temp_config(content: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir.path().join("tio");
        fs::create_dir_all(&config_dir).unwrap();
        let config_path = config_dir.join("config.toml");
        let mut f = fs::File::create(&config_path).unwrap();
        write!(f, "{}", content).unwrap();
        (dir, config_path)
    }

    fn make_cli(device: &str) -> crate::cli::Cli {
        crate::cli::Cli {
            device: Some(device.to_string()),
            baudrate: 115_200,
            databits: 8,
            stopbits: 1,
            parity: Parity::None,
            flow: FlowControl::None,
            auto_connect: None,
            no_reconnect: false,
            local_echo: false,
            timestamp: false,
            timestamp_format: None,
            input_mode: crate::cli::InputMode::Normal,
            output_mode: crate::cli::OutputMode::Normal,
            map: None,
            log: false,
            log_file: None,
            log_append: false,
            log_strip: false,
            list: false,
            json: false,
            send: None,
            expect: None,
            timeout: 10,
            mute: false,
            color: crate::cli::ColorMode::Auto,
            command: None,
        }
    }

    #[test]
    fn test_default_values_without_config() {
        let (_dir, _path) = write_temp_config("");
        let prev = std::env::var("XDG_CONFIG_HOME").ok();
        std::env::set_var("XDG_CONFIG_HOME", _dir.path());

        let (default, profiles) = load_config();
        let cli = make_cli("/dev/ttyUSB0");
        let cfg = resolve_config(&cli, default, profiles);

        assert_eq!(cfg.baudrate, 115_200);
        assert_eq!(cfg.databits, 8);
        assert_eq!(cfg.stopbits, 1);
        assert_eq!(cfg.device, "/dev/ttyUSB0");
        assert!(cfg.matched_profile.is_none());

        if let Some(p) = prev {
            std::env::set_var("XDG_CONFIG_HOME", p);
        } else {
            std::env::remove_var("XDG_CONFIG_HOME");
        }
    }

    #[test]
    fn test_default_section_applied() {
        let (_dir, _path) = write_temp_config(
            r#"
[default]
baudrate = 9600
databits = 7
local-echo = true
"#,
        );
        let prev = std::env::var("XDG_CONFIG_HOME").ok();
        std::env::set_var("XDG_CONFIG_HOME", _dir.path());

        let (default, profiles) = load_config();
        let cli = make_cli("/dev/ttyUSB0");
        let cfg = resolve_config(&cli, default, profiles);

        assert_eq!(cfg.baudrate, 9600);
        assert_eq!(cfg.databits, 7);
        assert!(cfg.local_echo);

        if let Some(p) = prev {
            std::env::set_var("XDG_CONFIG_HOME", p);
        } else {
            std::env::remove_var("XDG_CONFIG_HOME");
        }
    }

    #[test]
    fn test_profile_name_match() {
        let (_dir, _path) = write_temp_config(
            r#"
[default]
baudrate = 9600

[profile.myusb]
device = "/dev/ttyUSB0"
baudrate = 57600
local-echo = true
"#,
        );
        let prev = std::env::var("XDG_CONFIG_HOME").ok();
        std::env::set_var("XDG_CONFIG_HOME", _dir.path());

        let (default, profiles) = load_config();
        let cli = make_cli("myusb");
        let cfg = resolve_config(&cli, default, profiles);

        assert_eq!(cfg.baudrate, 57600);
        assert!(cfg.local_echo);
        assert_eq!(cfg.matched_profile, Some("myusb".to_string()));

        if let Some(p) = prev {
            std::env::set_var("XDG_CONFIG_HOME", p);
        } else {
            std::env::remove_var("XDG_CONFIG_HOME");
        }
    }

    #[test]
    fn test_profile_pattern_match() {
        let (_dir, _path) = write_temp_config(
            r#"
[default]
baudrate = 9600

[profile.usb]
pattern = "/dev/ttyUSB.*"
baudrate = 57600
local-echo = true
"#,
        );
        let prev = std::env::var("XDG_CONFIG_HOME").ok();
        std::env::set_var("XDG_CONFIG_HOME", _dir.path());

        let (default, profiles) = load_config();
        let cli = make_cli("/dev/ttyUSB1");
        let cfg = resolve_config(&cli, default, profiles);

        assert_eq!(cfg.baudrate, 57600);
        assert!(cfg.local_echo);
        assert_eq!(cfg.matched_profile, Some("usb".to_string()));

        if let Some(p) = prev {
            std::env::set_var("XDG_CONFIG_HOME", p);
        } else {
            std::env::remove_var("XDG_CONFIG_HOME");
        }
    }

    #[test]
    fn test_cli_overrides_profile() {
        let (_dir, _path) = write_temp_config(
            r#"
[profile.usb]
pattern = "/dev/ttyUSB.*"
baudrate = 57600
local-echo = true
"#,
        );
        let prev = std::env::var("XDG_CONFIG_HOME").ok();
        std::env::set_var("XDG_CONFIG_HOME", _dir.path());

        let (default, profiles) = load_config();
        let mut cli = make_cli("/dev/ttyUSB0");
        cli.baudrate = 38400;
        let cfg = resolve_config(&cli, default, profiles);

        // CLI baudrate should win over profile
        assert_eq!(cfg.baudrate, 38400);
        // Profile local-echo should still apply
        assert!(cfg.local_echo);

        if let Some(p) = prev {
            std::env::set_var("XDG_CONFIG_HOME", p);
        } else {
            std::env::remove_var("XDG_CONFIG_HOME");
        }
    }

    #[test]
    fn test_profile_overrides_default_section() {
        let (_dir, _path) = write_temp_config(
            r#"
[default]
baudrate = 9600
local-echo = false

[profile.fast]
pattern = "/dev/ttyACM.*"
baudrate = 115200
local-echo = true
"#,
        );
        let prev = std::env::var("XDG_CONFIG_HOME").ok();
        std::env::set_var("XDG_CONFIG_HOME", _dir.path());

        let (default, profiles) = load_config();
        let cli = make_cli("/dev/ttyACM0");
        let cfg = resolve_config(&cli, default, profiles);

        // Profile should override [default]
        assert_eq!(cfg.baudrate, 115_200);
        assert!(cfg.local_echo);

        if let Some(p) = prev {
            std::env::set_var("XDG_CONFIG_HOME", p);
        } else {
            std::env::remove_var("XDG_CONFIG_HOME");
        }
    }

    #[test]
    fn test_no_matching_profile() {
        let (_dir, _path) = write_temp_config(
            r#"
[profile.other]
pattern = "/dev/ttyS.*"
baudrate = 57600
"#,
        );
        let prev = std::env::var("XDG_CONFIG_HOME").ok();
        std::env::set_var("XDG_CONFIG_HOME", _dir.path());

        let (default, profiles) = load_config();
        let cli = make_cli("/dev/ttyUSB0");
        let cfg = resolve_config(&cli, default, profiles);

        assert!(cfg.matched_profile.is_none());
        assert_eq!(cfg.baudrate, 115_200); // built-in default

        if let Some(p) = prev {
            std::env::set_var("XDG_CONFIG_HOME", p);
        } else {
            std::env::remove_var("XDG_CONFIG_HOME");
        }
    }

    #[test]
    fn test_name_match_preferred_over_pattern() {
        let (_dir, _path) = write_temp_config(
            r#"
[profile.usb]
pattern = "/dev/ttyUSB.*"
baudrate = 57600

[profile.direct]
device = "/dev/ttyUSB0"
baudrate = 38400
"#,
        );
        let prev = std::env::var("XDG_CONFIG_HOME").ok();
        std::env::set_var("XDG_CONFIG_HOME", _dir.path());

        let (default, profiles) = load_config();
        let cli = make_cli("direct");
        let cfg = resolve_config(&cli, default, profiles);

        // Exact name match should win
        assert_eq!(cfg.matched_profile, Some("direct".to_string()));
        assert_eq!(cfg.baudrate, 38400);

        if let Some(p) = prev {
            std::env::set_var("XDG_CONFIG_HOME", p);
        } else {
            std::env::remove_var("XDG_CONFIG_HOME");
        }
    }

    #[test]
    fn test_parity_and_flow_from_profile() {
        let (_dir, _path) = write_temp_config(
            r#"
[profile.serial]
pattern = "/dev/ttyS.*"
parity = "odd"
flow = "hard"
"#,
        );
        let prev = std::env::var("XDG_CONFIG_HOME").ok();
        std::env::set_var("XDG_CONFIG_HOME", _dir.path());

        let (default, profiles) = load_config();
        let cli = make_cli("/dev/ttyS0");
        let cfg = resolve_config(&cli, default, profiles);

        assert!(matches!(cfg.parity, Parity::Odd));
        assert!(matches!(cfg.flow, FlowControl::Hard));

        if let Some(p) = prev {
            std::env::set_var("XDG_CONFIG_HOME", p);
        } else {
            std::env::remove_var("XDG_CONFIG_HOME");
        }
    }

    #[test]
    fn test_config_file_not_found() {
        let prev = std::env::var("XDG_CONFIG_HOME").ok();
        std::env::remove_var("XDG_CONFIG_HOME");
        // Also clear HOME to prevent dirs::config_dir() from finding something
        let prev_home = std::env::var("HOME").ok();
        std::env::set_var("HOME", "/nonexistent_home");

        let (default, profiles) = load_config();
        assert!(default.is_none());
        assert!(profiles.is_empty());

        if let Some(p) = prev {
            std::env::set_var("XDG_CONFIG_HOME", p);
        } else {
            std::env::remove_var("XDG_CONFIG_HOME");
        }
        if let Some(p) = prev_home {
            std::env::set_var("HOME", p);
        } else {
            std::env::remove_var("HOME");
        }
    }
}
