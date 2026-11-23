use crate::terminals::OwnedTerminal;
use crate::*;
use shellfox_core::{Generation, TerminalIdentity};
use shellfox_runtime::{Identity, Runtime};
use shellfox_terminal::{
    validate_isolated_path, EnvironmentPolicy, IsolationPathKind, PtyConfig, ShellProfileId,
};

impl Service {
    fn prepare_launch_config(
        &self,
        launch: shellfox_core::TerminalLaunch,
        generation: Generation,
    ) -> Result<PtyConfig> {
        let tab = launch.tab_id;
        let mut config = self.catalog_launch_config(launch)?;
        config.isolation_root = self.generation_home(tab, generation)?;
        Ok(config)
    }
    fn catalog_launch_config(&self, launch: shellfox_core::TerminalLaunch) -> Result<PtyConfig> {
        let mut config = PtyConfig::new(self.isolation.clone());
        // Interactive app launches opt into executables, never installed account
        // state. PtyConfig::new remains Minimal for callers that do not opt in.
        config.environment = EnvironmentPolicy::InstalledTools;
        config.cwd = PathBuf::from(launch.cwd);
        config.supervisor = Some(self.supervisor.clone());
        config.terminal_marker = Some(launch.tab_id.to_string());
        config.env = launch
            .env
            .into_iter()
            .map(|entry| (entry.name.into(), entry.value.into()))
            .collect();
        let profile = launch.profile_id.as_str();
        #[cfg(windows)]
        {
            if profile == ADMIN_POWERSHELL_PROFILE {
                return Ok(config);
            }
            if profile.starts_with("wsl:") {
                self.catalog.wsl.cache.ready()?.configure(profile, &mut config)?;
                return Ok(config);
            }
        }
        config.profile = Some(
            profile
                .parse::<ShellProfileId>()
                .context("Unsupported local shell profile.")?,
        );
        Ok(config)
    }
    pub(crate) fn generation_home(&self, tab: TabId, generation: Generation) -> Result<PathBuf> {
        let tab_home = self.isolation.join(tab.to_string());
        private_child_directory(&tab_home, &self.isolation, false)?;
        let home = tab_home.join(generation.to_string());
        private_child_directory(&home, &tab_home, true)?;
        Ok(home)
    }
    pub(crate) fn start_terminal(&mut self, tab: TabId) -> Result<()> {
        self.start_terminal_policy(tab, false)
    }
    #[cfg(windows)]
    pub(crate) fn start_administrator_tab(&mut self, tab: TabId) -> Result<()> {
        self.start_terminal_policy(tab, true)
    }
    fn start_terminal_policy(
        &mut self,
        tab: TabId,
        explicit_admin: bool,
    ) -> Result<()> {
        self.assert_accepting()?;
        if self.terminals.len() >= 64 {
            bail!("At most 64 live terminals are supported by one host.");
        }
        let launch = self.host.terminal_launch(tab)?;
        let administrator = launch.profile_id.as_str() == ADMIN_POWERSHELL_PROFILE;
        #[cfg(windows)]
        let wsl_launch = launch.profile_id.as_str().starts_with("wsl:").then(|| {
            (launch.profile_id.clone(), launch.wsl_launch.clone(), launch.cwd.clone())
        });
        if administrator && !explicit_admin {
            bail!("Open a new Administrator PowerShell tab to request elevation.");
        }
        #[cfg(windows)]
        let helper = administrator
            .then(shellfox_terminal::elevated::InteractiveHelper::bundled)
            .transpose()?;
        let generation = Generation::new();
        let identity = Identity {
            session_id: launch.session_id,
            tab_id: tab,
            generation,
        };
        #[cfg_attr(not(windows), allow(unused_mut))]
        let mut config = self.prepare_launch_config(launch, generation)?;
        #[cfg(windows)]
        let wsl_account = match wsl_launch {
            Some((profile, binding, cwd)) => self.catalog.wsl.cache.ready()?
                .pin_launch_account(profile.as_str(), binding.as_ref(), &cwd, &mut config)?,
            None => None,
        };
        let registration = TerminalIdentity {
            tab_id: tab,
            generation,
        };
        self.host.register_terminal(registration)?;
        let hooks = match self.register_activity(identity, &config) {
            Ok(hooks) => hooks,
            Err(error) => {
                self.host.terminal_stopped(registration)?;
                return Err(error);
            }
        };
        let start_ordinary = |config, hooks| {
            Runtime::start_with_hooks(identity, config, hooks)
        };
        #[cfg(windows)]
        let started = match helper {
            Some(helper) => Runtime::start_administrator(identity, config, helper, hooks),
            None => start_ordinary(config, hooks),
        };
        #[cfg(not(windows))]
        let started = start_ordinary(config, hooks);
        let runtime = match started {
            Ok(runtime) => runtime,
            Err(error) => {
                // Thread creation failed before any PTY could exist.
                self.host.terminal_stopped(registration)?;
                self.forget_activity(identity);
                return Err(error.into());
            }
        };
        self.install_terminal(tab, OwnedTerminal::new(runtime));
        // UAC runs on the Runtime owner. Keep Service available for attach,
        // status and cancellation while the secure desktop is open.
        if administrator {
            return Ok(());
        }
        self.wait_until(
            Instant::now() + Duration::from_secs(12),
            |service| {
                service.assert_owned()?;
                service.drain(tab)?;
                let terminal = &service.terminals[&tab];
                // Keep the original owner; propagate only its typed startup error,
                // never PTY output or failed-source presentation evidence.
                if let Some(Err(error)) = terminal.startup.as_ref() {
                    return Err(anyhow::Error::new(error.clone())).context(
                        "Terminal launch failed; original owner retained until confirmed cleanup.",
                    );
                }
                match terminal.runtime.phase() {
                    Phase::Active => Ok(Some(())),
                    Phase::Closed => bail!(
                        "Terminal launch failed; closed owner retained until confirmed cleanup."
                    ),
                    _ => Ok(None),
                }
            },
            || anyhow::anyhow!("Terminal startup timed out; owner retained for cleanup."),
        )?;
        #[cfg(windows)]
        if let Some(account) = wsl_account {
            self.host.record_wsl_launch_account(registration, account)?;
        }
        Ok(())
    }
}

