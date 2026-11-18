//! Experimental headless host. No GPUI, display, or production store defaults.
pub mod activity;
mod activity_hooks;
pub mod activity_integration;
pub mod automation;
mod creation;
pub mod desktop_launch;
pub mod experiment;
mod facts_capture;
mod facts_membership;
pub mod facts_projection;
mod grant_custody;
mod grant_requests;
mod grant_retirement;
mod launch;
pub mod local;
pub mod native;
pub mod native_providers;
mod native_endpoint_report;
pub use native_endpoint_report::{
    EndpointArtifactDisposition, EndpointPublicationOutcome, NativeEndpointReport,
};
mod native_catalog;
pub use native_catalog::{NativeLaunchCatalog, NativePythonFact};
mod native_sharing;
pub use native_sharing::NativeFactoryPolicy;
mod mailbox_custody;
mod manager_delivery;
mod manager_mailboxes;
mod manager_retire;
#[cfg(test)]
#[path = "../../shellfox-remote/tests/support/native_metadata.rs"]
mod native_metadata_fixture;
mod owner;
mod owner_automation;
mod owner_ssh;
#[cfg(windows)]
mod pipe_identity;
mod platform;
pub mod power;
mod power_client;
mod power_service;
pub mod remote;
pub mod remote_v2;
mod remote_v2_retire;
mod security;
mod service_activity;
mod service_open;
pub mod settings_runtime;
pub mod ssh;
#[cfg(test)]
mod stop_admission_tests;
pub mod system;
mod terminals;
#[cfg(test)]
mod tests;

use anyhow::{bail, Context, Result};
use shellfox_core::{PreparedClose, Snapshot, TabId, TerminalIdentity};
use shellfox_host::Host;
use shellfox_runtime::Phase;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

pub use activity_hooks::{ActivityShutdownState, ShutdownFailure, ShutdownOutcome};
pub use native::NativeAdmissionError;
pub use owner::{ExperimentAccepted, NativeAccepted, OwnerClient};
pub use owner_automation::{
    AutomationViewFailure, PresentationShutdownState, PresentationWarnings, AUTOMATION_PROFILE_ID,
    AUTOMATION_READONLY_PROFILE,
};
pub use terminals::{Attachment, TerminalOutput};

/// Trusted endpoint adapters consume at most one bounded queued request per tick.
/// Implementations must recheck their authenticated approval epoch here, on the
/// owner thread, before calling any metadata/runtime operation.
pub trait OwnerDispatcher: Send {
    fn dispatch_one(&mut self, service: &mut Service);
}
impl OwnerDispatcher for () {
    fn dispatch_one(&mut self, _service: &mut Service) {}
}

