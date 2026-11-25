//! Headless Shellfox host: terminals, metadata, remote access and the local API. No GPUI.
pub mod activity;
pub mod app_composition;
pub mod app_paths;
pub mod automation;
mod close;
pub mod crash_report;
mod creation;
pub mod desktop_launch;
pub mod desktop_session;
mod electron_import;
pub mod facts;
mod grant;
pub mod headless_layout;
mod launch;
pub mod local;
pub mod local_api;
pub mod native;
pub use activity::native::NativeTabActivity;
pub mod native_providers;
pub use native::catalog::{effective_default_profile, usable_profiles, NativeLaunchCatalog};
pub use native::endpoint_report::{
    EndpointArtifactDisposition, EndpointPublicationOutcome, NativeEndpointReport,
};
#[cfg(test)]
use shellfox_test_support::native_metadata as native_metadata_fixture;
mod owner;
mod owner_automation;
#[cfg(windows)]
mod pipe_identity;
mod platform;
pub mod power;
pub mod remote;
mod retention;
mod security;
mod service_open;
pub mod settings_runtime;
pub mod ssh;

/// Private terminal supervisor dispatch comes before any CLI parsing or UI.
pub fn dispatch_supervisor_entry() -> anyhow::Result<Option<u32>> {
    if let Some(code) = shellfox_terminal::supervisor_entry()? {
        return Ok(Some(code));
    }
    Ok(shellfox_terminal::raw_process::supervisor_entry())
}
#[cfg(test)]
mod stop_admission_tests;
pub mod system;
mod tab_stats;
mod terminals;
#[cfg(test)]
mod tests;
#[cfg(windows)]
pub mod wsl_files_worker;

use anyhow::{bail, Context, Result};
use shellfox_core::{Snapshot, TabId, TerminalIdentity};
use shellfox_host::Host;
use shellfox_runtime::Phase;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

pub use activity::hooks::{ActivityShutdownState, ShutdownFailure, ShutdownOutcome};
pub use native::NativeAdmissionError;
pub use owner::{LocalApiAccepted, NativeAccepted, OwnerClient, OwnerWake, OwnerWakeSlot};
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
    /// Called once on the owner thread before the first dispatch. Sources
    /// outside the owner inbox notify this wake when they queue work.
    fn bind_wake(&mut self, _wake: OwnerWake) {}
}
impl OwnerDispatcher for () {
    fn dispatch_one(&mut self, _service: &mut Service) {}
}

/// Construct only after the local endpoint adapter has proved private ownership.
/// One event loop owns this value and all terminal handles.
#[cfg(windows)]
mod windows_profiles;

pub(crate) use shellfox_terminal::elevated::ADMINISTRATOR_PROFILE_ID as ADMIN_POWERSHELL_PROFILE;

pub struct Service {
    host: Host,
    terminals: HashMap<TabId, terminals::OwnedTerminal>,
    /// The host data root.
    directory: PathBuf,
    supervisor: PathBuf,
    next_request: u64,
    grants: grant::custody::GrantCustody,
    grant_recipient: shellfox_protocol::CancellationToken,
    next_viewer_id: u64,
    native: native::NativeState,
    catalog: native::catalog::NativeCatalogState,
    power: power::service::ServicePower,
    activity: activity::service::ServiceActivity,
    retention: retention::Retention,
    ownership_lost: shellfox_protocol::CancellationToken,
    stopping: bool,
    metadata_closing: bool,
    closing: Option<close::Closing>,
    automation: owner_automation::AutomationState,
    ssh_profiles: Option<crate::ssh::Store>,
    system_native_handler: Option<std::sync::Arc<system::SystemNativeHandler>>,
    event_wake: Option<shellfox_runtime::EventWake>,
    runtime_signal: std::sync::Arc<owner::wait::RuntimeSignal>,
    /// The runtime that started the owner, for small async watchers.
    runtime_handle: Option<tokio::runtime::Handle>,
    /// When this host opened its data, as RFC 3339 UTC.
    started_at: String,
    /// The start a create or addTab began; its reply waits in launch_replies.
    launch_begun: Option<TabId>,
    launch_replies: Vec<(TabId, owner::launching::LaunchReply)>,
    // Last: Host/SQLite and every deferred resource owner drop before this guard.
    // Original directory lifetime only, not path/VFS confinement.
    _root_lifetime: Option<shellfox_transport::RootLifetimeGuard>,
}
impl Service {
    pub(crate) fn host(&self) -> &Host {
        &self.host
    }
    pub fn close_tab(&mut self, id: TabId) -> Result<Snapshot> {
        self.close_now(close::CloseTarget::Tab(id))
    }
    pub fn delete_session(&mut self, id: shellfox_core::SessionId) -> Result<Snapshot> {
        self.close_now(close::CloseTarget::Session(id))
    }
    /// Must succeed before the endpoint disappears or metadata ownership releases.
    /// An error retains every unconfirmed runtime and the database lock for retry.
    pub fn shutdown(&mut self) -> std::result::Result<ShutdownOutcome, ShutdownFailure> {
        // Fence admission before the first resource cleanup. A failed/uncertain
        // shutdown may retry cleanup, but can never reopen launch admission.
        self.stopping = true;
        self.abandon_close("The host is shutting down; the close was not finished.");
        self.begin_stop_power();
        self.begin_stop_native_catalog();
        self.begin_stop_system_native();
        self.stop_automation()
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
        // Start every owned PTY's shutdown before waiting for any output pipe
        // to finish. Independent runtimes can drain and close in parallel.
        for &identity in &identities {
            self.begin_terminal_stop(identity)
                .map_err(ShutdownFailure::ResourceCleanup)?;
        }
        let deadline = Instant::now() + Duration::from_secs(12);
        for identity in identities {
            self.confirm_terminal_stop(identity, deadline)
                .map_err(ShutdownFailure::ResourceCleanup)?;
        }
        self.stop_native_catalog()
            .map_err(ShutdownFailure::ResourceCleanup)?;
        self.stop_power()
            .map_err(ShutdownFailure::ResourceCleanup)?;
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