fn private_child_directory(path: &Path, parent: &Path, must_create: bool) -> Result<()> {
    validate_isolated_path(parent, parent, IsolationPathKind::Directory)?;
    validate_isolated_path(path, parent, IsolationPathKind::Directory)?;
    let builder = std::fs::DirBuilder::new();
    #[cfg(unix)]
    let builder = {
        use std::os::unix::fs::DirBuilderExt;
        let mut builder = builder;
        builder.mode(0o700);
        builder
    };
    match builder.create(path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists && !must_create => {}
        Err(error) => return Err(error).context("Create private terminal generation directory."),
    }
    // Windows inherits the proven private ancestor's current-user-only ACL.
    // Never follow an old generation, junction or symlink into account state.
    validate_isolated_path(path, parent, IsolationPathKind::Directory)
}

#[cfg(test)]
mod tests {
    use super::*;
    use shellfox_core::{CreateSession, ProfileId, SessionId};

    #[test]
    fn each_tab_and_generation_has_its_own_home_and_explicit_tool_policy() {
        let data = crate::tests::scratch();
        let service = Service::open_owned(data.path(), &std::env::current_exe().unwrap()).unwrap();
        let create = || {
            service
                .host()
                .create_session(CreateSession {
                    request_id: SessionId::new(),
                    title: None,
                    cwd: data.path().to_string_lossy().into_owned(),
                    profile_id: ProfileId::new("cmd").unwrap(),
                })
                .unwrap()
                .selection
                .tab_id
                .unwrap()
        };
        let a = create();
        let b = create();
        let first_generation = Generation::new();
        let first = service
            .prepare_launch_config(service.host().terminal_launch(a).unwrap(), first_generation)
            .unwrap();
        let second = service
            .prepare_launch_config(
                service.host().terminal_launch(b).unwrap(),
                Generation::new(),
            )
            .unwrap();
        let replacement = service
            .prepare_launch_config(
                service.host().terminal_launch(a).unwrap(),
                Generation::new(),
            )
            .unwrap();
        assert_ne!(first.isolation_root, second.isolation_root);
        assert_ne!(first.isolation_root, replacement.isolation_root);
        assert_eq!(
            first.isolation_root.parent().unwrap().file_name().unwrap(),
            a.to_string().as_str()
        );
        assert!(first.isolation_root.starts_with(&service.isolation));
        assert_eq!(first.environment, EnvironmentPolicy::InstalledTools);
        assert_eq!(
            PtyConfig::new(data.path()).environment,
            EnvironmentPolicy::Minimal
        );
        assert_eq!(std::fs::read_dir(&first.isolation_root).unwrap().count(), 0);
        assert!(service
            .prepare_launch_config(service.host().terminal_launch(a).unwrap(), first_generation)
            .is_err());
    }
}