/// Construct only after the local endpoint adapter has proved private ownership.
/// One event loop owns this value and all terminal handles.
pub struct Service {
    host: Host,
    terminals: HashMap<TabId, terminals::OwnedTerminal>,
    isolation: PathBuf,
    supervisor: PathBuf,
    next_request: u64,
    grants: grant_custody::GrantCustody,
    grant_recipient: shellfox_protocol::CancellationToken,
    next_viewer_id: u64,
    remote_v2_mailboxes: remote_v2::mailboxes::ObserverMailboxes,
    manager_v2_mailboxes: manager_mailboxes::ManagerMailboxes,
    native: native::NativeState,
    catalog: native_catalog::NativeCatalogState,
    power: power_service::ServicePower,
    factory_policy: native_sharing::FactoryPolicyState,
    activity: service_activity::ServiceActivity,
    ownership_lost: shellfox_protocol::CancellationToken,
    stopping: bool,
    metadata_closing: bool,
    automation: owner_automation::AutomationState,
    ssh: crate::ssh::NativeSshOwner,
    system_native_handler: Option<std::sync::Arc<system::SystemNativeHandler>>,
    // Last: Host/SQLite and every deferred resource owner drop before this guard.
    // Original directory lifetime only, not path/VFS confinement.
    _root_lifetime: Option<shellfox_transport::RootLifetimeGuard>,
}
impl Service {
    pub(crate) fn host(&self) -> &Host {
        &self.host
    }
    fn during_metadata_close<T>(
        &mut self,
        operation: impl FnOnce(&mut Self) -> Result<T>,
    ) -> Result<T> {
        if self.metadata_closing {
            bail!("Metadata cleanup is already active.");
        }
        self.metadata_closing = true;
        let result = operation(self);
        self.metadata_closing = false;
        result
    }
    pub fn close_tab(&mut self, id: TabId) -> Result<Snapshot> {
        self.assert_owned()?;
        self.during_metadata_close(|service| {
            let prepared = service.host.prepare_close_tab(id)?;
            if let Err(error) = service.close_external_for_tab(id) {
                service.host.cancel_close(prepared.token)?;
                return Err(error);
            }
            let snapshot = service.finish_close(prepared, &[id])?;
            service.forget_closed_external(id);
            Ok(snapshot)
        })
    }
    pub fn delete_session(&mut self, id: shellfox_core::SessionId) -> Result<Snapshot> {
        self.assert_owned()?;
        self.during_metadata_close(|service| {
            let tabs: Vec<_> = service
                .host
                .snapshot()?
                .sessions
                .iter()
                .find(|session| session.id == id)
                .map(|session| session.tabs.iter().map(|tab| tab.id).collect())
                .unwrap_or_default();
            let prepared = service.host.prepare_delete_session(id)?;
            for tab in &tabs {
                if let Err(error) = service.close_external_for_tab(*tab) {
                    service.host.cancel_close(prepared.token)?;
                    return Err(error);
                }
            }
            let snapshot = service.finish_close(prepared, &tabs)?;
            for tab in tabs {
                service.forget_closed_external(tab);
            }
            service.activity.persistence.forget_session(id);
            Ok(snapshot)
        })
    }
    fn finish_close(&mut self, prepared: PreparedClose, targets: &[TabId]) -> Result<Snapshot> {
        for identity in &prepared.terminals {
            if let Err(error) = self.stop_terminal(*identity) {
                self.host.cancel_close(prepared.token)?;
                return Err(error);
            }
        }
        if let Err(error) = self.close_ssh_confirmed(targets) {
            self.host.cancel_close(prepared.token)?;
            return Err(error);
        }
        // stop_terminal removes each generation only after Runtime phase Closed.
        // Native loss while waiting must never permit the metadata commit.
        if let Err(error) = self.assert_owned() {
            self.host.cancel_close(prepared.token)?;
            return Err(error);
        }
        match self.host.commit_close(prepared.token) {
            Ok(snapshot) => Ok(snapshot),
            Err(error) => {
                self.host.cancel_close(prepared.token)?;
                Err(error.into())
            }
        }
    }
    fn stop_terminal(&mut self, identity: TerminalIdentity) -> Result<()> {
        let runtime = &self
            .terminals
            .get(&identity.tab_id)
            .context("Registered terminal owner is missing; cleanup cannot be confirmed.")?
            .runtime;
        if runtime.identity().generation != identity.generation {
            bail!("Terminal generation changed.");
        }
        if runtime.source_kind() == shellfox_runtime::SourceKind::ExternalBytes {
            return self.close_external_parser(identity);
        }
        let deadline = Instant::now() + Duration::from_secs(12);
        if runtime.phase() != Phase::Closed {
            if let Some(terminal) = self.terminals.get_mut(&identity.tab_id) {
                terminal.discard_output();
            }
            self.request_close(identity.tab_id)?;
        }
        loop {
            if let Some(terminal) = self.terminals.get_mut(&identity.tab_id) {
                terminal.discard_output();
            }
            self.drain(identity.tab_id)?;
            let terminal = self
                .terminals
                .get(&identity.tab_id)
                .context("Terminal owner disappeared.")?;
            if terminal.runtime.phase() == Phase::Closed {
                break;
            }
            if Instant::now() >= deadline {
                bail!("Owned terminal cleanup not confirmed; metadata retained.");
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        let runtime_identity = self.terminals[&identity.tab_id].runtime.identity();
        self.host.terminal_stopped(identity)?;
        self.record_ssh_closed(runtime_identity);
        self.activity_collect_live();
        self.activity_confirmed_exit(runtime_identity);
        self.forget_activity(runtime_identity);
        self.terminals.remove(&identity.tab_id);
        self.ownership_source_destroyed(runtime_identity);
        Ok(())
    }
    /// Must succeed before the endpoint disappears or metadata ownership releases.
    /// An error retains every unconfirmed runtime and the database lock for retry.
    pub fn shutdown(&mut self) -> std::result::Result<ShutdownOutcome, ShutdownFailure> {
        // Fence admission before the first resource cleanup. A failed/uncertain
        // shutdown may retry cleanup, but can never reopen launch admission.
        self.stopping = true;
        self.begin_stop_power();
        self.begin_stop_native_catalog();
        self.begin_stop_system_native();
        self.disable_observer_factory_policy()
            .map_err(ShutdownFailure::ResourceCleanup)?;
        self.begin_stop_ssh_native();
        self.stop_automation()
            .map_err(ShutdownFailure::ResourceCleanup)?;
        self.stop_ssh_confirmed()
            .map_err(ShutdownFailure::ResourceCleanup)?;
        let identities: Vec<_> = self
            .terminals
            .values()
            .map(|terminal| {
                let id = terminal.runtime.identity();
                TerminalIdentity {
                    tab_id: id.tab_id,
                    generation: id.generation,
                }
            })
            .collect();
        for identity in identities {
            self.stop_terminal(identity)
                .map_err(ShutdownFailure::ResourceCleanup)?;
        }
        self.stop_native_catalog()
            .map_err(ShutdownFailure::ResourceCleanup)?;
        self.stop_power().map_err(ShutdownFailure::ResourceCleanup)?;
        self.activity_collect_live();
        let pending_sessions = self.activity.persistence.pending_count();
        let activity = if self.ownership_lost.is_cancelled() && pending_sessions > 0 {
            ActivityShutdownState::DirtyAfterOwnershipLoss { pending_sessions }
        } else if self.ownership_lost.is_cancelled() {
            ActivityShutdownState::Committed
        } else {
            self.flush_activity(true)
                .map_err(ShutdownFailure::ActivityPersistence)?;
            ActivityShutdownState::Committed
        };
        let presentation = self.presentation_shutdown_state();
        Ok(ShutdownOutcome {
            activity,
            presentation,
        })
    }
    pub fn active_terminals(&self) -> usize {
        self.terminals.len()
    }
    fn assert_accepting(&self) -> Result<()> {
        self.assert_owned()?;
        if self.stopping {
            bail!("The host is shutting down; resource admission is closed.");
        }
        Ok(())
    }
    fn assert_owned(&self) -> Result<()> {
        if self.ownership_lost.is_cancelled() {
            bail!("Native host ownership lost.");
        }
        Ok(())
    }
    pub(crate) fn allocate_viewer_id(&mut self) -> Result<u64> {
        self.next_viewer_id = self
            .next_viewer_id
            .checked_add(1)
            .filter(|value| *value <= shellfox_protocol::MAX_SAFE_INTEGER)
            .context("Owner viewer/controller IDs exhausted.")?;
        Ok(self.next_viewer_id)
    }
    fn request_id(&mut self) -> Result<shellfox_runtime::RequestId> {
        self.next_request = self
            .next_request
            .checked_add(1)
            .context("Runtime request counter exhausted.")?;
        Ok(shellfox_runtime::RequestId(self.next_request))
    }
}
