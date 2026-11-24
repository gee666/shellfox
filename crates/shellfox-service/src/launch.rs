use crate::terminals::OwnedTerminal;
use crate::*;
use shellfox_core::{Generation, TerminalIdentity};
use shellfox_runtime::{Identity, Runtime};
use shellfox_terminal::{PtyConfig, ShellProfileId};

/// The Shellfox CLI launcher's directory, which 0.9 put first on PATH in
/// every tab: a headless host's own package launcher, otherwise the
/// installed one. `supervisor` is the CLI executable running this host.
fn cli_bin(supervisor: &Path) -> Option<PathBuf> {
    if let Some(dir) = crate::headless_layout::launcher_dir(supervisor) {
        return Some(dir);
    }
    let var = |name| {
        std::env::var_os(name)
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
    };
    #[cfg(windows)]
    let launchers = [var("LOCALAPPDATA").map(|dir| dir.join(r"Shellfox\bin\shellfox.cmd"))];
    #[cfg(target_os = "linux")]
    let launchers = [
        Some(PathBuf::from("/usr/bin/shellfox")),
        var("HOME").map(|home| home.join(".local/bin/shellfox")),
    ];
    #[cfg(target_os = "macos")]
    let launchers = [var("HOME").map(|home| home.join(".local/bin/shellfox"))];
    #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
    let launchers: [Option<PathBuf>; 0] = [];
    launchers
        .into_iter()
        .flatten()
        .find(|launcher| launcher.is_file())
        .and_then(|launcher| launcher.parent().map(Path::to_path_buf))
}

