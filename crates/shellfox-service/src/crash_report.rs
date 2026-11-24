//! Crash reports and stderr logs of the desktop UI and its host.
//!
//! Both live in `<data>.logs/`, beside the host data and UI directories: one
//! `<role>.log` (plus `.1`) per process role, and the last ten `crash-*.txt`.
//! A UI started from a desktop entry has no useful stderr, so `install`
//! routes it into the log before anything else prints.
mod stderr_log;

use std::{
    fmt::Write as _,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex, OnceLock,
    },
    time::SystemTime,
};

const KEEP_REPORTS: usize = 10;
const TAIL_LINES: usize = 200;
/// Set to make the UI panic once its window is open; tests use it.
pub const TEST_PANIC_ENV: &str = "SHELLFOX_TEST_PANIC";

struct Reporter {
    dir: PathBuf,
    role: &'static str,
    version: &'static str,
    build: &'static str,
}
static REPORTER: OnceLock<Reporter> = OnceLock::new();
static NOTES: Mutex<Vec<(&'static str, String)>> = Mutex::new(Vec::new());
static WRITING: AtomicBool = AtomicBool::new(false);

/// `<data>.logs`, the sibling of the host data directory.
pub fn logs_dir(data_dir: &Path) -> PathBuf {
    let mut name = data_dir.file_name().unwrap_or_default().to_os_string();
    name.push(".logs");
    data_dir.with_file_name(name)
}

/// Routes stderr into `<role>.log` and writes a crash report on any panic.
/// Call once, first thing after the data directory is known.
pub fn install(data_dir: &Path, role: &'static str, version: &'static str, build: &'static str) {
    let dir = logs_dir(data_dir);
    if let Err(error) = stderr_log::capture(&dir, role) {
        eprintln!("Shellfox {role} log unavailable: {error}");
    }
    eprintln!(
        "--- Shellfox {role} {version} ({build}) pid {} started {} UTC ---",
        std::process::id(),
        timestamp(SystemTime::now(), " ")
    );
    if log::set_logger(&StderrLogger).is_ok() {
        log::set_max_level(log::LevelFilter::Warn);
    }
    let _ = REPORTER.set(Reporter {
        dir,
        role,
        version,
        build,
    });
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let message = info.payload_as_str().unwrap_or("(no message)");
        let location = info
            .location()
            .map_or_else(|| "unknown".to_owned(), ToString::to_string);
        let thread = std::thread::current();
        let body = format!(
            "Panic: {message}\nAt: {location}\nThread: {}\n\nBacktrace:\n{}",
            thread.name().unwrap_or("unnamed"),
            std::backtrace::Backtrace::force_capture()
        );
        if let Some(path) = write(role, "panic", &body) {
            eprintln!("Shellfox crash report: {}", path.display());
        }
        previous(info);
    }));
}

/// Warnings and errors of GPUI and other crates, which log through `log`.
struct StderrLogger;
impl log::Log for StderrLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Warn
    }
    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            eprintln!("{} {}: {}", record.level(), record.target(), record.args());
        }
    }
    fn flush(&self) {}
}

/// Adds a line to every later report, e.g. the GPU adapter.
pub fn note(key: &'static str, value: String) {
    let mut notes = NOTES.lock().unwrap_or_else(|error| error.into_inner());
    notes.retain(|(known, _)| *known != key);
    notes.push((key, value));
}

/// A fatal exit that is not a panic, such as an error returned from main.
pub fn fatal(error: &anyhow::Error) -> Option<PathBuf> {
    let role = REPORTER.get()?.role;
    write(role, "fatal error", &format!("Error: {error:#}"))
}

/// The host's report when its UI ended without the quit byte. A report the
/// UI wrote itself since `since` is returned instead of a new one.
pub fn ui_exit(since: SystemTime) -> Option<PathBuf> {
    let reporter = REPORTER.get()?;
    if let Some(own) = reports(&reporter.dir)
        .into_iter()
        .rev()
        .find(|(name, modified)| name.ends_with("-ui.txt") && *modified >= since)
    {
        return Some(reporter.dir.join(own.0));
    }
    write(
        "ui",
        "unexpected exit",
        "The UI ended without a crash report: a native crash, a signal or an \
         out-of-memory kill. Its last log lines follow.",
    )
}

