/// Session logger: auto-naming, file path override, append mode, control-char stripping.
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

/// Session logger configuration and state.
pub struct SessionLogger {
    file: Option<File>,
    path: Option<PathBuf>,
    strip: bool,
}

impl SessionLogger {
    /// Create a new logger. If `log_file` is None, no logging until `auto_name` is called.
    pub fn new(log_file: Option<&Path>, append: bool, strip: bool) -> std::io::Result<Self> {
        let (file, path) = if let Some(p) = log_file {
            let f = if append {
                OpenOptions::new().create(true).append(true).open(p)?
            } else {
                OpenOptions::new()
                    .create(true)
                    .write(true)
                    .truncate(true)
                    .open(p)?
            };
            (Some(f), Some(p.to_path_buf()))
        } else {
            (None, None)
        };
        Ok(Self { file, path, strip })
    }

    /// Auto-name a log file: tio_<device-basename>_<YYYYmmdd_HHMMSS>.log in cwd.
    pub fn auto_name(&mut self, device: &str, append: bool, strip: bool) -> std::io::Result<()> {
        let basename = Path::new(device)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown");

        let now = std::time::SystemTime::now();
        let duration = now
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        let secs = duration.as_secs();
        let tm = format_timestamp(secs);

        let filename = format!("tio_{}_{}.log", basename, tm);
        let path = PathBuf::from(&filename);

        let file = if append {
            OpenOptions::new().create(true).append(true).open(&path)?
        } else {
            OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(&path)?
        };

        self.file = Some(file);
        self.path = Some(path);
        self.strip = strip;
        Ok(())
    }

    /// Write a chunk of data to the log file.
    pub fn write(&mut self, data: &[u8]) -> std::io::Result<()> {
        if let Some(ref mut file) = self.file {
            if self.strip {
                let filtered: Vec<u8> = data
                    .iter()
                    .copied()
                    .filter(|&b| b == b'\n' || (0x20..0x7f).contains(&b))
                    .collect();
                file.write_all(&filtered)?;
            } else {
                file.write_all(data)?;
            }
        }
        Ok(())
    }

    /// Flush the log file.
    pub fn flush(&mut self) -> std::io::Result<()> {
        if let Some(ref mut file) = self.file {
            file.flush()?;
        }
        Ok(())
    }

    /// Return the current log file path if any.
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Return whether logging is active.
    pub fn is_active(&self) -> bool {
        self.file.is_some()
    }
}

/// Format a Unix timestamp as YYYYmmdd_HHMMSS.
fn format_timestamp(secs: u64) -> String {
    let days = secs / 86400;
    let rem = secs % 86400;
    let h = rem / 3600;
    let m = (rem % 3600) / 60;
    let s = rem % 60;

    // Approximate date from days since epoch (good enough for filenames)
    let (year, month, day) = days_to_date(days);
    format!("{:04}{:02}{:02}_{:02}{:02}{:02}", year, month, day, h, m, s)
}

/// Convert days since Unix epoch to (year, month, day).
fn days_to_date(mut days: u64) -> (u64, u64, u64) {
    let mut year = 1970u64;
    loop {
        let year_days = if is_leap(year) { 366 } else { 365 };
        if days < year_days {
            break;
        }
        days -= year_days;
        year += 1;
    }

    let month_lengths = if is_leap(year) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };

    let mut month = 1u64;
    for &ml in &month_lengths {
        if days < ml {
            break;
        }
        days -= ml;
        month += 1;
    }

    (year, month, days + 1)
}

