//! Protected in-process adapter. Not a remote/local wire method and never a UI dialog.
use super::*;
use crate::Service;
use anyhow::{ensure, Context};
use shellfox_core::TabId;
use shellfox_runtime::Identity;
use std::{collections::HashSet, path::Path};

/// Trusted bootstrap implementation must retain and revalidate its executable/resource
/// evidence. No permissive default is supplied, and no request can choose this policy.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SshProgramRole {
    Ssh,
    Keyscan,
    Askpass,
}
pub trait SshProgramPolicy: Send + Sync {
    fn validate_current(&self, role: SshProgramRole, executable: &Path) -> Result<()>;
}
/// Non-serializable startup resources. The private directory must be THIS Service's
/// rust-service-v1/ssh directory, not a frontend or remote-provided credential store.
pub struct NativeSshInstall {
    pub directory: Arc<PrivateDirectory>,
    pub adapter: Arc<SystemSshAdapter>,
    pub imports: Arc<dyn NativePlatform>,
    pub askpass: PathBuf,
    pub program: Arc<dyn SshProgramPolicy>,
}
pub(super) struct Installed {
    pub manager: SshManager,
    pub runtime: super::runtime::RuntimeState,
}
#[derive(Default)]
pub struct NativeSshOwner {
    pub(super) installed: Option<Installed>,
    pub(super) grants: HashSet<u64>,
    stopping: bool,
}
impl NativeSshOwner {
    pub fn install(&mut self, service: &Service, config: NativeSshInstall) -> Result<()> {
        service.assert_accepting()?;
        ensure!(
            !self.stopping && self.installed.is_none(),
            "SSH services are already installed or stopping."
        );
        let expected = service
            .isolation
            .parent()
            .context("Service data root missing")?
            .join("rust-service-v1/ssh");
        let expected =
            shellfox_transport::normalize_private_path(&std::fs::canonicalize(expected)?)?;
        let actual = shellfox_transport::normalize_private_path(&std::fs::canonicalize(
            config.directory.path(),
        )?)?;
        ensure!(
            actual == expected,
            "SSH resources do not belong to this Service."
        );
        config
            .program
            .validate_current(SshProgramRole::Askpass, &config.askpass)?;
        config
            .program
            .validate_current(SshProgramRole::Ssh, config.adapter.ssh_executable())?;
        config
            .program
            .validate_current(SshProgramRole::Keyscan, config.adapter.keyscan_executable())?;
        let manager = SshManager::new(
            config.directory,
            config.adapter,
            config.imports,
            config.askpass,
            service.ownership_lost.clone(),
        )?;
        self.installed = Some(Installed {
            manager,
            runtime: super::runtime::RuntimeState::new(config.program),
        });
        Ok(())
    }
    /// Only the trusted, non-serializable owner Task::Native admission calls this.
    pub fn admit(&mut self, controller: u64) -> Result<()> {
        ensure!(
            controller > 0 && !self.stopping,
            "Native SSH admission is closed."
        );
        ensure!(
            self.grants.contains(&controller) || self.grants.len() < 128,
            "Native SSH controller limit reached."
        );
        self.grants.insert(controller);
        Ok(())
    }
    pub fn handle(
        &mut self,
        service: &mut Service,
        controller: u64,
        request: Request,
    ) -> Result<Response> {
        service.assert_accepting()?;
        ensure!(
            self.grants.contains(&controller) && !self.stopping,
            "Native SSH authority is unavailable."
        );
        self.pump(service);
        let mut response = {
            let state = self
                .installed
                .as_mut()
                .context("SSH services have not been installed by the owning bootstrap.")?;
            let validator = super::runtime::Validator::capture(service, &self.grants)?;
            let mut starter = super::runtime::ServiceStarter::new(service, &mut state.runtime);
            state
                .manager
                .request(controller, request, &validator, &mut starter)?
        };
        self.project_response(service, controller, &mut response);
        Ok(response)
    }
    pub fn pump(&mut self, service: &mut Service) {
        let Some(state) = &mut self.installed else {
            return;
        };
        super::runtime::pump_confirmations(&mut state.manager, &state.runtime);
        let completed: Vec<_> = state
            .runtime
            .started
            .values()
            .copied()
            .filter(|id| {
                service.terminals.get(&id.tab_id).is_some_and(|t| {
                    t.runtime.identity() == *id
                        && t.runtime.phase() == shellfox_runtime::Phase::Closed
                })
            })
            .collect();
        for identity in completed {
            let diagnostic = state
                .runtime
                .taps
                .get(&identity.tab_id)
                .and_then(|tap| tap.public_diagnostics());
            let mut starter = super::runtime::ServiceStarter::new(service, &mut state.runtime);
            if matches!(starter.close(identity), Ok(true)) {
                state
                    .manager
                    .runtime_closed_diagnostic(identity, diagnostic.as_deref());
            }
        }
        let validator = match super::runtime::Validator::capture(service, &self.grants) {
            Ok(v) => v,
            Err(_) => {
                state.manager.begin_shutdown();
                super::runtime::Validator::denied(service)
            }
        };
        let mut starter = super::runtime::ServiceStarter::new(service, &mut state.runtime);
        state.manager.poll(&validator, &mut starter);
    }
    pub fn detach(&mut self, controller: u64) {
        self.grants.remove(&controller);
        if let Some(s) = &mut self.installed {
            s.manager.detach_controller(controller);
        }
    }
    pub fn revoke(&mut self, controller: u64) {
        self.grants.remove(&controller);
        if let Some(s) = &mut self.installed {
            s.manager.revoke_native_capability(controller);
        }
    }
    pub fn begin_stop(&mut self) {
        self.stopping = true;
        self.grants.clear();
        if let Some(s) = &mut self.installed {
            s.manager.begin_shutdown();
        }
    }
    pub fn stop(&mut self, service: &mut Service) -> Result<bool> {
        self.begin_stop();
        self.pump(service);
        Ok(self
            .installed
            .as_ref()
            .is_none_or(|s| s.manager.is_closed()))
    }
    pub fn ensure_local_start_allowed(&self, tab: TabId) -> Result<()> {
        if let Some(s) = &self.installed {
            ensure!(
                !s.runtime.targets.contains_key(&tab),
                "Tab is reserved for SSH; local auto-start is forbidden."
            );
        }
        Ok(())
    }
    pub fn close_targets(&mut self, service: &mut Service, tabs: &[TabId]) -> Result<bool> {
        let Some(s) = &mut self.installed else {
            return Ok(true);
        };
        for c in s
            .manager
            .connections
            .values_mut()
            .filter(|c| tabs.contains(&c.status.tab))
        {
            c.stop();
        }
        self.pump(service);
        Ok(self
            .installed
            .as_ref()
            .is_none_or(|s| !tabs.iter().any(|tab| s.runtime.targets.contains_key(tab))))
    }
    /// Called only AFTER Service confirmed original process closure and stopped metadata.
    pub fn record_closed(&mut self, identity: Identity) {
        if let Some(s) = &mut self.installed {
            s.runtime.record_closed(identity);
            let diagnostic = s
                .runtime
                .taps
                .get(&identity.tab_id)
                .and_then(|tap| tap.public_diagnostics());
            s.manager
                .runtime_closed_diagnostic(identity, diagnostic.as_deref());
        }
    }
}
impl Service {
    pub(crate) fn handle_ssh_native(
        &mut self,
        request: Request,
        controller: u64,
    ) -> Result<Response> {
        let mut owner = std::mem::take(&mut self.ssh);
        let result = owner.handle(self, controller, request);
        self.ssh = owner;
        result
    }
    pub fn install_ssh_native(&mut self, config: NativeSshInstall) -> Result<()> {
        let mut owner = std::mem::take(&mut self.ssh);
        let result = owner.install(self, config);
        self.ssh = owner;
        result
    }
    pub(crate) fn admit_ssh_native(&mut self, controller: u64) -> Result<()> {
        self.ssh.admit(controller)
    }
    pub(crate) fn pump_ssh_native(&mut self) {
        let mut owner = std::mem::take(&mut self.ssh);
        owner.pump(self);
        self.ssh = owner;
    }
    pub(crate) fn begin_stop_ssh_native(&mut self) {
        self.ssh.begin_stop();
    }
    pub(crate) fn stop_ssh_native(&mut self) -> Result<bool> {
        let mut owner = std::mem::take(&mut self.ssh);
        let result = owner.stop(self);
        self.ssh = owner;
        result
    }
    pub(crate) fn detach_ssh_native(&mut self, controller: u64) {
        self.ssh.detach(controller)
    }
    pub(crate) fn revoke_ssh_native(&mut self, controller: u64) {
        self.ssh.revoke(controller)
    }
    pub(crate) fn ensure_ssh_local_start_allowed(&self, tab: TabId) -> Result<()> {
        self.ssh.ensure_local_start_allowed(tab)
    }
    pub(crate) fn close_ssh_targets(&mut self, tabs: &[TabId]) -> Result<bool> {
        let mut owner = std::mem::take(&mut self.ssh);
        let result = owner.close_targets(self, tabs);
        self.ssh = owner;
        result
    }
    pub(crate) fn record_ssh_closed(&mut self, identity: Identity) {
        self.ssh.record_closed(identity)
    }
}