impl Service {
    /// A user terminal: the user's own environment plus Settings and session variables.
    fn launch_config(&self, launch: shellfox_core::TerminalLaunch) -> Result<PtyConfig> {
        let mut config = PtyConfig::user(launch.cwd);
        config.supervisor = Some(self.supervisor.clone());
        config.terminal_marker = Some(launch.tab_id.to_string());
        config.env = launch
            .env
            .into_iter()
            .map(|entry| (entry.name.into(), entry.value.into()))
            .collect();
        config.cli_bin = cli_bin(&self.supervisor);
        // The CLI in a tab talks to this host, as 0.9 did.
        if let Some(root) = crate::electron_import::electron_root(&self.directory) {
            config
                .host_paths
                .insert("SHELLFOX_USER_DATA".into(), root.into());
        }
        let profile = launch.profile_id.as_str();
        #[cfg(windows)]
        {
            if profile == ADMIN_POWERSHELL_PROFILE {
                // Settings and session variables can come from a remote device.
                config.env.clear();
                return Ok(config);
            }
            if profile.starts_with("wsl:") {
                self.catalog
                    .wsl
                    .cache
                    .ready()?
                    .configure(profile, &mut config)?;
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
    /// Starts `tab` if needed and waits, blocking the owner, until it is
    /// running. The owner's request routes start first with `begin_start`
    /// and come back here once it settled, so this rarely waits.
    pub(crate) fn start_terminal(&mut self, tab: TabId) -> Result<()> {
        if !self.terminals.contains_key(&tab) {
            self.start_terminal_policy(tab, false)?;
        }
        // A running tab, or an Administrator one on its secure desktop, is
        // used as it is, as before.
        if self
            .terminals
            .get(&tab)
            .is_some_and(|terminal| terminal.start_deadline.is_none())
        {
            return Ok(());
        }
        // startup_state enforces the start's own 12 s deadline.
        self.wait_until(
            Instant::now() + Duration::from_secs(13),
            |service| service.started(tab),
            || anyhow::anyhow!("Terminal startup timed out; owner retained for cleanup."),
        )
    }
    /// Starts `tab` if needed, without waiting. Ok(true) while its start is
    /// pending: the caller comes back once `start_settled`. Ok(false) when it
    /// runs or settled. Err when the start failed at once; nothing is retried.
    pub(crate) fn begin_start(&mut self, tab: TabId) -> Result<bool> {
        if !self.terminals.contains_key(&tab) {
            self.start_terminal_policy(tab, false)?;
        }
        Ok(self
            .terminals
            .get(&tab)
            .is_some_and(|terminal| terminal.start_deadline.is_some())
            && !self.start_settled(tab))
    }
    /// True once `tab`'s start succeeded, failed or ran out of time.
    pub(crate) fn start_settled(&mut self, tab: TabId) -> bool {
        !matches!(self.startup_state(tab), Ok(None))
    }
    /// When the earliest pending start gives up, for the owner's pass timeout.
    /// A passed deadline no longer counts: the waiter enforces it when it
    /// runs, and one that never runs must not make the owner spin.
    pub(crate) fn start_deadline(&self) -> Option<Instant> {
        let pending = self
            .terminals
            .values()
            .filter(|terminal| {
                terminal.startup.is_none() && terminal.runtime.phase() == Phase::Starting
            })
            .filter_map(|terminal| terminal.start_deadline);
        next_live(pending, Instant::now())
    }
    /// Ok(Some) once running; Ok(None) while starting; Err if it failed or timed out.
    fn startup_state(&mut self, tab: TabId) -> Result<Option<()>> {
        self.assert_owned()?;
        self.drain(tab)?;
        let terminal = self
            .terminals
            .get(&tab)
            .context("Terminal is not running.")?;
        // Keep the original owner; propagate only its typed startup error,
        // never PTY output or failed-source presentation evidence.
        if let Some(Err(error)) = terminal.startup.as_ref() {
            return Err(anyhow::Error::new(error.clone())).context(
                "Terminal launch failed; original owner retained until confirmed cleanup.",
            );
        }
        match terminal.runtime.phase() {
            Phase::Active => Ok(Some(())),
            Phase::Closed => {
                bail!("Terminal launch failed; closed owner retained until confirmed cleanup.")
            }
            _ if terminal
                .start_deadline
                .is_some_and(|deadline| Instant::now() >= deadline) =>
            {
                bail!("Terminal startup timed out; owner retained for cleanup.")
            }
            _ => Ok(None),
        }
    }
    /// `startup_state`, then records a pinned WSL account once running.
    /// A metadata write, so it runs in the request, never during a close. If
    /// every waiter was cancelled, the next attach to this terminal records it.
    /// The outcome is reported once: afterwards the tab is used as it is, as
    /// when the start used to block.
    fn started(&mut self, tab: TabId) -> Result<Option<()>> {
        let state = self.startup_state(tab);
        if !matches!(state, Ok(None)) {
            if let Some(terminal) = self.terminals.get_mut(&tab) {
                terminal.start_deadline = None;
            }
        }
        let Some(()) = state? else {
            return Ok(None);
        };
        #[cfg(windows)]
        if let Some(terminal) = self.terminals.get_mut(&tab) {
            if let Some(account) = terminal.wsl_account.take() {
                let id = terminal.runtime.identity();
                let identity = TerminalIdentity {
                    tab_id: tab,
                    generation: id.generation,
                };
                if let Err(error) = self
                    .host
                    .record_wsl_launch_account(identity, account.clone())
                {
                    if let Some(terminal) = self.terminals.get_mut(&tab) {
                        terminal.wsl_account = Some(account);
                    }
                    return Err(error.into());
                }
            }
        }
        Ok(Some(()))
    }
    #[cfg(windows)]
    pub(crate) fn start_administrator_tab(&mut self, tab: TabId) -> Result<()> {
        self.start_terminal_policy(tab, true)
    }
    fn start_terminal_policy(&mut self, tab: TabId, explicit_admin: bool) -> Result<()> {
        self.assert_accepting()?;
        if self.terminals.len() >= 64 {
            bail!("At most 64 live terminals are supported by one host.");
        }
        let launch = self.host.terminal_launch(tab)?;
        let administrator = launch.profile_id.as_str() == ADMIN_POWERSHELL_PROFILE;
        #[cfg(windows)]
        let wsl_launch = launch.profile_id.as_str().starts_with("wsl:").then(|| {
            (
                launch.profile_id.clone(),
                launch.wsl_launch.clone(),
                launch.cwd.clone(),
            )
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
        let mut config = self.launch_config(launch)?;
        #[cfg(windows)]
        let wsl_account = match wsl_launch {
            Some((profile, binding, cwd)) => self.catalog.wsl.cache.ready()?.pin_launch_account(
                profile.as_str(),
                binding.as_ref(),
                &cwd,
                &mut config,
            )?,
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
        let start_ordinary = |config, hooks| Runtime::start_with_hooks(identity, config, hooks);
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
        let mut terminal = OwnedTerminal::new(runtime);
        // UAC runs on the Runtime owner and has no deadline: the secure
        // desktop stays open as long as the user needs.
        if !administrator {
            terminal.start_deadline = Some(Instant::now() + Duration::from_secs(12));
        }
        #[cfg(windows)]
        {
            terminal.wsl_account = wsl_account;
        }
        self.install_terminal(tab, terminal);
        Ok(())
    }
}

/// The earliest deadline still in the future.
fn next_live(deadlines: impl Iterator<Item = Instant>, now: Instant) -> Option<Instant> {
    deadlines.filter(|deadline| *deadline > now).min()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_passed_start_deadline_does_not_drive_the_pass_timeout() {
        let now = Instant::now();
        let (past, soon) = (now - Duration::from_secs(1), now + Duration::from_secs(1));
        assert_eq!(
            next_live([past].into_iter(), now),
            None,
            "a missed waiter must not make the owner spin"
        );
        assert_eq!(next_live([past, soon].into_iter(), now), Some(soon));
    }
    use shellfox_core::{CreateSession, EnvVar, ProfileId, SessionId};
    use shellfox_terminal::TerminalEnvironment;

    #[test]
    fn tabs_get_the_user_environment_and_settings_variables() {
        let data = crate::tests::scratch();
        let service = Service::open_owned(data.path(), &std::env::current_exe().unwrap()).unwrap();
        let session = service
            .host()
            .create_session(CreateSession {
                request_id: SessionId::new(),
                title: None,
                cwd: data.path().to_string_lossy().into_owned(),
                profile_id: ProfileId::new("cmd").unwrap(),
            })
            .unwrap();
        let tab = session.selection.tab_id.unwrap();
        let env = vec![
            EnvVar {
                name: "PATH".into(),
                value: "/tools".into(),
            },
            EnvVar {
                name: "NODE_OPTIONS".into(),
                value: "--max-old-space-size=64".into(),
            },
        ];
        service.host().set_terminal_env(env).unwrap();
        let config = service
            .launch_config(service.host().terminal_launch(tab).unwrap())
            .unwrap();
        assert_eq!(config.environment, TerminalEnvironment::User);
        assert_eq!(config.cwd, data.path());
        assert_eq!(
            config.env.get(std::ffi::OsStr::new("PATH")).unwrap(),
            "/tools"
        );
        assert_eq!(
            config.terminal_marker.as_deref(),
            Some(tab.to_string().as_str())
        );
        assert!(!data.path().join("terminal-home").exists());
        // A custom data directory has no 0.9 root to name.
        assert!(config.host_paths.is_empty());
    }

    fn tab_in(service: &Service, profile: &str) -> TabId {
        let env = vec![EnvVar {
            name: "NODE_OPTIONS".into(),
            value: "--require=x".into(),
        }];
        let snapshot = service
            .host()
            .create_session(CreateSession {
                request_id: SessionId::new(),
                title: None,
                cwd: service.directory.to_string_lossy().into_owned(),
                profile_id: ProfileId::new(profile).unwrap(),
            })
            .unwrap();
        service
            .host()
            .set_session_env(snapshot.sessions[0].id, env)
            .unwrap();
        snapshot.selection.tab_id.unwrap()
    }

    #[test]
    fn tabs_name_the_hosts_data_root_for_the_cli() {
        let scratch = crate::tests::scratch();
        let data = scratch.path().join("Shellfox/rust-v1/host");
        std::fs::create_dir_all(&data).unwrap();
        let service = Service::open_owned(&data, &std::env::current_exe().unwrap()).unwrap();
        let tab = tab_in(&service, "cmd");
        let config = service
            .launch_config(service.host().terminal_launch(tab).unwrap())
            .unwrap();
        assert_eq!(
            config
                .host_paths
                .get(std::ffi::OsStr::new("SHELLFOX_USER_DATA"))
                .map(PathBuf::from),
            Some(scratch.path().join("Shellfox"))
        );
        assert!(config
            .env
            .contains_key(std::ffi::OsStr::new("NODE_OPTIONS")));
    }

    /// A headless package as extracted: `<root>/bin/shellfox-cli` and the
    /// `shellfox` launcher users put on PATH.
    fn headless_package(root: &Path, version: &str) -> PathBuf {
        use crate::headless_layout::{arch, exe, platform};
        std::fs::create_dir_all(root.join("bin")).unwrap();
        let build = format!(
            r#"{{"product":"headless","version":"{version}","platform":"{}","arch":"{}"}}"#,
            platform(),
            arch()
        );
        std::fs::write(root.join("build.json"), build).unwrap();
        for launcher in ["shellfox", "shellfox.cmd"] {
            std::fs::write(root.join(launcher), "").unwrap();
        }
        let cli = root.join("bin").join(exe("shellfox-cli"));
        std::fs::write(&cli, "").unwrap();
        cli
    }

    #[test]
    fn headless_tabs_get_the_original_package_launcher_on_path() {
        use crate::headless_layout::{activate, arch, normalize, platform, root_name};
        let scratch = crate::tests::scratch();
        let parent = normalize(scratch.path());
        let anchor = parent.join(root_name());
        let original = headless_package(&anchor, "0.9.15");
        assert_eq!(cli_bin(&original), Some(anchor.clone()));
        // After `shellfox update` the host runs from the new version, but the
        // anchor's launcher is the one users and services call.
        let version = parent
            .join(format!(
                "shellfox-headless-0.10.0-{}-{}-Ab12",
                platform(),
                arch()
            ))
            .join("installation")
            .join(root_name());
        let updated = headless_package(&version, "0.10.0");
        activate(&anchor, &version).unwrap();
        assert_eq!(cli_bin(&updated), Some(anchor.clone()));

        let data = scratch.path().join("data");
        std::fs::create_dir(&data).unwrap();
        let service = Service::open_owned(&data, &updated).unwrap();
        let tab = tab_in(&service, "cmd");
        let config = service
            .launch_config(service.host().terminal_launch(tab).unwrap())
            .unwrap();
        assert_eq!(config.cli_bin, Some(anchor));
    }

    #[cfg(windows)]
    #[test]
    fn administrator_tabs_carry_no_settings_or_session_variables() {
        let data = crate::tests::scratch();
        let service = Service::open_owned(data.path(), &std::env::current_exe().unwrap()).unwrap();
        service
            .host()
            .set_terminal_env(vec![EnvVar {
                name: "PATH".into(),
                value: "/x".into(),
            }])
            .unwrap();
        let tab = tab_in(&service, ADMIN_POWERSHELL_PROFILE);
        let config = service
            .launch_config(service.host().terminal_launch(tab).unwrap())
            .unwrap();
        assert!(config.env.is_empty());
    }
}
