//! Host-only SSH module. One metadata Host and one Runtime registry remain owner-owned.
#[path = "ssh_admission.rs"]
mod admission;
#[path = "ssh_api.rs"]
mod api;
#[path = "ssh_broker.rs"]
mod broker;
#[path = "ssh_diagnostics.rs"]
mod diagnostics;
#[path = "ssh_dispatch.rs"]
mod dispatch;
#[path = "ssh_entry.rs"]
mod entry;
#[path = "ssh_lifecycle.rs"]
mod lifecycle;
#[path = "ssh_native.rs"]
mod native;
#[path = "ssh_open.rs"]
mod open;
#[path = "ssh_poll.rs"]
mod poll;
#[path = "ssh_presentation_target.rs"]
mod presentation_target;
#[path = "ssh_program_env.rs"]
mod program_env;
#[path = "ssh_runtime.rs"]
mod runtime;
#[path = "ssh_tap.rs"]
mod tap;
#[cfg(test)]
#[path = "ssh_tests.rs"]
mod tests;
#[path = "ssh_work.rs"]
mod work;
use anyhow::Result;
pub use api::*;
pub use broker::{askpass_entry, askpass_from_environment, has_askpass_environment};
pub use entry::dispatch_early_entry;
pub use native::{NativeSshInstall, NativeSshOwner, SshProgramPolicy, SshProgramRole};
pub use open::{OpenProfile, OpenedProfile};
use shellfox_protocol::CancellationToken;
pub use shellfox_ssh::key_metadata::{KeyFormat, KeyMetadata};
use shellfox_ssh::{
    keys::PreparedIdentity,
    pty::RemotePty,
    system::{AuthenticationProof, SystemSshAdapter},
    PrivateDirectory, ProfileStore,
};
pub use shellfox_ssh::{Secret, TrustDecision};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        mpsc::{self, Receiver, SyncSender},
        Arc,
    },
    thread::JoinHandle,
};
use uuid::Uuid;
use work::{Work, WorkEvent};

