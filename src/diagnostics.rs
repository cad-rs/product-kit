//! What support needs when something goes wrong: a rolling log file, the
//! crash-report text, and the small pieces both are built from.
//!
//! Shared by the workbench framework and the nanobot harness; it replaced
//! their two copies. The product-specific parts stay in each framework: the
//! logger (workbench composes `env_logger`, nanobot prints to stderr), where
//! crash reports are kept (a key in the app's state store vs a file), and how
//! the plain-text export is assembled.
//!
//! Native only: on the web there is no file to write.

#[cfg(not(target_family = "wasm"))]
use std::fs::{self, File, OpenOptions};
#[cfg(not(target_family = "wasm"))]
use std::io::{Read as _, Seek as _, SeekFrom, Write as _};
#[cfg(not(target_family = "wasm"))]
use std::path::{Path, PathBuf};
#[cfg(not(target_family = "wasm"))]
use std::time::{SystemTime, UNIX_EPOCH};

/// A log file rolls over at this size…
pub const MAX_LOG_BYTES: u64 = 5 * 1024 * 1024;
/// …and this many older ones stay (`app.1.log` is the newest of them).
pub const KEPT_LOGS: u32 = 3;
/// How much of the log a report includes.
pub const REPORTED_LOG_BYTES: u64 = 1024 * 1024;
/// How much of the log a crash report includes.
pub const CRASH_LOG_BYTES: u64 = 64 * 1024;
/// A panic storm stops writing reports here; the first are the telling ones.
pub const MAX_REPORTS_PER_SESSION: u32 = 5;

/// A heading in the plain-text diagnostics export.
pub fn section(title: &str) -> String {
    format!("\n---- {title} ----")
}

/// At most the last `limit` bytes of the file, starting at a whole line.
#[cfg(not(target_family = "wasm"))]
pub fn tail(path: &Path, limit: u64) -> String {
    tail_native(path, limit)
}

/// The web has no log file; a report's log section is empty.
#[cfg(target_family = "wasm")]
pub fn tail(_path: &std::path::Path, _limit: u64) -> String {
    String::new()
}

#[cfg(not(target_family = "wasm"))]
fn tail_native(path: &Path, limit: u64) -> String {
    let Ok(mut file) = File::open(path) else {
        return String::new();
    };
    let start = file.metadata().map_or(0, |meta| meta.len()).saturating_sub(limit);
    let mut bytes = Vec::new();
    if file.seek(SeekFrom::Start(start)).is_err() || file.read_to_end(&mut bytes).is_err() {
        return String::new();
    }
    let text = String::from_utf8_lossy(&bytes);
    match text.find('\n') {
        Some(end_of_cut_line) if start > 0 => text[end_of_cut_line + 1..].to_string(),
        _ => text.into_owned(),
    }
}

/// Seconds since the unix epoch.
#[cfg(not(target_family = "wasm"))]
pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

/// `secs` since the unix epoch as UTC `(year, month, day, hour, minute,
/// second)`. Hand-rolled (the civil-from-days algorithm) so neither framework
/// needs a date crate for it.
pub fn utc(secs: u64) -> (i64, u32, u32, u32, u32, u32) {
    let days = (secs / 86_400) as i64;
    let rest = (secs % 86_400) as u32;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * shifted_month + 2) / 5 + 1) as u32;
    let month = (if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    }) as u32;
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day, rest / 3600, rest % 3600 / 60, rest % 60)
}

/// `%Y%m%d-%H%M%S`, the stamp crash report names carry.
pub fn utc_stamp(t: (i64, u32, u32, u32, u32, u32)) -> String {
    let (year, month, day, hour, minute, second) = t;
    format!("{year:04}{month:02}{day:02}-{hour:02}{minute:02}{second:02}")
}

