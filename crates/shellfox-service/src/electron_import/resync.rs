//! Electron used again after the import (a downgrade to 0.9 and back): its
//! later sessions, settings, SSH profiles and host keys are merged on the next
//! launch. Approvals are not: that would re-grant remote access revoked here.
use super::*;
use retention::Fingerprint;
use shellfox_host::ElectronSettingsDigest;

const SYNC: &str = "electron-sync-v1.json";

/// What this host last took from Electron.
#[derive(Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Synced {
    pub(super) synced_at_ms: u64,
    pub(super) fingerprint: Fingerprint,
    /// None until a sync has seen Electron's settings.
    pub(super) settings: Option<ElectronSettingsDigest>,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct Merged {
    sessions_added: usize,
    sessions_updated: usize,
    settings: Vec<String>,
    ssh_profiles: usize,
    ssh_host_keys: usize,
}

/// Electron's settings at import time, the baseline for later merges.
pub(super) fn record_import(
    data: &Path,
    control: &Path,
    snapshot: Option<&Path>,
    marker: &Imported,
    check: &impl Fn() -> Result<()>,
) -> Result<()> {
    let settings = match snapshot {
        // Nothing is newer than u64::MAX, so this only reads the digest.
        Some(path) => Some(
            Host::open(data)?
                .merge_electron(path, u64::MAX, None)?
                .digest,
        ),
        None => None,
    };
    let synced = Synced {
        synced_at_ms: marker.imported_at_ms,
        fingerprint: marker.fingerprint.clone(),
        settings,
    };
    replace_marker(control, &synced, check)
}

/// Merges Electron's changes if its files changed since the import or the last merge.
pub(super) fn resync(
    source: &Dir,
    root: &Path,
    data: &Path,
    control: &Path,
    policy: StoragePolicy,
    marker: &Imported,
    check: &impl Fn() -> Result<()>,
) -> Result<()> {
    let last = match std::fs::read(control.join(SYNC)) {
        Ok(bytes) => serde_json::from_slice(&bytes).context("Invalid Electron sync marker")?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Synced {
            synced_at_ms: marker.imported_at_ms,
            fingerprint: marker.fingerprint.clone(),
            settings: None,
        },
        Err(error) => return Err(error.into()),
    };
    if fingerprint(source)? == last.fingerprint {
        return Ok(());
    }
    // Held until the merge is recorded, so Electron can't change its files meanwhile.
    let _lock = match retention::held(root) {
        true => None,
        false => files::electron_lock(source)?,
    };
    let (started, current) = (now_ms(), fingerprint(source)?);
    let mut merged = Merged::default();
    let mut settings = last.settings.clone();
    if let Some(snapshot) = files::database_snapshot(source, control)? {
        check()?;
        let path = snapshot.path().join(files::DATABASE);
        let merge =
            Host::open(data)?.merge_electron(&path, last.synced_at_ms, last.settings.as_ref())?;
        (
            merged.sessions_added,
            merged.sessions_updated,
            merged.settings,
        ) = (merge.added, merge.updated, merge.settings);
        settings = Some(merge.digest);
    }
    // Both stores only add what they lack: saved profiles and keys are never replaced.
    // The profiles only when their file changed, so deleted ones don't return each time.
    let stamp = |print: &Fingerprint| print.iter().find(|(name, _)| name == SSH).cloned();
    let ssh = match stamp(&current) != stamp(&last.fingerprint) {
        true => files::read(source, SSH, 16 * 1024 * 1024)?,
        false => None,
    };
    let known = files::read(source, KNOWN_HOSTS, 4 * 1024 * 1024)?;
    if ssh.is_some() || known.is_some() {
        check()?;
        let directory = Arc::new(PrivateDirectory::open(control.join("ssh"), policy)?);
        if let Some(bytes) = ssh.as_deref() {
            match ProfileStore::new(directory.clone()).import_electron(bytes) {
                Ok(count) => merged.ssh_profiles = count,
                Err(error) => eprintln!("Electron SSH profiles were not merged: {error}"),
            }
        }
        if let Some(bytes) = known.as_deref() {
            match KnownHosts::new(directory).import_electron(bytes) {
                Ok(count) => merged.ssh_host_keys = count,
                Err(error) => eprintln!("Electron SSH host keys were not merged: {error}"),
            }
        }
    }
    let synced = Synced {
        synced_at_ms: started,
        fingerprint: current,
        settings,
    };
    replace_marker(control, &synced, check)?;
    eprintln!(
        "Electron changes merged: {}",
        serde_json::to_string(&merged)?
    );
    Ok(())
}

fn replace_marker(control: &Path, value: &Synced, check: &impl Fn() -> Result<()>) -> Result<()> {
    let mut marker = NamedTempFile::new_in(control)?;
    serde_json::to_writer(&mut marker, value)?;
    marker.write_all(b"\n")?;
    marker.as_file().sync_all()?;
    check()?;
    marker.persist(control.join(SYNC))?;
    Ok(())
}
