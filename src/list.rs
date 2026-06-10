use std::collections::HashMap;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::time::SystemTime;

use serde::Serialize;
use sha2::{Digest, Sha256};

/// Information about a single serial device.
#[derive(Debug, Serialize)]
pub struct DeviceInfo {
    pub path: String,
    pub tid: String,
    pub uptime_s: u64,
    pub driver: String,
    pub description: String,
    pub by_id: Option<String>,
    pub by_path: Option<String>,
}

/// Enumerate available serial devices.
pub fn enumerate_devices() -> Vec<DeviceInfo> {
    let mut devices = Vec::new();

    // Collect by-id and by-path symlinks for lookup
    let by_id_map = collect_serial_links("/dev/serial/by-id");
    let by_path_map = collect_serial_links("/dev/serial/by-path");

    // Get ports from serialport crate
    let ports = serialport::available_ports().unwrap_or_default();

    for port in &ports {
        let path = &port.port_name;
        let driver = resolve_driver(path);
        let uptime_s = compute_uptime_s(path);
        let description = read_usb_description(path);
        let by_id = by_id_map.get(path).cloned();
        let by_path = by_path_map.get(path).cloned();
        let tid = compute_tid(&by_path, path);

        devices.push(DeviceInfo {
            path: path.clone(),
            tid,
            uptime_s,
            driver,
            description,
            by_id,
            by_path,
        });
    }

    // Also scan /dev/tty{USB,ACM,S}* for devices that serialport might not report
    for prefix in &["ttyUSB", "ttyACM", "ttyS"] {
        if let Ok(entries) = fs::read_dir("/dev") {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with(prefix) {
                    let path = format!("/dev/{}", name);
                    // Skip if already listed
                    if devices.iter().any(|d| d.path == path) {
                        continue;
                    }
                    let driver = resolve_driver(&path);
                    let uptime_s = compute_uptime_s(&path);
                    let description = read_usb_description(&path);
                    let by_id = by_id_map.get(&path).cloned();
                    let by_path = by_path_map.get(&path).cloned();
                    let tid = compute_tid(&by_path, &path);

                    devices.push(DeviceInfo {
                        path,
                        tid,
                        uptime_s,
                        driver,
                        description,
                        by_id,
                        by_path,
                    });
                }
            }
        }
    }

    // Sort by uptime (oldest first, matching tio behavior)
    devices.sort_by_key(|d| d.uptime_s);
    devices
}

/// Collect symlinks from a directory, mapping target -> link name.
fn collect_serial_links(dir: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let link_path = entry.path();
            if let Ok(target) = fs::read_link(&link_path) {
                let target_str = normalize_dev_path(&target);
                let link_name = link_path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                map.insert(target_str, link_name);
            }
        }
    }
    map
}

/// Normalize a /dev/... path to /dev/ttyXXX form.
fn normalize_dev_path(path: &Path) -> String {
    let p = path.to_string_lossy().to_string();
    // Resolve ../.. style relative symlinks
    if p.starts_with("/dev/") {
        p
    } else if let Ok(canonical) = path.canonicalize() {
        canonical.to_string_lossy().to_string()
    } else {
        // Try prepending /dev/
        let with_dev = format!("/dev/{}", p);
        if Path::new(&with_dev).exists() {
            with_dev
        } else {
            p
        }
    }
}

/// Resolve the driver name for a tty device via sysfs.
fn resolve_driver(device: &str) -> String {
    let dev_name = Path::new(device)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let driver_link = format!("/sys/class/tty/{}/device/driver", dev_name);
    fs::read_link(&driver_link)
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
        .unwrap_or_else(|| "unknown".to_string())
}