/// `%Y-%m-%dT%H:%M:%SZ`, the report's `Time:` line.
pub fn utc_rfc3339(t: (i64, u32, u32, u32, u32, u32)) -> String {
    let (year, month, day, hour, minute, second) = t;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

/// The `Thread … panicked at …` line of a crash report.
pub fn panic_message(info: &std::panic::PanicHookInfo<'_>) -> String {
    let thread = std::thread::current();
    format!(
        "Thread {} panicked at {}: {}",
        thread.name().unwrap_or("(unnamed)"),
        info.location().map_or("(unknown)".to_string(), ToString::to_string),
        info.payload_as_str().unwrap_or("(no message)"),
    )
}

/// A crash report's text: the product line, the time, the panic, the backtrace
/// and the end of the log.
pub fn crash_report(
    product: &str,
    time: &str,
    panic: &str,
    backtrace: &str,
    recent_log: &str,
) -> String {
    format!("{product}\nTime: {time}\n{panic}\n\nBacktrace:\n{backtrace}\nRecent log:\n{recent_log}")
}

/// An append-only file that starts over when full, keeping the last few.
/// Logging must never take the app down, so failures only lose lines.
#[cfg(not(target_family = "wasm"))]
pub struct RollingFile {
    path: PathBuf,
    file: Option<File>,
    size: u64,
    max_size: u64,
}

#[cfg(not(target_family = "wasm"))]
impl RollingFile {
    pub fn open(path: PathBuf, max_size: u64) -> Self {
        if let Some(dir) = path.parent() {
            let _ = fs::create_dir_all(dir);
        }
        let mut rolling = Self {
            path,
            file: None,
            size: 0,
            max_size,
        };
        rolling.reopen();
        rolling
    }

    fn reopen(&mut self) {
        self.file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .ok();
        self.size = self
            .file
            .as_ref()
            .and_then(|f| f.metadata().ok())
            .map_or(0, |m| m.len());
    }

    pub fn write_line(&mut self, line: &str) {
        if self.size > 0 && self.size + line.len() as u64 > self.max_size {
            self.roll_over();
        }
        if let Some(file) = &mut self.file {
            if file.write_all(line.as_bytes()).is_ok() {
                self.size += line.len() as u64;
            }
        }
    }

    fn roll_over(&mut self) {
        self.file = None; // Windows can't rename a file that is open
        for older in (1..KEPT_LOGS).rev() {
            let _ = fs::rename(self.numbered(older), self.numbered(older + 1));
        }
        let _ = fs::rename(&self.path, self.numbered(1));
        self.reopen();
    }

    /// `app.log` → `app.<n>.log`.
    fn numbered(&self, n: u32) -> PathBuf {
        let stem = self.path.file_stem().unwrap_or_default().to_string_lossy();
        self.path.with_file_name(format!("{stem}.{n}.log"))
    }
}

#[cfg(all(test, not(target_family = "wasm")))]
mod tests {
    use super::*;

    fn empty_dir(test: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("pk-diag-{}-{test}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn read(dir: &Path, name: &str) -> String {
        fs::read_to_string(dir.join(name)).unwrap()
    }

    #[test]
    fn a_full_log_rolls_over_and_the_oldest_is_dropped() {
        let dir = empty_dir("roll");
        let mut log = RollingFile::open(dir.join("app.log"), 20);
        for n in 0..7 {
            log.write_line(&format!("line {n} of the log\n"));
        }
        assert_eq!(read(&dir, "app.log"), "line 6 of the log\n");
        assert_eq!(read(&dir, "app.1.log"), "line 5 of the log\n");
        assert_eq!(read(&dir, "app.2.log"), "line 4 of the log\n");
        assert_eq!(read(&dir, "app.3.log"), "line 3 of the log\n");
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 4, "nothing older is kept");
        drop(log);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_new_session_continues_the_log_and_counts_what_is_there() {
        let dir = empty_dir("continue");
        fs::write(dir.join("app.log"), "from the last session\n").unwrap();
        let mut log = RollingFile::open(dir.join("app.log"), 30);
        log.write_line("short\n");
        assert_eq!(read(&dir, "app.log"), "from the last session\nshort\n");
        log.write_line("this one does not fit\n");
        assert_eq!(read(&dir, "app.log"), "this one does not fit\n");
        assert_eq!(read(&dir, "app.1.log"), "from the last session\nshort\n");
        drop(log);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_log_that_cannot_be_opened_only_loses_lines() {
        let dir = empty_dir("unwritable");
        fs::create_dir(dir.join("app.log")).unwrap();
        let mut log = RollingFile::open(dir.join("app.log"), 20);
        log.write_line("nowhere to go\n");
        drop(log);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_tail_starts_at_a_whole_line() {
        let dir = empty_dir("tail");
        let file = dir.join("log");
        fs::write(&file, "first line\nsecond line\nthird\n").unwrap();
        assert_eq!(tail(&file, 1000), "first line\nsecond line\nthird\n");
        assert_eq!(tail(&file, 15), "third\n");
        assert_eq!(tail(&dir.join("missing"), 15), "");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn utc_reads_the_calendar() {
        assert_eq!(utc(0), (1970, 1, 1, 0, 0, 0));
        assert_eq!(utc(951_782_400), (2000, 2, 29, 0, 0, 0), "a leap day");
        assert_eq!(utc(1_794_268_799), (2026, 11, 9, 23, 59, 59));
        assert_eq!(utc(4_102_444_800), (2100, 1, 1, 0, 0, 0));
    }

    #[test]
    fn the_crash_report_carries_the_whole_story() {
        let t = utc(0);
        let text = crash_report(
            "Acme 1.0, macos aarch64",
            &utc_rfc3339(t),
            "Thread main panicked at src/lib.rs:1:1: boom",
            "  0: Acme::run",
            "WARN  Acme: the last lines",
        );
        assert!(text.starts_with("Acme 1.0, macos aarch64\nTime: 1970-01-01T00:00:00Z\n"));
        assert!(text.contains("Thread main panicked at src/lib.rs:1:1: boom"));
        assert!(text.contains("Backtrace:\n  0: Acme::run"));
        assert!(text.contains("Recent log:\nWARN  Acme: the last lines"));
        assert_eq!(utc_stamp(t), "19700101-000000");
    }

    #[test]
    fn the_section_heading_is_a_marker() {
        assert_eq!(section("Log"), "\n---- Log ----");
    }
}