fn is_leap(year: u64) -> bool {
    year.is_multiple_of(4) && !year.is_multiple_of(100) || year.is_multiple_of(400)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Read;

    #[test]
    fn test_new_no_file() {
        let logger = SessionLogger::new(None, false, false).unwrap();
        assert!(!logger.is_active());
        assert!(logger.path().is_none());
    }

    #[test]
    fn test_new_with_file() {
        let path = Path::new("/tmp/tio_test_log.log");
        let logger = SessionLogger::new(Some(path), false, false).unwrap();
        assert!(logger.is_active());
        assert_eq!(logger.path(), Some(path));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn test_write() {
        let path = Path::new("/tmp/tio_test_write.log");
        {
            let mut logger = SessionLogger::new(Some(path), false, false).unwrap();
            logger.write(b"hello world\n").unwrap();
        }
        let mut content = String::new();
        fs::File::open(path)
            .unwrap()
            .read_to_string(&mut content)
            .unwrap();
        assert_eq!(content, "hello world\n");
        let _ = fs::remove_file(path);
    }

    #[test]
    fn test_write_strip() {
        let path = Path::new("/tmp/tio_test_strip.log");
        {
            let mut logger = SessionLogger::new(Some(path), false, true).unwrap();
            // Mix of printable, newline, and control chars
            logger.write(b"abc\x01\x02\n\x03def\n").unwrap();
        }
        let mut content = String::new();
        fs::File::open(path)
            .unwrap()
            .read_to_string(&mut content)
            .unwrap();
        assert_eq!(content, "abc\ndef\n");
        let _ = fs::remove_file(path);
    }

    #[test]
    fn test_append_mode() {
        let path = Path::new("/tmp/tio_test_append.log");
        {
            let mut logger = SessionLogger::new(Some(path), false, false).unwrap();
            logger.write(b"first\n").unwrap();
        }
        {
            let mut logger = SessionLogger::new(Some(path), true, false).unwrap();
            logger.write(b"second\n").unwrap();
        }
        let mut content = String::new();
        fs::File::open(path)
            .unwrap()
            .read_to_string(&mut content)
            .unwrap();
        assert_eq!(content, "first\nsecond\n");
        let _ = fs::remove_file(path);
    }

    #[test]
    fn test_truncate_mode() {
        let path = Path::new("/tmp/tio_test_truncate.log");
        {
            let mut logger = SessionLogger::new(Some(path), false, false).unwrap();
            logger.write(b"first\n").unwrap();
        }
        {
            let mut logger = SessionLogger::new(Some(path), false, false).unwrap();
            logger.write(b"second\n").unwrap();
        }
        let mut content = String::new();
        fs::File::open(path)
            .unwrap()
            .read_to_string(&mut content)
            .unwrap();
        assert_eq!(content, "second\n");
        let _ = fs::remove_file(path);
    }

    #[test]
    fn test_auto_name_format() {
        let _dir = "/tmp";
        let device = "/dev/ttyUSB0";
        let basename = Path::new(device).file_name().unwrap().to_str().unwrap();
        assert_eq!(basename, "ttyUSB0");

        // Just verify auto_name creates a file with expected prefix
        let mut logger = SessionLogger::new(None, false, false).unwrap();
        logger.auto_name(device, false, false).unwrap();
        let path = logger.path().unwrap();
        let filename = path.file_name().unwrap().to_str().unwrap();
        assert!(filename.starts_with("tio_ttyUSB0_"));
        assert!(filename.ends_with(".log"));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn test_auto_name_unknown_device() {
        let mut logger = SessionLogger::new(None, false, false).unwrap();
        logger.auto_name("", false, false).unwrap();
        let path = logger.path().unwrap();
        let filename = path.file_name().unwrap().to_str().unwrap();
        assert!(filename.starts_with("tio_unknown_"));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn test_format_timestamp() {
        // 2024-01-01 00:00:00 UTC = 1704067200
        let ts = format_timestamp(1704067200);
        assert_eq!(ts, "20240101_000000");
    }

    #[test]
    fn test_days_to_date_epoch() {
        let (y, m, d) = days_to_date(0);
        assert_eq!((y, m, d), (1970, 1, 1));
    }

    #[test]
    fn test_days_to_date_2024_01_01() {
        // Days from 1970-01-01 to 2024-01-01
        let _days = (2024 - 1970) * 365 + ((2024 - 1969) / 4); // approximate
                                                              // More precise: count leap years
        let mut d = 0u64;
        for y in 1970..2024 {
            d += if is_leap(y) { 366 } else { 365 };
        }
        let (y, m, dd) = days_to_date(d);
        assert_eq!((y, m, dd), (2024, 1, 1));
    }

    #[test]
    fn test_flush() {
        let path = Path::new("/tmp/tio_test_flush.log");
        {
            let mut logger = SessionLogger::new(Some(path), false, false).unwrap();
            logger.write(b"data").unwrap();
            logger.flush().unwrap();
        }
        let mut content = String::new();
        fs::File::open(path)
            .unwrap()
            .read_to_string(&mut content)
            .unwrap();
        assert_eq!(content, "data");
        let _ = fs::remove_file(path);
    }
}