/// Writes `crash-<time>-<process role>.txt` with the tail of `log_role`'s log.
fn write(log_role: &str, kind: &str, body: &str) -> Option<PathBuf> {
    let reporter = REPORTER.get()?;
    if WRITING.swap(true, Ordering::SeqCst) {
        return None;
    }
    let now = SystemTime::now();
    let mut text = format!(
        "Shellfox {} {kind}\nVersion: {} ({})\nTime: {} UTC\nPid: {}\n",
        reporter.role,
        reporter.version,
        reporter.build,
        timestamp(now, " "),
        std::process::id()
    );
    for (key, value) in system() {
        let _ = writeln!(text, "{key}: {value}");
    }
    for (key, value) in NOTES
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .iter()
    {
        let _ = writeln!(text, "{key}: {value}");
    }
    let log = reporter.dir.join(format!("{log_role}.log"));
    let _ = write!(
        text,
        "\n{body}\n\nLast log lines ({}):\n{}",
        log.display(),
        stderr_log::tail(&log, TAIL_LINES)
    );
    let path = reporter.dir.join(format!(
        "crash-{}-{}.txt",
        timestamp(now, "-"),
        reporter.role
    ));
    let written = std::fs::write(&path, text).is_ok();
    prune(&reporter.dir);
    WRITING.store(false, Ordering::SeqCst);
    written.then_some(path)
}

/// Crash reports by name, oldest first, with their modification times.
fn reports(dir: &Path) -> Vec<(String, SystemTime)> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut reports: Vec<_> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            let metadata = entry.metadata().ok()?;
            (name.starts_with("crash-") && name.ends_with(".txt") && metadata.is_file())
                .then(|| (name, metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH)))
        })
        .collect();
    reports.sort();
    reports
}

fn prune(dir: &Path) {
    let reports = reports(dir);
    let excess = reports.len().saturating_sub(KEEP_REPORTS);
    for (name, _) in &reports[..excess] {
        let _ = std::fs::remove_file(dir.join(name));
    }
}

/// OS, desktop session and display backend, as far as the environment tells.
fn system() -> Vec<(&'static str, String)> {
    let mut facts = vec![(
        "OS",
        format!("{} {}", std::env::consts::OS, std::env::consts::ARCH),
    )];
    if cfg!(target_os = "linux") {
        let release = std::fs::read_to_string("/etc/os-release").unwrap_or_default();
        if let Some(name) = release
            .lines()
            .find_map(|line| line.strip_prefix("PRETTY_NAME="))
        {
            facts.push(("Distribution", name.trim_matches('"').to_owned()));
        }
        if let Ok(kernel) = std::fs::read_to_string("/proc/sys/kernel/osrelease") {
            facts.push(("Kernel", kernel.trim().to_owned()));
        }
        for key in [
            "XDG_SESSION_TYPE",
            "XDG_CURRENT_DESKTOP",
            "DESKTOP_SESSION",
            "DISPLAY",
            "WAYLAND_DISPLAY",
            "GPUI_X11_SCALE_FACTOR",
        ] {
            if let Some(value) = std::env::var_os(key) {
                facts.push((key, value.to_string_lossy().into_owned()));
            }
        }
    }
    facts
}

/// `YYYYMMDD<sep>HHMMSS` in UTC.
fn timestamp(time: SystemTime, separator: &str) -> String {
    let seconds = time
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    let (days, day) = (seconds / 86_400, seconds % 86_400);
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let shifted = days as i64 + 719_468;
    let era = shifted.div_euclid(146_097);
    let of_era = shifted.rem_euclid(146_097);
    let year_of_era = (of_era - of_era / 1460 + of_era / 36_524 - of_era / 146_096) / 365;
    let of_year = of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * of_year + 2) / 153;
    let date = of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}{month:02}{date:02}{separator}{:02}{:02}{:02}",
        day / 3600,
        day / 60 % 60,
        day % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn timestamps_are_utc_and_sortable() {
        let time = SystemTime::UNIX_EPOCH + Duration::from_secs(1_795_534_502);
        assert_eq!(timestamp(time, "-"), "20261124-153502");
        let leap = SystemTime::UNIX_EPOCH + Duration::from_secs(951_782_400);
        assert_eq!(timestamp(leap, "-"), "20000229-000000");
        assert_eq!(timestamp(SystemTime::UNIX_EPOCH, " "), "19700101 000000");
    }

    #[test]
    fn logs_sit_beside_the_data_directory() {
        let data = Path::new("/home/u/.config/Shellfox/rust-v1/host");
        assert_eq!(
            logs_dir(data),
            Path::new("/home/u/.config/Shellfox/rust-v1/host.logs")
        );
    }

    #[test]
    fn only_the_newest_reports_are_kept() {
        let dir = tempfile::tempdir().unwrap();
        for index in 0..13 {
            std::fs::write(dir.path().join(format!("crash-2026{index:04}-ui.txt")), "").unwrap();
        }
        std::fs::write(dir.path().join("ui.log"), "").unwrap();
        prune(dir.path());
        let names: Vec<_> = reports(dir.path())
            .into_iter()
            .map(|(name, _)| name)
            .collect();
        assert_eq!(names.len(), KEEP_REPORTS);
        assert_eq!(names[0], "crash-20260003-ui.txt");
        assert!(dir.path().join("ui.log").exists());
    }
}
