//! The import marker and the end of the rollback window.
use super::*;

/// Name, then size and mtime in ms when present.
pub(super) type Fingerprint = Vec<(String, Option<(u64, u128)>)>;

#[derive(Default, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Imported {
    pub(super) imported_at_ms: u64,
    /// False when the root held no Electron data; nothing is ever removed then.
    pub(super) electron: bool,
    /// Allowlisted entries that existed at import time.
    pub(super) present: Vec<String>,
    /// Size and mtime of Electron's own files. A later change means Electron
    /// ran again after the import, so its data is not removed.
    pub(super) fingerprint: Fingerprint,
}

/// The rollback window has ended: remove what the import found, unless Electron ran since.
pub(super) fn cleanup(
    source: &Dir,
    root: &Path,
    control: &Path,
    marker: &Imported,
    check: &impl Fn() -> Result<()>,
) -> Result<()> {
    if !marker.electron {
        return write_marker(control, CLEANUP_MARKER, &false, check);
    }
    let lock = match take_held(root) {
        Some(lock) => Some(lock),
        None => files::electron_lock(source)?,
    };
    if fingerprint(source)? != marker.fingerprint {
        eprintln!("Electron Shellfox was used after the import; its data is kept.");
        return write_marker(control, CLEANUP_MARKER, &false, check);
    }
    if !cleanup::remove_electron_data(source, &marker.present) {
        return Ok(());
    }
    // Unix can unlink a held lock, so no Electron takes it in between.
    // Windows refuses the delete while Electron holds it, and retries next launch.
    #[cfg(windows)]
    drop(lock);
    let removed = cleanup::remove_lock_directory(source);
    #[cfg(not(windows))]
    drop(lock);
    if removed {
        write_marker(control, CLEANUP_MARKER, &true, check)?;
    }
    Ok(())
}

pub(super) fn finish_without_electron(
    control: &Path,
    check: &impl Fn() -> Result<()>,
) -> Result<()> {
    for name in [WINDOW_MARKER, MARKER] {
        if control.join(name).symlink_metadata().is_err() {
            write_marker(control, name, &false, check)?;
        }
    }
    if control.join(MARKER_V2).symlink_metadata().is_err() {
        let marker = Imported {
            imported_at_ms: now_ms(),
            ..Default::default()
        };
        write_marker(control, MARKER_V2, &marker, check)?;
    }
    Ok(())
}

pub(super) fn fingerprint(source: &Dir) -> Result<Fingerprint> {
    let mut stamps = Vec::new();
    for name in [files::DATABASE, "manager.sqlite3-wal", remote::REMOTE, SSH] {
        stamps.push((name.to_owned(), files::stamp_of(source, name)?));
    }
    let node = match files::open_dir(source, "tailscale")? {
        Some(dir) => files::stamp_of(&dir, "tailscaled.state")?,
        None => None,
    };
    stamps.push(("tailscale/tailscaled.state".into(), node));
    Ok(stamps)
}

/// A crash can leave a private database snapshot, which holds saved environment values.
pub(super) fn remove_stale_snapshots(control: &Path) {
    let Ok(entries) = std::fs::read_dir(control) else {
        return;
    };
    for entry in entries.flatten() {
        let is_dir = entry.file_type().is_ok_and(|kind| kind.is_dir());
        if is_dir
            && entry
                .file_name()
                .to_string_lossy()
                .starts_with("electron-import-")
        {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

/// The v2 marker. A time in the future (the clock was wrong at import) is reset to now.
pub(super) fn imported(
    control: &Path,
    check: &impl Fn() -> Result<()>,
) -> Result<Option<Imported>> {
    let path = control.join(MARKER_V2);
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    // An unreadable marker counts as imported but never expires.
    let Ok(mut marker) = serde_json::from_slice::<Imported>(&bytes) else {
        return Ok(Some(Imported {
            imported_at_ms: u64::MAX,
            ..Default::default()
        }));
    };
    if marker.imported_at_ms > now_ms() {
        marker.imported_at_ms = now_ms();
        std::fs::remove_file(&path)?;
        write_marker(control, MARKER_V2, &marker, check)?;
    }
    Ok(Some(marker))
}

/// Electron's lock, held until this process exits once its node identity is
/// copied, so Electron or the Node headless host can't run next to this host.
static HELD: std::sync::Mutex<Vec<(std::path::PathBuf, std::fs::File)>> =
    std::sync::Mutex::new(Vec::new());

pub(super) fn hold(root: &Path, lock: std::fs::File) {
    if let Ok(mut held) = HELD.lock() {
        held.push((root.to_owned(), lock));
    }
}

pub(super) fn take_held(root: &Path) -> Option<std::fs::File> {
    let mut held = HELD.lock().ok()?;
    let index = held.iter().position(|(path, _)| path == root)?;
    Some(held.swap_remove(index).1)
}
