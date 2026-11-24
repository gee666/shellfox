//! Each UI process owns one `run-<pid>-<stamp>` directory under the UI root
//! and holds an OS file lock on its `owner.lock` while it runs. The lock ends
//! with the process on every OS, so a lock that can be taken marks a dead
//! owner regardless of pid reuse. `remove_dir_all` never follows links.
//! One process-wide holder keeps the lock of every run directory this
//! process prepared until it exits, though a window's `FrontendDirectories`
//! is dropped early. A directory is locked once: on Unix, flock conflicts
//! between two descriptors of the same process, so a second lock would fail.
use std::{
    fs::File,
    path::{Path, PathBuf},
    sync::Mutex,
    time::{Duration, SystemTime},
};

const LOCK: &str = "owner.lock";
/// Run directories from before the lock (0.10.0-beta.2 and older) are removed
/// once untouched this long; a live UI of the same install forwards launches
/// to itself, so none still uses one.
const UNLOCKED_AGE: Duration = Duration::from_secs(10 * 60);

struct RunLock {
    dir: PathBuf,
    _file: File,
}
static HELD: Mutex<Vec<RunLock>> = Mutex::new(Vec::new());

/// Locks `run` for this process. Preparing the same directory again, as
/// a second `FrontendDirectories` for it does, shares the held lock.
pub(super) fn claim(run: &Path) -> std::io::Result<()> {
    let mut held = held();
    if holds(&held, run) {
        return Ok(());
    }
    let file = File::options()
        .create(true)
        .truncate(false)
        .write(true)
        .open(run.join(LOCK))?;
    file.try_lock().map_err(std::io::Error::other)?;
    held.push(RunLock {
        dir: run.to_owned(),
        _file: file,
    });
    Ok(())
}

fn holds(held: &[RunLock], run: &Path) -> bool {
    held.iter().any(|lock| lock.dir == run)
}

fn held() -> std::sync::MutexGuard<'static, Vec<RunLock>> {
    HELD.lock().unwrap_or_else(|error| error.into_inner())
}

/// Removes this process's run directories.
pub(super) fn release_all() {
    release_where(|_| true);
}

fn release_where(mut matches: impl FnMut(&Path) -> bool) {
    let released: Vec<_> = {
        let mut held = held();
        let (released, kept) = std::mem::take(&mut *held)
            .into_iter()
            .partition(|lock| matches(&lock.dir));
        *held = kept;
        released
    };
    for lock in released {
        lock.remove();
    }
}

impl RunLock {
    /// Unlocks, then removes the directory. A file another process still holds
    /// (a WebView2 profile on Windows) leaves it for the next start's sweep.
    fn remove(self) {
        let RunLock { dir, _file } = self;
        drop(_file);
        if let Err(error) = std::fs::remove_dir_all(&dir)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            eprintln!("Run directory {} kept: {error}", dir.display());
        }
    }
}

/// Removes the run directories of UI processes that are gone.
pub(super) fn sweep(ui: &Path) {
    sweep_older(ui, UNLOCKED_AGE);
}

fn sweep_older(ui: &Path, unlocked_age: Duration) {
    let Ok(entries) = std::fs::read_dir(ui) else {
        return;
    };
    for entry in entries.flatten() {
        let is_run = entry
            .file_name()
            .to_str()
            .is_some_and(|name| name.starts_with("run-"));
        // DirEntry::file_type does not follow links. This process's own
        // directories are skipped by name: lock semantics within one process
        // differ between OSes.
        if is_run
            && entry.file_type().is_ok_and(|kind| kind.is_dir())
            && !holds(&held(), &entry.path())
            && abandoned(&entry.path(), unlocked_age)
        {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

fn abandoned(run: &Path, unlocked_age: Duration) -> bool {
    let lock = run.join(LOCK);
    match std::fs::symlink_metadata(&lock) {
        Ok(metadata) if metadata.is_file() => {
            File::open(&lock).is_ok_and(|file| file.try_lock().is_ok())
        }
        Ok(_) => false,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => std::fs::metadata(run)
            .and_then(|metadata| metadata.modified())
            .is_ok_and(|modified| {
                SystemTime::now()
                    .duration_since(modified)
                    .is_ok_and(|age| age >= unlocked_age)
            }),
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_runs_of_gone_processes_are_swept() {
        let ui = tempfile::tempdir().unwrap();
        let run = |name: &str| {
            let path = ui.path().join(name);
            std::fs::create_dir(&path).unwrap();
            std::fs::write(path.join("profile"), "x").unwrap();
            path
        };
        let live = run("run-1-1");
        claim(&live).unwrap();
        // A lock file no process holds, as a gone owner leaves it.
        let dead = run("run-2-2");
        std::fs::write(dead.join(LOCK), "").unwrap();
        let unlocked = run("run-3-3");
        let other = run("settings");
        sweep_older(ui.path(), Duration::from_secs(3600));
        assert!(live.exists() && !dead.exists() && unlocked.exists() && other.exists());
        sweep_older(ui.path(), Duration::ZERO);
        assert!(live.exists() && !unlocked.exists() && other.exists());
        release_where(|dir| dir == live);
        assert!(!live.exists());
    }

    #[test]
    fn one_process_prepares_a_run_directory_twice_and_keeps_it() {
        let ui = tempfile::tempdir().unwrap();
        let run = ui.path().join("run-5-5");
        std::fs::create_dir(&run).unwrap();
        claim(&run).unwrap();
        claim(&run).unwrap();
        let other = ui.path().join("run-6-6");
        std::fs::create_dir(&other).unwrap();
        claim(&other).unwrap();
        sweep_older(ui.path(), Duration::ZERO);
        assert!(run.exists() && other.exists());
        release_where(|dir| dir == run || dir == other);
        assert!(!run.exists() && !other.exists());
    }

    #[cfg(unix)]
    #[test]
    fn a_linked_run_directory_is_not_followed() {
        let ui = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("keep"), "x").unwrap();
        std::os::unix::fs::symlink(outside.path(), ui.path().join("run-4-4")).unwrap();
        sweep_older(ui.path(), Duration::ZERO);
        assert!(outside.path().join("keep").exists());
    }
}