pub struct SshManager {
    directory: Arc<PrivateDirectory>,
    profiles: ProfileStore,
    backend: Arc<SystemSshAdapter>,
    platform: Arc<dyn NativePlatform>,
    askpass: PathBuf,
    lost: CancellationToken,
    stopping: bool,
    events: SyncSender<WorkEvent>,
    incoming: Receiver<WorkEvent>,
    connections: HashMap<Uuid, Connection>,
}
struct Connection {
    controller: u64,
    status: ConnectionStatus,
    cancel: CancellationToken,
    auth_cancel: CancellationToken,
    authenticated: bool,
    view_attached: bool,
    target: SourceConnectTarget,
    reserved: bool,
    job: Option<JoinHandle<()>>,
    pending: Option<Pending>,
    runtime: Option<shellfox_runtime::Identity>,
    broker: Option<broker::Broker>,
    identity: Option<PreparedIdentity>,
    proof: Option<AuthenticationProof>,
}
enum Pending {
    Host(Uuid, SyncSender<TrustDecision>),
    Prompt(Uuid, SyncSender<Option<Secret>>),
}
impl SshManager {
    /// ModuleManager supplies the SAME project's stable rust-service-v1/ssh private
    /// directory and verified resource providers. This never opens another Host.
    pub fn new(
        directory: Arc<PrivateDirectory>,
        backend: Arc<SystemSshAdapter>,
        platform: Arc<dyn NativePlatform>,
        askpass: PathBuf,
        lost: CancellationToken,
    ) -> Result<Self> {
        anyhow::ensure!(
            askpass.is_absolute() && askpass.is_file(),
            "A trusted askpass executable is required."
        );
        anyhow::ensure!(
            backend.uses_directory(&directory),
            "SSH stores and adapter must share one private directory capability."
        );
        let (events, incoming) = mpsc::sync_channel(128);
        Ok(Self {
            profiles: ProfileStore::new(directory.clone()),
            directory,
            backend,
            platform,
            askpass,
            lost,
            stopping: false,
            events,
            incoming,
            connections: HashMap::new(),
        })
    }
    fn statuses(&self, controller: u64, id: Option<Uuid>) -> Vec<ConnectionStatus> {
        let mut statuses: Vec<_> = self
            .connections
            .values()
            .filter(|c| c.controller == controller && id.is_none_or(|id| c.status.connection == id))
            .map(|c| c.status.clone())
            .collect();
        statuses.sort_by_key(|s| s.connection);
        statuses
    }
    fn owned(&mut self, controller: u64, id: Uuid) -> Result<&mut Connection> {
        self.connections
            .get_mut(&id)
            .filter(|c| c.controller == controller)
            .ok_or_else(|| anyhow::anyhow!("SSH connection is unavailable to this controller."))
    }
    fn check(
        &self,
        controller: u64,
        tab: Option<shellfox_core::TabId>,
        validator: &dyn NativeControllerValidator,
    ) -> Result<()> {
        anyhow::ensure!(!self.lost.is_cancelled(), "Native host ownership was lost.");
        anyhow::ensure!(!self.stopping, "SSH module is stopping.");
        anyhow::ensure!(controller > 0, "Invalid native controller.");
        validator.validate_native(controller)?;
        if let Some(tab) = tab {
            validator.validate_target(controller, tab)?;
        }
        Ok(())
    }
    pub fn begin_shutdown(&mut self) {
        self.stopping = true;
        for c in self.connections.values_mut() {
            if !matches!(c.status.state, State::Closed) {
                c.stop();
            }
        }
    }
    /// Only true after worker, broker, encrypted identity and original runtime are closed.
    pub fn is_closed(&self) -> bool {
        self.connections.values().all(|c| {
            c.runtime.is_none()
                && c.job.is_none()
                && c.broker.is_none()
                && c.identity.is_none()
                && !c.reserved
        })
    }
    /// Feed ONLY bytes from the existing runtime event stream. A local PTY start or
    /// arbitrary UI "ready" message is not authentication proof.
    pub fn observe_output(&mut self, identity: shellfox_runtime::Identity, bytes: &[u8]) {
        for c in self
            .connections
            .values_mut()
            .filter(|c| c.runtime == Some(identity))
        {
            if !c.authenticated
                && !c.cancel.is_cancelled()
                && c.proof.as_mut().is_some_and(|p| p.observe(bytes))
            {
                c.authenticated = true;
                c.status.state = State::Ready;
            }
        }
    }
    /// Only the native live-output tap can supply this non-wire proof receipt.
    pub(super) fn accept_live_confirmation(
        &mut self,
        identity: shellfox_runtime::Identity,
        proof: &AuthenticationProof,
    ) {
        for c in self
            .connections
            .values_mut()
            .filter(|c| c.runtime == Some(identity) && !c.authenticated && !c.cancel.is_cancelled())
        {
            if c.proof
                .as_mut()
                .is_some_and(|expected| expected.accept_confirmation(proof))
            {
                c.authenticated = true;
                c.status.state = State::Ready;
            }
        }
    }
    /// Owner calls this only for its matching generation after confirmed tree cleanup.
    pub fn runtime_closed(&mut self, identity: shellfox_runtime::Identity) {
        for c in self
            .connections
            .values_mut()
            .filter(|c| c.runtime == Some(identity))
        {
            c.runtime = None;
            let incomplete = !c.cancel.is_cancelled() && !c.authenticated;
            c.cancel.cancel();
            c.auth_cancel.cancel();
            c.status.state = if incomplete {
                State::Failed {
                    message: "SSH closed before authentication was confirmed.".into(),
                }
            } else {
                State::Closing
            };
        }
    }
}
// An accidentally dropped manager must not release credentials/private identity ownership
// for a still-live runtime. Normal Service shutdown polls cleanup instead of using this hold.
struct Retained {
    _directory: Arc<PrivateDirectory>,
    _connections: HashMap<Uuid, Connection>,
}
static UNCONFIRMED: std::sync::OnceLock<std::sync::Mutex<Vec<Retained>>> =
    std::sync::OnceLock::new();
impl Drop for SshManager {
    fn drop(&mut self) {
        if !self.is_closed() {
            self.begin_shutdown();
            UNCONFIRMED
                .get_or_init(Default::default)
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(Retained {
                    _directory: self.directory.clone(),
                    _connections: std::mem::take(&mut self.connections),
                });
        }
    }
}
