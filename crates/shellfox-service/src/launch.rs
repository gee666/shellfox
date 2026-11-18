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
    // Pure config capture: cold remote admission must precede directory/activity/registration effects.
    fn catalog_launch_config(&self, launch: shellfox_core::TerminalLaunch) -> Result<PtyConfig> {
        let profile = match launch.profile_id.as_str() {
            "pwsh" => ShellProfileId::Pwsh,
            "windows-powershell" => ShellProfileId::WindowsPowerShell,
            "cmd" => ShellProfileId::Cmd,
            "wsl" => ShellProfileId::Wsl,
            "login-shell" => ShellProfileId::LoginShell,
            _ => bail!("Unsupported local shell profile."),
        };
        let mut config = PtyConfig::new(self.isolation.clone());
        // Interactive app launches opt into executables, never installed account
        // state. PtyConfig::new remains Minimal for callers that do not opt in.
        config.environment = EnvironmentPolicy::InstalledTools;
        config.cwd = PathBuf::from(launch.cwd);
        config.supervisor = Some(self.supervisor.clone());
        config.profile = Some(profile);
        config.env = launch
            .env
            .into_iter()
            .map(|entry| (entry.name.into(), entry.value.into()))
            .collect();
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
        self.start_terminal_policy(tab, false).map(|_| ())
    }
    pub(crate) fn start_manager_native0_terminal(&mut self, tab: TabId) -> Result<bool> {
        self.start_terminal_policy(tab, true)
    }
    fn start_terminal_policy(&mut self, tab: TabId, require_native0: bool) -> Result<bool> {
        self.assert_accepting()?;
        if require_native0 && self.terminals.contains_key(&tab) {
            bail!("Cold Manager source already exists.");
        }
        self.ensure_ssh_local_start_allowed(tab)?;
        if self.terminals.len() >= 64 {
            bail!("At most 64 live terminals are supported by this experimental host.");
        }
        let launch = self.host.terminal_launch(tab)?;
        let generation = Generation::new();
        let identity = Identity {
            session_id: launch.session_id,
            tab_id: tab,
            generation,
        };
        let mut config = if require_native0 {
            self.catalog_launch_config(launch)?
        } else {
            self.prepare_launch_config(launch, generation)?
        };
        let origin = crate::native_sharing::capture_catalog_origin(&config);
        #[cfg(feature = "experimental-shellfox-native")]
        let native_creation = self.catalog_native0_creation(&origin)?;
        #[cfg(feature = "experimental-shellfox-native")]
        if require_native0 && native_creation.is_none() {
            return Ok(false);
        }
        #[cfg(not(feature = "experimental-shellfox-native"))]
        if require_native0 {
            return Ok(false);
        }
        if require_native0 {
            config.isolation_root = self.generation_home(tab, generation)?;
        }
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
        #[cfg(feature = "experimental-shellfox-native")]
        let started = match native_creation {
            Some(native) => Runtime::start_remote_eligible_native0_program_with_limits_and_hooks(
                identity,
                config,
                Default::default(),
                None,
                hooks,
                native,
            ),
            None => Runtime::start_with_hooks(identity, config, hooks),
        };
        #[cfg(not(feature = "experimental-shellfox-native"))]
        let started = Runtime::start_with_hooks(identity, config, hooks);
        let runtime = match started {
            Ok(runtime) => runtime,
            Err(error) => {
                // Thread creation failed before any PTY could exist.
                self.host.terminal_stopped(registration)?;
                self.forget_activity(identity);
                return Err(error.into());
            }
        };
        self.terminals
            .insert(tab, OwnedTerminal::from_catalog(runtime, origin));
        let deadline = Instant::now() + Duration::from_secs(12);
        loop {
            self.assert_owned()?;
            self.drain(tab)?;
            // Keep the original owner; propagate only its typed startup error,
            // never PTY output or failed-source presentation evidence.
            if let Some(Err(error)) = self.terminals[&tab].startup.as_ref() {
                return Err(anyhow::Error::new(error.clone())).context(
                    "Terminal launch failed; original owner retained until confirmed cleanup.",
                );
            }
            let phase = self.terminals[&tab].runtime.phase();
            if phase == Phase::Active
                && (!require_native0 || matches!(self.terminals[&tab].startup, Some(Ok(()))))
            {
                return Ok(true);
            }
            if phase == Phase::Closed {
                bail!("Terminal launch failed; closed owner retained until confirmed cleanup.");
            }
            if Instant::now() >= deadline {
                bail!("Terminal startup timed out; owner retained for cleanup.");
            }
            std::thread::sleep(Duration::from_millis(5));
        }
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
