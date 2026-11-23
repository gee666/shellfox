//! Takes over an Electron (or Node headless) root on first launch of a Rust
//! host laid out as `<root>/rust-v1/host`. Each item fills only an unused
//! Rust store; markers in the private control dir keep it to one pass. The
//! Electron files stay for rollback until `RETENTION` after the import.
//! SQLite only ever opens a private snapshot of Electron's database.
use anyhow::{Context, Result};
use cap_std::{ambient_authority, fs::Dir};
use serde::Serialize;
use shellfox_host::Host;
use shellfox_ssh::{KnownHosts, PrivateDirectory, ProfileStore, StoragePolicy};
use shellfox_transport::PermissionVerifier;
use std::{
    io::Write,
    path::{Component, Path},
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tempfile::NamedTempFile;

#[path = "electron_cleanup.rs"]
mod cleanup;
#[path = "electron_import_files.rs"]
mod files;
#[path = "electron_import_remote.rs"]
mod remote;
#[path = "electron_retention.rs"]
mod retention;
use retention::{
    cleanup, fingerprint, finish_without_electron, imported, remove_stale_snapshots, Imported,
};

/// Settings, SSH profiles and approvals.
const MARKER: &str = "electron-import-v1.json";
const WINDOW_MARKER: &str = "electron-window-import-v1.json";
/// Sessions, remote preferences and the Tailscale node, plus the import time.
const MARKER_V2: &str = "electron-import-v2.json";
const CLEANUP_MARKER: &str = "electron-cleanup-v1.json";
const SSH: &str = "ssh-profiles.json";
const KNOWN_HOSTS: &str = "ssh-known-hosts.json";
const RETENTION: Duration = Duration::from_secs(30 * 24 * 60 * 60);

/// Runs for every host whose data dir is `<root>/rust-v1/host`, importing from
/// `<root>`. Any other layout (tests, `--data-dir` fixtures) is left alone.
pub(super) async fn import_electron_root(
    data: &Path,
    control: &Path,
    verifier: PermissionVerifier,
    lost: shellfox_protocol::CancellationToken,
) -> Result<()> {
    let Some(source) = electron_root(data) else {
        return Ok(());
    };
    let (source, data, control) = (source.to_owned(), data.to_owned(), control.to_owned());
    let policy = StoragePolicy::default();
    #[cfg(windows)]
    let policy = {
        use shellfox_transport::PrivatePathKind;
        let runtime = tokio::runtime::Handle::current();
        let verifier = verifier.clone();
        policy.with_permission_check(move |path, directory, protect| {
            let kind = if directory {
                PrivatePathKind::Directory
            } else {
                PrivatePathKind::File
            };
            runtime
                .block_on(async {
                    if protect {
                        verifier.protect(path, kind).await
                    } else {
                        verifier.check(path, kind).await
                    }
                })
                .map_err(|_| shellfox_ssh::Error::PermissionsHelper)
        })
    };
    let runtime = tokio::runtime::Handle::current();
    tokio::task::spawn_blocking(move || {
        let check = || {
            anyhow::ensure!(!lost.is_cancelled(), "Host ownership ended");
            Ok(())
        };
        let node = Some((&verifier, &runtime));
        if let Some(report) = run(&source, &data, &control, policy, node, check)? {
            eprintln!("Electron import: {}", serde_json::to_string(&report)?);
        }
        Ok(())
    })
    .await?
}

fn electron_root(data: &Path) -> Option<&Path> {
    let mut tail = data.components().rev();
    let layout = tail.next() == Some(Component::Normal("host".as_ref()))
        && tail.next() == Some(Component::Normal("rust-v1".as_ref()));
    layout.then(|| data.ancestors().nth(2)).flatten()
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct Report {
    settings: bool,
    window_bounds: bool,
    sessions: usize,
    ssh_profiles: usize,
    ssh_host_keys: usize,
    approved: usize,
    // The live approval store represents revocation by absence. Keep the
    // rejected records here for reporting, never as live authority.
    revoked: Vec<Revoked>,
    remote_preferences: bool,
    node: Option<remote::Node>,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Revoked {
    peer_id: Option<String>,
    reason: &'static str,
}
type NodeAccess<'a> = Option<(&'a PermissionVerifier, &'a tokio::runtime::Handle)>;

fn run(
    source: &Path,
    data: &Path,
    control: &Path,
    policy: StoragePolicy,
    node: NodeAccess<'_>,
    check: impl Fn() -> Result<()>,
) -> Result<Option<Report>> {
    let root = source;
    let marked = |name: &str| control.join(name).symlink_metadata().is_ok();
    let (migrated, migrated_window) = (marked(MARKER), marked(WINDOW_MARKER));
    let imported = imported(control, &check)?;
    let pending = !migrated || !migrated_window || imported.is_none();
    let expired = imported.as_ref().is_some_and(|marker| {
        now_ms()
            >= marker
                .imported_at_ms
                .saturating_add(RETENTION.as_millis() as u64)
    });
    if !pending && (marked(CLEANUP_MARKER) || !expired) {
        return Ok(None);
    }
    remove_stale_snapshots(control);
    let parent = Dir::open_ambient_dir(
        source.parent().context("Electron parent missing")?,
        ambient_authority(),
    )?;
    let name = source.file_name().and_then(|name| name.to_str());
    let source = match files::open_dir(&parent, name.context("Electron root name")?) {
        Ok(source) => source.context("Electron root missing")?,
        Err(error) => {
            // A linked root is not taken over; Electron keeps using it.
            eprintln!("Electron import skipped: the data root is not a plain directory ({error}).");
            finish_without_electron(control, &check)?;
            return Ok(None);
        }
    };
    if !pending {
        let marker = imported.context("Import marker missing")?;
        return cleanup(&source, root, control, &marker, &check).map(|()| None);
    }
    let evidence = files::exists(&source, files::DATABASE)?
        || files::exists(&source, remote::REMOTE)?
        || files::exists(&source, ".host-lock")?;
    if !evidence {
        finish_without_electron(control, &check)?;
        return Ok(None);
    }
    let lock = files::electron_lock(&source)?;
    let marker = Imported {
        imported_at_ms: now_ms(),
        electron: true,
        present: cleanup::present(&source),
        fingerprint: fingerprint(&source)?,
    };
    let report = import(
        &source,
        data,
        control,
        policy,
        node,
        &check,
        [migrated, migrated_window, imported.is_some()],
        &marker,
    )?;
    cleanup::remove_caches(&source);
    if let Some(lock) = lock {
        retention::hold(root, lock);
    }
    Ok(Some(report))
}

#[allow(clippy::too_many_arguments)]
fn import(
    source: &Dir,
    data: &Path,
    control: &Path,
    policy: StoragePolicy,
    node: NodeAccess<'_>,
    check: &impl Fn() -> Result<()>,
    [migrated, migrated_window, migrated_v2]: [bool; 3],
    marker: &Imported,
) -> Result<Report> {
    let mut report = Report::default();
    // A changing database is retried on the next launch, before any marker.
    let database = files::database_snapshot(source, control)
        .context("Could not snapshot Electron settings")?;
    let database = database
        .as_ref()
        .map(|temp| temp.path().join(files::DATABASE));
    if !migrated_window {
        check()?;
        if let Some(snapshot) = &database {
            report.window_bounds = Host::open(data)?.import_electron_window_state(snapshot)?;
        }
        write_marker(control, WINDOW_MARKER, &report.window_bounds, check)?;
    }
    // Invalid documents are skipped for good; I/O errors retry on the next launch.
    let remote = files::read(source, remote::REMOTE, 4 * 1024 * 1024)
        .context("Could not read Electron remote.json")?
        .and_then(|bytes| {
            remote::RemoteFile::parse(&bytes)
                .inspect_err(|error| eprintln!("Electron import skipped remote.json: {error:#}"))
                .ok()
        });
    if let Some(snapshot) = database.as_ref().filter(|_| !migrated || !migrated_v2) {
        check()?;
        let host = Host::open(data)?;
        // Revision 1 is still unused only if the v1 settings import made it.
        let sessions_up_to =
            (!migrated_v2).then(|| u64::from(migrated && v1_copied_settings(control)));
        match host.import_electron(snapshot, !migrated, sessions_up_to) {
            Ok(imported) => {
                (report.settings, report.sessions) = (imported.settings, imported.sessions)
            }
            Err(error) if error.code == shellfox_core::ErrorCode::Validation => {
                // A session the Rust model rejects must not cost the settings.
                eprintln!("Electron sessions were not imported: {}", error.message);
                if !migrated {
                    report.settings = host.import_electron_settings(snapshot)?;
                }
            }
            Err(error) => return Err(error.into()),
        }
    }
    if !migrated {
        let ssh = files::read(source, SSH, 16 * 1024 * 1024)
            .context("Could not read Electron SSH profiles")?;
        let known = files::read(source, KNOWN_HOSTS, 4 * 1024 * 1024)
            .context("Could not read Electron SSH host keys")?;
        check()?;
        if ssh.is_some() || known.is_some() {
            let directory = Arc::new(PrivateDirectory::open(control.join("ssh"), policy)?);
            if let Some(bytes) = ssh.as_deref() {
                match ProfileStore::new(directory.clone()).import_electron(bytes) {
                    Ok(count) => report.ssh_profiles = count,
                    Err(error) => eprintln!("Electron SSH profiles were not imported: {error}"),
                }
            }
            if let Some(bytes) = known.as_deref() {
                match KnownHosts::new(directory).import_electron(bytes) {
                    Ok(count) => report.ssh_host_keys = count,
                    Err(error) => eprintln!("Electron SSH host keys were not imported: {error}"),
                }
            }
        }
        check()?;
        if let Some(file) = &remote {
            remote::import_approvals(&Host::open(data)?, file, &mut report)?;
        }
        write_marker(control, MARKER, &report, check)?;
    }
    if !migrated_v2 {
        let control_dir = Dir::open_ambient_dir(control, ambient_authority())?;
        check()?;
        // The node goes first: preferences that turn remote access on must not
        // start a fresh node before Electron's identity is in place.
        if let Some((verifier, runtime)) = node {
            report.node = Some(remote::adopt_node(
                source,
                data,
                &control_dir,
                verifier,
                runtime,
                check,
            )?);
        }
        if let Some(file) = &remote {
            report.remote_preferences = remote::import_preferences(&control_dir, file)?;
        }
        write_marker(control, MARKER_V2, marker, check)?;
    }
    Ok(report)
}

fn v1_copied_settings(control: &Path) -> bool {
    std::fs::read(control.join(MARKER))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        .is_some_and(|report| report["settings"] == true)
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |time| time.as_millis() as u64)
}

fn write_marker(
    control: &Path,
    name: &str,
    value: &impl Serialize,
    check: &impl Fn() -> Result<()>,
) -> Result<()> {
    let mut marker = NamedTempFile::new_in(control)?;
    serde_json::to_writer(&mut marker, value)?;
    marker.write_all(b"\n")?;
    marker.as_file().sync_all()?;
    check()?;
    marker.persist_noclobber(control.join(name))?;
    Ok(())
}

#[cfg(test)]
#[path = "electron_import_tests.rs"]
mod tests;