/// Compute uptime in seconds from device node mtime.
fn compute_uptime_s(device: &str) -> u64 {
    fs::metadata(device)
        .ok()
        .and_then(|m| {
            let mtime = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(m.mtime() as u64);
            SystemTime::now().duration_since(mtime).ok()
        })
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Read USB product/manufacturer description from sysfs.
fn read_usb_description(device: &str) -> String {
    let dev_name = Path::new(device)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();

    // Walk up sysfs to find USB device with product/manufacturer
    let base = format!("/sys/class/tty/{}/device", dev_name);
    let mut dir = Path::new(&base).to_path_buf();

    for _ in 0..5 {
        let product = dir.join("product");
        if product.exists() {
            if let Ok(p) = fs::read_to_string(&product) {
                return p.trim().to_string();
            }
        }
        // Also check manufacturer
        let manufacturer = dir.join("manufacturer");
        if manufacturer.exists() {
            if let Ok(m) = fs::read_to_string(&manufacturer) {
                let product = dir.join("product");
                let prod = if product.exists() {
                    fs::read_to_string(&product)
                        .unwrap_or_default()
                        .trim()
                        .to_string()
                } else {
                    String::new()
                };
                if prod.is_empty() {
                    return m.trim().to_string();
                } else {
                    return format!("{} {}", m.trim(), prod);
                }
            }
        }
        // Go up one level
        if let Some(parent) = dir.parent() {
            dir = parent.to_path_buf();
        } else {
            break;
        }
    }

    String::new()
}

/// Compute short TID: first 4 hex chars of sha256 of the by-path link target.
fn compute_tid(by_path: &Option<String>, device: &str) -> String {
    let input = match by_path {
        Some(p) => p.clone(),
        None => device.to_string(),
    };
    let hash = Sha256::digest(input.as_bytes());
    format!("{:02x}{:02x}", hash[0], hash[1])
}

/// Render devices as an aligned text table.
pub fn render_table(devices: &[DeviceInfo]) -> String {
    if devices.is_empty() {
        return "No serial devices found.\n".to_string();
    }

    let mut output = String::new();
    output.push_str(&format!(
        "{:<20} {:<6} {:<10} {:<16} {:<30} {:<20} {}\n",
        "Device", "TID", "Uptime", "Driver", "Description", "By-ID", "By-Path"
    ));
    output.push_str(&"-".repeat(120));
    output.push('\n');

    for d in devices {
        let uptime_str = format_uptime(d.uptime_s);
        let by_id = d.by_id.as_deref().unwrap_or("-");
        let by_path = d.by_path.as_deref().unwrap_or("-");
        let desc = if d.description.is_empty() {
            "-"
        } else {
            &d.description
        };
        output.push_str(&format!(
            "{:<20} {:<6} {:<10} {:<16} {:<30} {:<20} {}\n",
            d.path, d.tid, uptime_str, d.driver, desc, by_id, by_path
        ));
    }
    output
}

fn format_uptime(secs: u64) -> String {
    if secs < 60 {
        format!("{}s", secs)
    } else if secs < 3600 {
        format!("{}m", secs / 60)
    } else if secs < 86400 {
        format!("{}h", secs / 3600)
    } else {
        format!("{}d", secs / 86400)
    }
}

/// Render devices as JSON.
pub fn render_json(devices: &[DeviceInfo]) -> String {
    serde_json::to_string_pretty(devices).unwrap_or_else(|_| "[]".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_uptime() {
        assert_eq!(format_uptime(30), "30s");
        assert_eq!(format_uptime(90), "1m");
        assert_eq!(format_uptime(7200), "2h");
        assert_eq!(format_uptime(172800), "2d");
    }

    #[test]
    fn test_compute_tid_deterministic() {
        let tid1 = compute_tid(
            &Some("/dev/serial/by-path/platform-xhci-hcd.0-usb-0:1.2:1.0-port0".to_string()),
            "/dev/ttyUSB0",
        );
        let tid2 = compute_tid(
            &Some("/dev/serial/by-path/platform-xhci-hcd.0-usb-0:1.2:1.0-port0".to_string()),
            "/dev/ttyUSB0",
        );
        assert_eq!(tid1, tid2);
        assert_eq!(tid1.len(), 4);
    }

    #[test]
    fn test_compute_tid_fallback() {
        let tid = compute_tid(&None, "/dev/ttyUSB0");
        assert_eq!(tid.len(), 4);
    }

    #[test]
    fn test_json_serialization() {
        let devices = vec![DeviceInfo {
            path: "/dev/ttyUSB0".to_string(),
            tid: "a1b2".to_string(),
            uptime_s: 120,
            driver: "pl2303".to_string(),
            description: "USB Serial".to_string(),
            by_id: Some("usb-FTDI_FT232R_A12345-if00-port0".to_string()),
            by_path: Some("platform-xhci-hcd.0-usb-0:1.2:1.0-port0".to_string()),
        }];
        let json = render_json(&devices);
        assert!(json.contains("\"path\": \"/dev/ttyUSB0\""));
        assert!(json.contains("\"tid\": \"a1b2\""));
        assert!(json.contains("\"uptime_s\": 120"));
        assert!(json.contains("\"driver\": \"pl2303\""));
        assert!(json.contains("\"description\": \"USB Serial\""));
        assert!(json.contains("\"by_id\""));
        assert!(json.contains("\"by_path\""));
    }

    #[test]
    fn test_empty_table() {
        let devices: Vec<DeviceInfo> = vec![];
        let table = render_table(&devices);
        assert!(table.contains("No serial devices found"));
    }

    #[test]
    fn test_empty_json() {
        let devices: Vec<DeviceInfo> = vec![];
        let json = render_json(&devices);
        assert_eq!(json, "[]");
    }
}
