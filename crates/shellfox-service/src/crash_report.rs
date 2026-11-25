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
/// Makes the UI panic 5 s after its window opens; see desktop_windows.rs.
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
/// This process's own report. A later panic, such as the abort after a panic
/// in a callback that cannot unwind, is appended to it.
static OWN_REPORT: Mutex<Option<PathBuf>> = Mutex::new(None);

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
        let kind = if gpu_failure(message, &location) {
            GPU_FAILURE
        } else {
            "panic"
        };
        if let Some(path) = write(role, kind, &body) {
            eprintln!("Shellfox crash report: {}", path.display());
        }
        previous(info);
    }));
}

const GPU_FAILURE: &str = "GPU failure";

/// blade-graphics, GPUI's Vulkan and Metal layer, panics when the device is
/// lost (a driver reset or GPU hang) or out of memory. Nothing in GPUI 0.2.2
/// can recreate its renderer, so only a new UI process recovers.
fn gpu_failure(message: &str, location: &str) -> bool {
    message.starts_with("GPU has crashed")
        || message.starts_with("GPU error")
        || location.contains("blade-graphics")
}

/// Whether the report at `path` is about a GPU failure.
pub fn is_gpu_failure(path: &Path) -> bool {
    use std::io::BufRead;
    let mut first = String::new();
    std::fs::File::open(path)
        .map(|file| std::io::BufReader::new(file).read_line(&mut first))
        .is_ok_and(|read| read.is_ok())
        && first.trim_end().ends_with(GPU_FAILURE)
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

/// Writes `crash-<time>-<role>.txt` about the `role` process, with the tail
/// of its log.
fn write(role: &str, kind: &str, body: &str) -> Option<PathBuf> {
    let reporter = REPORTER.get()?;
    if WRITING.swap(true, Ordering::SeqCst) {
        return None;
    }
    let path = write_unguarded(reporter, role, kind, body);
    WRITING.store(false, Ordering::SeqCst);
    path
}

fn write_unguarded(reporter: &Reporter, role: &str, kind: &str, body: &str) -> Option<PathBuf> {
    let own = role == reporter.role;
    let mut first = OWN_REPORT.lock().unwrap_or_else(|error| error.into_inner());
    if let (true, Some(path)) = (own, first.as_ref()) {
        use std::io::Write as _;
        let mut file = std::fs::OpenOptions::new().append(true).open(path).ok()?;
        file.write_all(format!("\n\nThen, {kind}:\n{body}\n").as_bytes())
            .ok()?;
        return Some(path.clone());
    }
    let now = SystemTime::now();
    let mut text = format!(
        "Shellfox {role} {kind}\nVersion: {} ({})\nTime: {} UTC\n",
        reporter.version,
        reporter.build,
        timestamp(now, " "),
    );
    let pid = std::process::id();
    let _ = if own {
        writeln!(text, "Pid: {pid}")
    } else {
        writeln!(text, "Reported by: {} pid {pid}", reporter.role)
    };
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
    let log = reporter.dir.join(format!("{role}.log"));
    let _ = write!(
        text,
        "\n{body}\n\nLast log lines ({}):\n{}",
        log.display(),
        stderr_log::tail(&log, TAIL_LINES)
    );
    let path = reporter
        .dir
        .join(format!("crash-{}-{role}.txt", timestamp(now, "-")));
    std::fs::write(&path, text).ok()?;
    prune(&reporter.dir);
    if own {
        *first = Some(path.clone());
    }
    Some(path)
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

/// The executable's lowest mapping, which is its load address (PIE).
#[cfg(target_os = "linux")]
fn linux_image_base() -> Option<String> {
    let exe = std::fs::read_link("/proc/self/exe").ok()?;
    let exe = exe.to_str()?;
    let maps = std::fs::read_to_string("/proc/self/maps").ok()?;
    let start = maps
        .lines()
        .find(|line| line.ends_with(exe))?
        .split('-')
        .next()?;
    Some(format!("0x{start}"))
}

/// OS, desktop session and display backend, as far as the environment tells.
fn system() -> Vec<(&'static str, String)> {
    let mut facts = vec![(
        "OS",
        format!("{} {}", std::env::consts::OS, std::env::consts::ARCH),
    )];
    // Releases ship no debug info (PDBs, dSYMs, Linux .debug files), so
    // backtraces are bare addresses. With the load address they resolve
    // offline against the release workflow's symbols artifact.
    #[cfg(target_os = "macos")]
    {
        unsafe extern "C" {
            fn _dyld_get_image_header(index: u32) -> *const std::ffi::c_void;
        }
        // SAFETY: image 0 is always the main executable.
        let base = unsafe { _dyld_get_image_header(0) };
        facts.push(("Image base", format!("{base:p}")));
    }
    #[cfg(windows)]
    {
        // SAFETY: a null name asks for this executable's own module handle.
        let base = unsafe {
            windows_sys::Win32::System::LibraryLoader::GetModuleHandleW(std::ptr::null())
        };
        facts.push(("Image base", format!("{base:p}")));
    }
    #[cfg(target_os = "linux")]
    facts.extend(linux_image_base().map(|base| ("Image base", base)));
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

    #[cfg(target_os = "linux")]
    #[test]
    fn the_linux_image_base_maps_this_executable() {
        let base = linux_image_base().expect("image base");
        let address = usize::from_str_radix(base.trim_start_matches("0x"), 16).unwrap();
        let here = linux_image_base as fn() -> Option<String> as usize;
        assert!(address != 0 && address <= here, "{base} vs {here:#x}");
    }

    #[test]
    fn gpu_failures_are_told_apart() {
        let blade = "/r/blade-graphics-0.7.1/src/vulkan/resource.rs:314:9";
        assert!(gpu_failure(
            "called `Result::unwrap()` on an `Err` value",
            blade
        ));
        assert!(gpu_failure(
            "GPU has crashed, and no debug information is available.",
            "x"
        ));
        assert!(!gpu_failure("index out of bounds", "crates/a.rs:1:1"));
        let dir = tempfile::tempdir().unwrap();
        let report = dir.path().join("crash.txt");
        std::fs::write(&report, "Shellfox ui GPU failure\nVersion: x\n").unwrap();
        assert!(is_gpu_failure(&report));
        std::fs::write(&report, "Shellfox ui panic\nGPU failure\n").unwrap();
        assert!(!is_gpu_failure(&report));
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
