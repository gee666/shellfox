//! Each UI process owns one `run-<pid>-<stamp>` directory under the UI root
//! and holds an OS file lock on its `owner.lock` while it runs. The lock ends
//! with the process on every OS, so a lock that can be taken marks a dead
//! owner regardless of pid reuse. `remove_dir_all` never follows links.
use std::{
    fs::File,
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

const LOCK: &str = "owner.lock";
/// Run directories from before the lock (0.10.0-beta.2 and older) are removed
/// once untouched this long; a live UI of the same install forwards launches
/// to itself, so none still uses one.
const UNLOCKED_AGE: Duration = Duration::from_secs(10 * 60);

pub(super) struct RunLock {
    dir: PathBuf,
    _file: File,
}

pub(super) fn claim(run: &Path) -> std::io::Result<RunLock> {
    let file = File::options()
        .create(true)
        .truncate(false)
        .write(true)
        .open(run.join(LOCK))?;
    file.try_lock().map_err(std::io::Error::other)?;
    Ok(RunLock {
        dir: run.to_owned(),
        _file: file,
    })
}

impl RunLock {
    /// Unlocks, then removes the directory. A file another process still holds
    /// (a WebView2 profile on Windows) leaves it for the next start's sweep.
    pub fn remove(self) {
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
        // DirEntry::file_type does not follow links.
        if is_run
            && entry.file_type().is_ok_and(|kind| kind.is_dir())
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
        let held = claim(&live).unwrap();
        let dead = run("run-2-2");
        drop(claim(&dead).unwrap());
        let unlocked = run("run-3-3");
        let other = run("settings");
        sweep_older(ui.path(), Duration::from_secs(3600));
        assert!(live.exists() && !dead.exists() && unlocked.exists() && other.exists());
        sweep_older(ui.path(), Duration::ZERO);
        assert!(live.exists() && !unlocked.exists() && other.exists());
        held.remove();
        assert!(!live.exists());
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
