//! Removal of Electron's leftovers from its root, by exact name only. Entries
//! are never followed: symlinks and junctions are unlinked, not entered, and
//! nothing outside these lists (rust-v1, uploads, host, unknown files) is touched.
use cap_fs_ext::DirExt;
use cap_std::fs::Dir;
use std::{ffi::OsStr, io, path::Path};

/// Chromium caches Electron 44 rebuilds on its next start.
const CACHES: &[&str] = &[
    "Cache",
    "Code Cache",
    "GPUCache",
    "GPUPersistentCache",
    "GrShaderCache",
    "ShaderCache",
    "DawnCache",
    "DawnGraphiteCache",
    "DawnWebGPUCache",
    "Shared Dictionary",
    "blob_storage",
    "Crashpad",
    "Service Worker/CacheStorage",
    "Service Worker/ScriptCache",
];

/// Electron's own data and Chromium profile state, removed when the rollback window ends.
const ELECTRON_DATA: &[&str] = &[
    "manager.sqlite3",
    "manager.sqlite3-wal",
    "manager.sqlite3-shm",
    "manager.sqlite3-journal",
    "remote.json",
    "remote.json.new",
    "discovered-hosts.json",
    "ssh-profiles.json",
    "ssh-known-hosts.json",
    "tailscale",
    "Local State",
    "Preferences",
    "Local Storage",
    "Session Storage",
    "IndexedDB",
    "WebStorage",
    "Service Worker",
    "Network",
    "Partitions",
    "Dictionaries",
    "VideoDecodeStats",
    "databases",
    "Cookies",
    "Cookies-journal",
    "Network Persistent State",
    "TransportSecurity",
    "Trust Tokens",
    "Trust Tokens-journal",
    "SharedStorage",
    "SharedStorage-wal",
    "DIPS",
    "DIPS-journal",
    "DIPS-wal",
    "declarative_performance_observer.db",
    "declarative_performance_observer.db-journal",
    "lockfile",
    "SingletonLock",
    "SingletonCookie",
    "SingletonSocket",
];
/// Removed last: the lock is held while everything else goes.
pub(super) const LOCK_DIRECTORY: &str = ".host-lock";
const DEPTH_LIMIT: usize = 64;

/// Best effort; caches are rebuilt and any leftover goes with the later removal.
pub(super) fn remove_caches(root: &Dir) {
    for name in CACHES {
        if let Err(error) = remove(root, Path::new(name)) {
            eprintln!("Electron cache {name} was not removed: {error}");
        }
    }
}

/// Allowlisted entries that exist now. Only these are removed later.
pub(super) fn present(root: &Dir) -> Vec<String> {
    CACHES
        .iter()
        .chain(ELECTRON_DATA)
        .filter(|name| root.symlink_metadata(name).is_ok())
        .map(|name| (*name).to_owned())
        .collect()
}

/// Removes the allowlisted entries recorded in `present`. Returns whether all are gone.
pub(super) fn remove_electron_data(root: &Dir, present: &[String]) -> bool {
    let mut complete = true;
    let recorded = |name: &&&str| present.iter().any(|entry| entry == **name);
    for name in CACHES.iter().chain(ELECTRON_DATA).filter(recorded) {
        if let Err(error) = remove(root, Path::new(name)) {
            eprintln!("Electron data {name} was not removed: {error}");
            complete = false;
        }
    }
    complete
}

pub(super) fn remove_lock_directory(root: &Dir) -> bool {
    remove(root, Path::new(LOCK_DIRECTORY))
        .inspect_err(|error| eprintln!("Electron lock directory was not removed: {error}"))
        .is_ok()
}

/// `name` may have one parent component, opened without following links.
fn remove(root: &Dir, name: &Path) -> io::Result<()> {
    let leaf = name.file_name().ok_or(io::ErrorKind::InvalidInput)?;
    match name
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        Some(parent) => match root.open_dir_nofollow(parent) {
            Ok(dir) => remove_entry(&dir, leaf, 0),
            Err(error) if missing(&error) => Ok(()),
            Err(error) => Err(error),
        },
        None => remove_entry(root, leaf, 0),
    }
}

fn remove_entry(dir: &Dir, name: &OsStr, depth: usize) -> io::Result<()> {
    let info = match dir.symlink_metadata(name) {
        Ok(info) => info,
        Err(error) if missing(&error) => return Ok(()),
        Err(error) => return Err(error),
    };
    if info.is_dir() && !info.file_type().is_symlink() {
        if depth >= DEPTH_LIMIT {
            return Err(io::Error::other("directory tree is too deep"));
        }
        let child = dir.open_dir_nofollow(name)?;
        for entry in child.entries()? {
            remove_entry(&child, &entry?.file_name(), depth + 1)?;
        }
        drop(child);
        dir.remove_dir(name)
    } else {
        dir.remove_file_or_symlink(name)
    }
}

fn missing(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::NotFound
}

#[cfg(test)]
#[path = "electron_cleanup_tests.rs"]
mod tests;
