//! Remote access from Electron's remote.json and its Tailscale node state.
use super::files::{exists, open_dir, publish_private, read};
use super::{Report, Revoked};
use crate::remote::preferences::{RememberedPeer, RemotePreferences};
use anyhow::{ensure, Context, Result};
use cap_std::fs::Dir;
use serde::{Deserialize, Serialize};
use shellfox_core::ApprovalStoreRecord;
use shellfox_host::Host;
use shellfox_transport::{IsolatedStateDirectory, PermissionVerifier};
use std::{collections::BTreeMap, path::Path};

pub(super) const REMOTE: &str = "remote.json";
const PREFERENCES: &str = "remote-settings-v1.json";
const NETWORK: &str = "go-network";
const NODE_STATE: &str = "tailscaled.state";
// Real node states are a few KiB; this is the private-publication ceiling.
const NODE_STATE_LIMIT: u64 = shellfox_files::private::MAX_BYTES as u64;

#[derive(Deserialize)]
pub(super) struct RemoteFile {
    version: u8,
    #[serde(default)]
    enabled: bool,
    hostname: Option<String>,
    approvals: Vec<serde_json::Value>,
    #[serde(default)]
    peers: Vec<serde_json::Value>,
}
impl RemoteFile {
    pub(super) fn parse(bytes: &[u8]) -> Result<Self> {
        let file: Self = serde_json::from_slice(bytes).context("Invalid Electron remote.json")?;
        ensure!(
            file.version == 1 && file.approvals.len() <= shellfox_core::APPROVAL_RECORD_LIMIT,
            "Unsupported Electron remote.json"
        );
        Ok(file)
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Approval {
    peer_id: String,
    hostname: String,
    approved_at: String,
}
#[derive(Deserialize)]
struct Peer {
    id: String,
    hostname: String,
    addresses: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) enum Node {
    /// Electron never enrolled a node, or left no readable state.
    Absent,
    Adopted,
    /// The Rust leaf already has a node identity of its own.
    RustHasState,
}

pub(super) fn import_approvals(host: &Host, file: &RemoteFile, report: &mut Report) -> Result<()> {
    let current = host.approval_snapshot()?;
    let mut approvals = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for value in &file.approvals {
        let identity = value
            .get("peerId")
            .and_then(serde_json::Value::as_str)
            .filter(|id| valid_peer_id(id))
            .map(str::to_owned);
        let record = serde_json::from_value::<Approval>(value.clone())
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

/// Carry the node name, the on/off switch and remembered hosts into a Rust
/// store that has none yet.
pub(super) fn import_preferences(control: &Dir, file: &RemoteFile) -> Result<bool> {
    let Some(hostname) = file.hostname.clone() else {
        return Ok(false);
    };
    if exists(control, PREFERENCES)? {
        return Ok(false);
    }
    let peers = file
        .peers
        .iter()
        .filter_map(|value| serde_json::from_value::<Peer>(value.clone()).ok())
        .map(|peer| RememberedPeer {
            id: peer.id,
            hostname: peer.hostname,
            addresses: peer.addresses,
        })
        .collect();
    let mut preferences = RemotePreferences {
        version: 1,
        enabled: file.enabled,
        hostname,
        peers,
    };
    if preferences.validate().is_err() {
        // A malformed remembered host must not cost the node name and switch.
        preferences.peers.clear();
        if preferences.validate().is_err() {
            return Ok(false);
        }
    }
    publish_private(
        control,
        PREFERENCES,
        &serde_json::to_vec(&preferences)?,
        None,
    )?;
    Ok(true)
}

/// Copy Electron's node state into the default Rust leaf unless that leaf
/// already has an identity. Electron's copy stays in place for rollback.
pub(super) fn adopt_node(
    source: &Dir,
    data: &Path,
    control: &Dir,
    verifier: &PermissionVerifier,
    runtime: &tokio::runtime::Handle,
    check: &impl Fn() -> Result<()>,
) -> Result<Node> {
    let Some(electron) = open_dir(source, "tailscale")? else {
        return Ok(Node::Absent);
    };
    let Some(bytes) = read(&electron, NODE_STATE, NODE_STATE_LIMIT)? else {
        return Ok(Node::Absent);
    };
    if !has_identity(&bytes) {
        return Ok(Node::Absent);
    }
    let leaf_path = crate::app_paths::default_remote_state(data);
    let leaf = leaf_path
        .file_name()
        .and_then(|name| name.to_str())
        .context("Remote state leaf missing")?;
    let network = match open_dir(control, NETWORK)? {
        Some(network) => network,
        None => {
            let parent = control.try_clone()?.into_std_file();
            shellfox_files::private::create_dir(&parent, NETWORK.as_ref())
                .map_err(|error| anyhow::anyhow!("Could not create {NETWORK}: {error}"))?;
            open_dir(control, NETWORK)?.context("Network directory vanished")?
        }
    };
    check()?;
    // The marker proves this leaf is the Rust one at this exact path.
    let _proof = runtime
        .block_on(async {
            if exists(&network, leaf)? {
                IsolatedStateDirectory::open_existing(leaf_path.clone(), verifier).await
            } else {
                IsolatedStateDirectory::create_new_private(leaf_path.clone(), verifier).await
            }
            .map_err(|error| anyhow::anyhow!("Rust node directory refused: {error:?}"))
        })
        .context("Could not prepare the Rust node directory")?;
    let leaf = open_dir(&network, leaf)?.context("Rust node directory vanished")?;
    let expected = match read(&leaf, NODE_STATE, NODE_STATE_LIMIT) {
        Ok(None) => None,
        Ok(Some(current)) if !has_identity(&current) && is_object(&current) => {
            // The helper writes `{}` before enrollment. Replace only that exact version.
            let parent = leaf.try_clone()?.into_std_file();
            match shellfox_files::private::read(
                &parent,
                NODE_STATE.as_ref(),
                NODE_STATE_LIMIT as usize,
            ) {
                Ok(read) if read.bytes == *current => Some(read.version),
                _ => return Ok(Node::RustHasState),
            }
        }
        _ => return Ok(Node::RustHasState),
    };
    check()?;
    publish_private(&leaf, NODE_STATE, &bytes, expected.as_ref())?;
    Ok(Node::Adopted)
}

/// The helper's store is a JSON object of base64 values. Any key means tsnet
/// has written a machine key or profile.
fn has_identity(bytes: &[u8]) -> bool {
    serde_json::from_slice::<BTreeMap<String, String>>(bytes).is_ok_and(|map| !map.is_empty())
}
fn is_object(bytes: &[u8]) -> bool {
    serde_json::from_slice::<BTreeMap<String, String>>(bytes).is_ok()
}
