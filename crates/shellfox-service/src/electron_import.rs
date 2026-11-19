//! First launch of the default Rust host. Never open the Electron database with SQLite.
use anyhow::{ensure, Context, Result};
use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt};
use cap_std::{
    ambient_authority,
    fs::{Dir, File, OpenOptions},
};
use serde::{Deserialize, Serialize};
use shellfox_core::ApprovalStoreRecord;
use shellfox_host::Host;
use shellfox_ssh::{PrivateDirectory, ProfileStore, StoragePolicy};
use std::{
    io::{self, Read, Write},
    path::Path,
    sync::Arc,
    time::SystemTime,
};
use tempfile::{NamedTempFile, TempDir};
use zeroize::Zeroizing;

const MARKER: &str = "electron-import-v1.json";
const WINDOW_MARKER: &str = "electron-window-import-v1.json";
const DATABASE: &str = "manager.sqlite3";
const WAL: &str = "manager.sqlite3-wal";
const JOURNAL: &str = "manager.sqlite3-journal";
const SSH: &str = "ssh-profiles.json";
const REMOTE: &str = "remote.json";
const DB_LIMIT: u64 = 256 * 1024 * 1024;

pub(super) async fn default_host(
    data: &Path,
    control: &Path,
    #[cfg(windows)] verifier: shellfox_transport::PermissionVerifier,
    lost: shellfox_protocol::CancellationToken,
) -> Result<()> {
    let Ok(default) = crate::app_paths::default_data_root() else {
        return Ok(());
    };
    // Original root spelling may have been normalized by the native owner.
    if !matches!((data.canonicalize(), default.canonicalize()), (Ok(a), Ok(b)) if a == b) {
        return Ok(());
    }
    let source = default
        .ancestors()
        .nth(2)
        .context("Electron data path missing")?
        .to_owned();
    let (data, control) = (data.to_owned(), control.to_owned());
    let policy = StoragePolicy::default();
    #[cfg(windows)]
    let policy = {
        use shellfox_transport::PrivatePathKind;
        let runtime = tokio::runtime::Handle::current();
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
    tokio::task::spawn_blocking(move || {
        let check = || {
            ensure!(!lost.is_cancelled(), "Host ownership ended");
            Ok(())
        };
        if let Some(report) = run(&source, &data, &control, policy, check)? {
            if report.window_bounds { eprintln!("Electron import: window bounds copied."); }
            if report.settings || report.ssh_profiles != 0 || report.approved != 0 || !report.revoked.is_empty() {
                eprintln!("Electron import: settings {}, {} SSH profiles, {} approved devices, {} revoked. Re-select the Rust host on your device after enrolling it.",
                    if report.settings { "copied" } else { "unchanged" }, report.ssh_profiles, report.approved, report.revoked.len());
            }
        }
        Ok(())
    }).await?
}

#[derive(Default, Serialize)]
struct Report {
    settings: bool,
    window_bounds: bool,
    ssh_profiles: usize,
    approved: usize,
    // The live approval store represents revocation by absence. Keep the
    // rejected records here for reporting, never as live authority.
    revoked: Vec<Revoked>,
}
#[derive(Serialize)]
struct Revoked {
    peer_id: Option<String>,
    reason: &'static str,
}
#[derive(Deserialize)]
struct RemoteFile {
    version: u8,
    approvals: Vec<serde_json::Value>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Approval {
    peer_id: String,
    hostname: String,
    approved_at: String,
}

fn run(
    source: &Path,
    data: &Path,
    control: &Path,
    policy: StoragePolicy,
    check: impl Fn() -> Result<()>,
) -> Result<Option<Report>> {
    let migrated = control.join(MARKER).symlink_metadata().is_ok();
    let migrated_window = control.join(WINDOW_MARKER).symlink_metadata().is_ok();
    if migrated && migrated_window { return Ok(None); }
    let parent = Dir::open_ambient_dir(
        source.parent().context("Electron parent missing")?,
        ambient_authority(),
    )?;
    let name = source.file_name().context("Electron data name missing")?;
    let source = match parent.open_dir_nofollow(name) {
        Ok(source) => Some(source),
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        Err(e) => return Err(e.into()),
    };
    let mut report = Report::default();
    if let Some(source) = source {
        let database =
            database_snapshot(&source, control).context("Could not snapshot Electron settings")?;
        if !migrated_window {
            check()?;
            if let Some(snapshot) = &database {
                report.window_bounds = Host::open(data)?.import_electron_window_state(&snapshot.path().join(DATABASE))?;
            }
            write_marker(control, WINDOW_MARKER, &report.window_bounds, &check)?;
        }
        if migrated { return Ok(Some(report)); }
        let ssh = Zeroizing::new(
            read(&source, SSH, 16 * 1024 * 1024).context("Could not read Electron SSH profiles")?,
        );
        let remote =
            read(&source, REMOTE, 4 * 1024 * 1024).context("Could not read Electron approvals")?;
        check()?;
        if let Some(database) = database {
            report.settings =
                Host::open(data)?.import_electron_settings(&database.path().join(DATABASE))?;
        }
        check()?;
        if let Some(bytes) = ssh.as_deref() {
            let profiles = ProfileStore::new(Arc::new(PrivateDirectory::open(
                control.join("ssh"),
                policy,
            )?));
            report.ssh_profiles = profiles
                .import_electron(bytes)
                .context("Could not import Electron SSH profiles")?;
        }
        check()?;
        if let Some(bytes) = remote {
            import_approvals(&Host::open(data)?, &bytes, &mut report)?;
        }
    }
    if !migrated_window && !control.join(WINDOW_MARKER).exists() {
        write_marker(control, WINDOW_MARKER, &false, &check)?;
    }
    if !migrated { write_marker(control, MARKER, &report, &check)?; }
    Ok(Some(report))
}

fn write_marker(control: &Path, name: &str, value: &impl Serialize, check: &impl Fn() -> Result<()>) -> Result<()> {
    let mut marker = NamedTempFile::new_in(control)?;
    serde_json::to_writer(&mut marker, value)?;
    marker.write_all(b"\n")?;
    marker.as_file().sync_all()?;
    check()?;
    marker.persist_noclobber(control.join(name))?;
    Ok(())
}

fn import_approvals(host: &Host, bytes: &[u8], report: &mut Report) -> Result<()> {
    let file: RemoteFile =
        serde_json::from_slice(bytes).context("Invalid Electron approval document")?;
    ensure!(
        file.version == 1 && file.approvals.len() <= shellfox_core::APPROVAL_RECORD_LIMIT,
        "Unsupported Electron approval document"
    );
    let current = host.approval_snapshot()?;
    let mut approvals = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for value in file.approvals {
        let identity = value
            .get("peerId")
            .and_then(serde_json::Value::as_str)
            .filter(|id| valid_peer_id(id))
            .map(str::to_owned);
        let record = serde_json::from_value::<Approval>(value)
            .ok()
            .and_then(|approval| {
                let time = chrono::DateTime::parse_from_rfc3339(&approval.approved_at).ok()?;
                let record = ApprovalStoreRecord {
                    peer_id: approval.peer_id,
                    hostname: approval.hostname,
                    approved_at_millis: time.timestamp_millis().try_into().ok()?,
                    policy: shellfox_core::ApprovalStorePolicy::ControlFullAccountV1,
                };
                record.validate().ok()?;
                Some(record)
            });
        match record {
            Some(record) if seen.insert(record.peer_id.clone()) => approvals.push(record),
            _ => report.revoked.push(Revoked {
                peer_id: identity,
                reason: "Incompatible Electron approval",
            }),
        }
    }
    // If any row for an ID was ambiguous or incompatible, don't authorize it.
    approvals.retain(|record| {
        !report
            .revoked
            .iter()
            .any(|revoked| revoked.peer_id.as_deref() == Some(&record.peer_id))
    });
    if current.revision == 0 && !approvals.is_empty() {
        report.approved = host
            .commit_approvals(current.revision, approvals)?
            .approvals
            .len();
    } else {
        for record in approvals {
            if current.approvals.contains(&record) {
                report.approved += 1;
            } else {
                report.revoked.push(Revoked {
                    peer_id: Some(record.peer_id),
                    reason: "Rust approval store already in use",
                });
            }
        }
    }
    Ok(())
}
fn valid_peer_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 200
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_.:-".contains(&b))
}

fn open(dir: &Dir, name: &str) -> Result<Option<File>> {
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
fn read(dir: &Dir, name: &str, limit: u64) -> Result<Option<Vec<u8>>> {
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

fn stamp(file: &File) -> io::Result<(u64, SystemTime)> {
    let info = file.metadata()?;
    Ok((info.len(), info.modified()?.into_std()))
}
fn database_snapshot(source: &Dir, control: &Path) -> Result<Option<TempDir>> {
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
    let copy = |name, input: &mut File, size| -> Result<()> {
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

#[cfg(test)]
#[path = "electron_import_tests.rs"]
mod tests;
