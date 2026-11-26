//! One launch at a time per data directory, like 0.9's single-instance lock.
//! A launch holds `<data>.launch.lock` from its host lookup until its window
//! registers with the host. A second launch waits for it, then finds that
//! window and hands its request over instead of starting a host and a window
//! of its own. The OS drops the lock when the process ends, so a crashed or
//! stuck launch delays the next one at most `WAIT`.
use std::{
    fs::File,
    path::Path,
    sync::Mutex,
    time::{Duration, Instant},
};

const WAIT: Duration = Duration::from_secs(60);
static HELD: Mutex<Option<File>> = Mutex::new(None);

/// Waits until no other launch on `data_dir` is starting. Past `WAIT` this
/// launch goes on without the lock; a host started meanwhile loses the
/// directory lock and this launch attaches to the winner.
pub(crate) fn acquire(data_dir: &Path) {
    let mut name = data_dir.file_name().unwrap_or_default().to_owned();
    name.push(".launch.lock");
    let path = data_dir.with_file_name(name);
    let file = path
        .parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|()| {
            File::options()
                .create(true)
                .truncate(false)
                .write(true)
                .open(&path)
        });
    let file = match file {
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
    *held() = Some(file);
}

/// Lets the next launch in: this one's window registered, or it never will.
pub(crate) fn release() {
    held().take();
}

fn held() -> std::sync::MutexGuard<'static, Option<File>> {
    HELD.lock().unwrap_or_else(|error| error.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn the_lock_is_free_again_after_release() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("host");
        acquire(&data);
        let other = File::options()
            .write(true)
            .open(dir.path().join("host.launch.lock"))
            .unwrap();
        assert!(other.try_lock().is_err(), "a held launch lock was free");
        release();
        other.try_lock().unwrap();
    }
}
