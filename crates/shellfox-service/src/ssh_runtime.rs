//! Exactly one existing Service Host/Runtime registry. No parser or process owner here.
use super::native::SshProgramPolicy;
use super::*;
use crate::{terminals::OwnedTerminal, Service};
use anyhow::{ensure, Context};
use shellfox_core::{Generation, TabId, TerminalIdentity};
use shellfox_runtime::{Identity, Phase, Runtime};
use shellfox_terminal::{PtyCommand, PtyConfig};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    ffi::OsString,
};

pub(super) struct RuntimeState {
    pub targets: HashMap<TabId, SourceConnectTarget>,
    pub started: HashMap<TabId, Identity>,
    closed: HashMap<TabId, Identity>,
    pub taps: HashMap<TabId, Arc<super::tap::ProgramTap>>,
    policy: Arc<dyn SshProgramPolicy>,
}
impl RuntimeState {
    pub fn new(policy: Arc<dyn SshProgramPolicy>) -> Self {
        Self {
            targets: HashMap::new(),
            started: HashMap::new(),
            closed: HashMap::new(),
            taps: HashMap::new(),
            policy,
        }
    }
    pub fn record_closed(&mut self, identity: Identity) {
        if self.started.get(&identity.tab_id) == Some(&identity) {
            self.closed.insert(identity.tab_id, identity);
        }
    }
}
pub(super) struct Validator {
    grants: HashSet<u64>,
    tabs: HashSet<TabId>,
    lost: CancellationToken,
}
impl Validator {
    pub fn denied(service: &Service) -> Self {
        Self {
            grants: HashSet::new(),
            tabs: HashSet::new(),
            lost: service.ownership_lost.clone(),
        }
    }
    pub fn capture(service: &Service, grants: &HashSet<u64>) -> Result<Self> {
        let tabs = service
            .host
            .snapshot()?
            .sessions
            .iter()
            .flat_map(|s| s.tabs.iter().map(|t| t.id))
            .collect();
        Ok(Self {
            grants: grants.clone(),
            tabs,
            lost: service.ownership_lost.clone(),
        })
    }
}
impl NativeControllerValidator for Validator {
    fn validate_native(&self, controller: u64) -> Result<()> {
        ensure!(
            !self.lost.is_cancelled() && self.grants.contains(&controller),
            "Native SSH grant is absent or revoked."
        );
        Ok(())
    }
    fn validate_target(&self, controller: u64, tab: TabId) -> Result<()> {
        self.validate_native(controller)?;
        ensure!(
            self.tabs.contains(&tab),
            "SSH target is not a current metadata tab."
        );
        Ok(())
    }
}
pub(super) struct ServiceStarter<'a> {
    service: &'a mut Service,
    state: &'a mut RuntimeState,
}
impl<'a> ServiceStarter<'a> {
    pub fn new(service: &'a mut Service, state: &'a mut RuntimeState) -> Self {
        Self { service, state }
    }
}
impl OwnedRuntimeStarter for ServiceStarter<'_> {
    fn reserve_target(&mut self, _: u64, target: SourceConnectTarget) -> Result<()> {
        self.service.assert_accepting()?;
        self.service.host.terminal_launch(target.tab())?;
        ensure!(
            !self.service.terminals.contains_key(&target.tab())
                && !self.state.targets.contains_key(&target.tab()),
            "SSH target has an active or unclosed source."
        );
        ensure!(
            self.service.terminals.len() + self.state.targets.len() < 64,
            "Owned terminal capacity reached."
        );
        self.state.targets.insert(target.tab(), target);
        Ok(())
    }
    fn release_target(&mut self, target: SourceConnectTarget) -> Result<()> {
        ensure!(
            self.state.targets.get(&target.tab()) == Some(&target),
            "Stale SSH source release."
        );
        ensure!(
            !self.service.terminals.contains_key(&target.tab()),
            "Original terminal owner is still registered."
        );
        if let Some(identity) = self.state.started.get(&target.tab()) {
            ensure!(
                self.state.closed.get(&target.tab()) == Some(identity),
                "Original SSH cleanup is unconfirmed."
            );
        }
        self.state.targets.remove(&target.tab());
        self.state.started.remove(&target.tab());
        self.state.closed.remove(&target.tab());
        self.state.taps.remove(&target.tab());
        Ok(())
    }
    fn runtime_is_current(&self, identity: Identity) -> Result<bool> {
        Ok(self.state.started.get(&identity.tab_id) == Some(&identity)
            && self
                .service
                .terminals
                .get(&identity.tab_id)
                .is_some_and(|t| {
                    t.runtime.identity() == identity && t.runtime.phase() == Phase::Active
                }))
    }
    fn start(
        &mut self,
        _: u64,
        target: SourceConnectTarget,
        launch: &shellfox_ssh::system::SshLaunchRequest,
        local: &BTreeMap<String, String>,
    ) -> Result<Identity> {
        self.service.assert_accepting()?;
        ensure!(
            self.state.targets.get(&target.tab()) == Some(&target)
                && !self.service.terminals.contains_key(&target.tab()),
            "SSH reservation was lost or reused."
        );
        self.state
            .policy
            .validate_current(SshProgramRole::Ssh, &launch.executable)?;
        ensure!(
            launch.authentication.saved_password.is_none(),
            "A credential must stay in the private broker, not the runtime."
        );
        let metadata = self.service.host.terminal_launch(target.tab())?;
        let identity = Identity {
            session_id: metadata.session_id,
            tab_id: target.tab(),
            generation: Generation::new(),
        };
        let mut config = PtyConfig::new(
            self.service
                .generation_home(target.tab(), identity.generation)?,
        );
        config.cwd = self.service.isolation.clone();
        config.supervisor = Some(self.service.supervisor.clone());
        config.cols = launch.pty.cols;
        config.rows = launch.pty.rows;
        config.command = Some(PtyCommand {
            program: launch.executable.as_os_str().to_owned(),
            args: launch.args.clone(),
        });
        config.env = launch
            .environment
            .iter()
            .filter(|(name, _)| !matches!(name.as_str(), "TERM" | "COLORTERM"))
            .map(|(k, v)| (OsString::from(k), OsString::from(v)))
            .collect();
        shellfox_runtime::GridLimits::default().validate_size(config.cols, config.rows)?;
        let program_env =
            super::program_env::capability(&launch.executable, local, self.state.policy.clone())?;
        let registration = TerminalIdentity {
            tab_id: target.tab(),
            generation: identity.generation,
        };
        self.service.host.register_terminal(registration)?;
        let hooks = (|| -> Result<_> {
            let _ = self.service.register_activity(identity, &config)?;
            // The common sink/context remains installed; only inspection is corrected.
            // No local-shell/default fact describes this SSH remote command.
            let expected = launch
                .executable
                .to_str()
                .context("SSH executable encoding unsupported")?
                .to_owned();
            self.service
                .activity
                .owner
                .watch(
                    identity,
                    expected,
                    crate::activity_integration::InspectionScope::Ssh,
                )
                .map_err(|_| anyhow::anyhow!("SSH program inspection registration failed"))?;
            let activity = self
                .service
                .activity
                .owner
                .live_output_sink(identity)
                .context("SSH live activity sink missing")?;
            let tap = Arc::new(super::tap::ProgramTap::new(
                identity,
                activity,
                launch.expected_authentication.clone(),
            ));
            Ok(tap)
        })();
        let tap = match hooks {
            Ok(t) => t,
            Err(e) => {
                self.service.host.terminal_stopped(registration)?;
                self.service.forget_activity(identity);
                return Err(e);
            }
        };
        let hooks = shellfox_runtime::RuntimeHooks {
            live_output: Some(tap.clone()),
        };
        if let Err(error) = self
            .state
            .policy
            .validate_current(SshProgramRole::Ssh, &launch.executable)
        {
            self.service.host.terminal_stopped(registration)?;
            self.service.forget_activity(identity);
            return Err(error);
        }
        let runtime = match Runtime::start_program_with_hooks(identity, config, program_env, hooks)
        {
            Ok(r) => r,
            Err(error) => {
                self.service.host.terminal_stopped(registration)?;
                self.service.forget_activity(identity);
                return Err(error.into());
            }
        };
        // From this point every result carries the original identity, including failed startup.
        self.service
            .terminals
            .insert(target.tab(), OwnedTerminal::new(runtime));
        self.state.started.insert(target.tab(), identity);
        self.state.taps.insert(target.tab(), tap);
        Ok(identity)
    }
    fn close(&mut self, identity: Identity) -> Result<bool> {
        if self.state.closed.get(&identity.tab_id) == Some(&identity) {
            return Ok(true);
        }
        ensure!(
            self.state.started.get(&identity.tab_id) == Some(&identity),
            "Unknown SSH generation cannot be closed."
        );
        ensure!(
            self.service
                .terminals
                .get(&identity.tab_id)
                .is_some_and(|t| t.runtime.identity() == identity),
            "Original SSH runtime is absent without cleanup proof."
        );
        self.service.stop_terminal(TerminalIdentity {
            tab_id: identity.tab_id,
            generation: identity.generation,
        })?;
        self.state.record_closed(identity);
        Ok(true)
    }
}
pub(super) fn pump_confirmations(manager: &mut SshManager, state: &RuntimeState) {
    for (tab, tap) in &state.taps {
        if let Some(identity) = state.started.get(tab) {
            if let Some(proof) = tap.confirmation() {
                manager.accept_live_confirmation(*identity, &proof);
            }
        }
    }
    for identity in state.closed.values() {
        manager.runtime_closed(*identity);
    }
}
