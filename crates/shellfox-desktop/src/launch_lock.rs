//! One launch at a time per data directory, like 0.9's single-instance lock.
//! A launch holds `<data>.launch.lock` from its host lookup until its window
//! registers with the host. A second launch waits for it, then finds that
//! window and hands its request over instead of starting a host and a window
//! of its own.
//!
//! The process whose window registered then holds `<data>.window.lock` while
//! its host serves. A launch that sees it held knows a host is up, so a lookup
//! that misses it (as on loaded macOS and arm64 runners right after the
//! window registered) is retried instead of starting a competing host.
//!
//! The OS drops both locks when the process ends, so a crashed or stuck
//! launch delays the next one at most `WAIT`.
use std::{
    fs::File,
    path::{Path, PathBuf},
    sync::Mutex,
    time::{Duration, Instant},
};

const WAIT: Duration = Duration::from_secs(60);

struct Held {
    data_dir: Option<PathBuf>,
    launch: Option<File>,
    window: Option<File>,
}
static HELD: Mutex<Held> = Mutex::new(Held {
    data_dir: None,
    launch: None,
    window: None,
});

fn lock_path(data_dir: &Path, kind: &str) -> PathBuf {
    let mut name = data_dir.file_name().unwrap_or_default().to_owned();
    name.push(format!(".{kind}.lock"));
    data_dir.with_file_name(name)
}

fn open(data_dir: &Path, kind: &str) -> std::io::Result<File> {
    let path = lock_path(data_dir, kind);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    File::options()
        .create(true)
        .truncate(false)
        .write(true)
        .open(path)
}

/// Waits until no other launch on `data_dir` is starting. Past `WAIT` this
/// launch goes on without the lock; a host started meanwhile loses the
/// directory lock and this launch attaches to the winner.
pub(crate) fn acquire(data_dir: &Path) {
    held().data_dir = Some(data_dir.to_owned());
    let file = match open(data_dir, "launch") {
        Ok(file) => file,
        Err(error) => {
            eprintln!("Launch lock unavailable: {error}");
            return;
        }
    };
    let deadline = Instant::now() + WAIT;
    loop {
        match file.try_lock() {
            Ok(()) => break,
            Err(std::fs::TryLockError::WouldBlock) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(error) => {
                eprintln!("Launch lock not taken, going on without it: {error}");
                return;
            }
        }
    }
    held().launch = Some(file);
}

/// Lets the next launch in: this one forwarded, quit, or its window could
/// not register.
pub(crate) fn release() {
    held().launch.take();
}

/// This process's window registered with the host: mark the host as serving
/// a window, then let the next launch in to find it.
pub(crate) fn registered() {
    let mut held = held();
    if held.window.is_none()
        && let Some(data_dir) = held.data_dir.clone()
    {
        match open(&data_dir, "window").and_then(|file| {
            file.try_lock().map_err(std::io::Error::other)?;
            Ok(file)
        }) {
            Ok(file) => held.window = Some(file),
            Err(error) => eprintln!("Window lock not taken: {error}"),
        }
    }
    held.launch.take();
}

/// The host this process's window used is gone.
pub(crate) fn host_ended() {
    held().window.take();
}

/// Whether another process's window is registered with the host of `data_dir`.
pub(crate) fn window_alive(data_dir: &Path) -> bool {
    open(data_dir, "window")
        .is_ok_and(|file| matches!(file.try_lock(), Err(std::fs::TryLockError::WouldBlock)))
}

fn held() -> std::sync::MutexGuard<'static, Held> {
    HELD.lock().unwrap_or_else(|error| error.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn the_launch_lock_is_free_after_release_and_the_window_lock_after_its_host() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("host");
        let other = |kind| {
            File::options()
                .write(true)
                .open(dir.path().join(format!("host.{kind}.lock")))
                .unwrap()
        };
        acquire(&data);
        assert!(
            other("launch").try_lock().is_err(),
            "a held launch lock was free"
        );
        release();
        other("launch").try_lock().unwrap();

        acquire(&data);
        registered();
        other("launch").try_lock().unwrap();
        assert!(
            other("window").try_lock().is_err(),
            "a registered window was not marked"
        );
        host_ended();
        other("window").try_lock().unwrap();
    }
}
