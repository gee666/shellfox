//! No-follow reads from the Electron root and the private database snapshot.
use anyhow::{bail, ensure, Result};
use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::{Dir, File, OpenOptions};
use std::{
    ffi::OsStr,
    io::{self, Read},
    path::Path,
    time::SystemTime,
};
use tempfile::TempDir;
use zeroize::Zeroizing;

pub(super) const DATABASE: &str = "manager.sqlite3";
const WAL: &str = "manager.sqlite3-wal";
const JOURNAL: &str = "manager.sqlite3-journal";
const DB_LIMIT: u64 = 256 * 1024 * 1024;

pub(super) fn open_dir(dir: &Dir, name: &str) -> Result<Option<Dir>> {
    match dir.open_dir_nofollow(name) {
        Ok(dir) => Ok(Some(dir)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

pub(super) fn open(dir: &Dir, name: &str) -> Result<Option<File>> {
    match dir.symlink_metadata(name) {
        Ok(info) => ensure!(
            info.is_file() && !info.is_symlink(),
            "Invalid Electron data file"
        ),
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    }
    let mut options = OpenOptions::new();
    options.read(true).follow(FollowSymlinks::No);
    let file = dir.open_with(name, &options)?;
    ensure!(file.metadata()?.is_file(), "Invalid Electron data file");
    Ok(Some(file))
}

pub(super) fn read(dir: &Dir, name: &str, limit: u64) -> Result<Option<Vec<u8>>> {
    let Some(file) = open(dir, name)? else {
        return Ok(None);
    };
    let mut bytes = Zeroizing::new(Vec::new());
    file.take(limit + 1).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= limit,
        "Electron data file is too large"
    );
    Ok(Some(std::mem::take(&mut *bytes)))
}

pub(super) fn exists(dir: &Dir, name: impl AsRef<Path>) -> Result<bool> {
    match dir.symlink_metadata(name) {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e.into()),
    }
}

/// Electron and the Node headless host hold `.host-lock/host.lock` through
/// their Go helper (flock or LockFileEx) while they run. Holding it here also
/// keeps them from starting while their files are read or removed.
pub(super) fn electron_lock(source: &Dir) -> Result<Option<std::fs::File>> {
    let Some(dir) = open_dir(source, ".host-lock")? else {
        return Ok(None);
    };
    let Some(file) = open(&dir, "host.lock")? else {
        return Ok(None);
    };
    let file = file.into_std();
    match file.try_lock() {
        Ok(()) => Ok(Some(file)),
        Err(std::fs::TryLockError::WouldBlock) => {
            bail!("Electron Shellfox is running; its data is imported after it quits")
        }
        Err(std::fs::TryLockError::Error(error)) => Err(error.into()),
    }
}

fn stamp(file: &File) -> io::Result<(u64, SystemTime)> {
    let info = file.metadata()?;
    Ok((info.len(), info.modified()?.into_std()))
}

/// Copy the database and its WAL into a private temp dir, refusing any change
/// observed while copying. SQLite only ever opens the copy.
pub(super) fn database_snapshot(source: &Dir, control: &Path) -> Result<Option<TempDir>> {
    let Some(mut database) = open(source, DATABASE)? else {
        return Ok(None);
    };
    let mut wal = open(source, WAL)?;
    let before = (stamp(&database)?, wal.as_ref().map(stamp).transpose()?);
    // Electron uses WAL. Don't try to recover a rollback journal in the snapshot.
    ensure!(
        open(source, JOURNAL)?.is_none(),
        "Electron database is being updated"
    );
    let temp = tempfile::Builder::new()
        .prefix("electron-import-")
        .tempdir_in(control)?;
    let copy = |name: &str, input: &mut File, size| -> Result<()> {
        ensure!(
            size <= DB_LIMIT,
            "Electron database exceeds the import limit"
        );
        let mut out = std::fs::File::create_new(temp.path().join(name))?;
        ensure!(
            io::copy(&mut input.take(size + 1), &mut out)? == size,
            "Electron database changed during import"
        );
        Ok(())
    };
    copy(DATABASE, &mut database, before.0 .0)?;
    if let (Some(wal), Some((size, _))) = (&mut wal, before.1) {
        copy(WAL, wal, size)?;
    }
    let after = (
        open(source, DATABASE)?.as_ref().map(stamp).transpose()?,
        open(source, WAL)?.as_ref().map(stamp).transpose()?,
    );
    ensure!(
        (Some(before.0), before.1) == after && open(source, JOURNAL)?.is_none(),
        "Electron database changed during import; quit Electron and reopen Shellfox"
    );
    Ok(Some(temp))
}

/// Publish `bytes` as a new private leaf, or replace the `expected` version.
pub(super) fn publish_private(
    dir: &Dir,
    name: &str,
    bytes: &[u8],
    expected: Option<&shellfox_files::PrivateVersion>,
) -> Result<()> {
    use shellfox_files::PrivatePublishOutcome;
    let parent = dir.try_clone()?.into_std_file();
    let stage = format!(".{name}-{}.new", uuid::Uuid::new_v4().simple());
    let mut attempt = match shellfox_files::prepare_private_publication_at(
        &parent,
        OsStr::new(name),
        OsStr::new(&stage),
        expected,
    ) {
        Ok(attempt) => attempt,
        Err(failure) => {
            let (error, attempt) = failure.into_parts();
            if let Some(mut attempt) = attempt {
                let _ = attempt.cleanup();
            }
            bail!("Could not stage {name}: {}", error.message);
        }
    };
    if let Err(error) = attempt.write_all(bytes) {
        let _ = attempt.cleanup();
        bail!("Could not write {name}: {}", error.message);
    }
    match attempt.publish() {
        PrivatePublishOutcome::Published(_) => {
            // Close our descriptor and keep the published file.
            attempt.preserve_stale();
            Ok(())
        }
        _ => {
            let _ = attempt.cleanup();
            bail!("Could not publish {name}")
        }
    }
}
