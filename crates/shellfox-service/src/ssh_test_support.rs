use super::*;
use shellfox_ssh::{system::SshPrograms, StoragePolicy};
pub(super) use shellfox_ssh::{ProcessOutput, ProcessRequest, ProcessRunner};
pub(super) use std::{
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};
pub(super) const PUBLIC: &str =
    "AAAAC3NzaC1lZDI1NTE5AAAAINdamAGCsQq31Uv+08lkBzoO4XLz2qYjJa8CGmj3B1Ea";
struct Probe;
impl ProcessRunner for Probe {
    fn run(&self, _: &ProcessRequest) -> shellfox_ssh::Result<ProcessOutput> {
        Ok(ProcessOutput {
            success: true,
            stdout: format!("fixture.invalid ssh-ed25519 {PUBLIC}\n").into_bytes(),
            stderr: vec![],
        })
    }
}
struct Platform;
impl NativePlatform for Platform {
    fn import_sessions(&self) -> Result<Vec<shellfox_ssh::ImportSession>> {
        Ok(vec![])
    }
}
pub(super) struct Validator(pub(super) AtomicBool);
impl NativeControllerValidator for Validator {
    fn validate_native(&self, controller: u64) -> Result<()> {
        anyhow::ensure!(
            [7, 8].contains(&controller) && self.0.load(Ordering::SeqCst),
            "Revoked native application capability."
        );
        Ok(())
    }
    fn validate_target(&self, controller: u64, _: shellfox_core::TabId) -> Result<()> {
        self.validate_native(controller)
    }
}
pub(super) struct Fixture {
    pub(super) directory: Arc<PrivateDirectory>,
    _temp: tempfile::TempDir,
}
fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_owned()
}
impl Fixture {
    pub(super) fn new() -> Self {
        std::fs::create_dir_all(root().join("tmp")).unwrap();
        let temp = tempfile::Builder::new()
            .prefix("service-ssh-")
            .tempdir_in(root().join("tmp"))
            .unwrap();
        let policy = if cfg!(windows) {
            StoragePolicy::with_windows_helper(
                root().join("tmp/tailscale/win32-x64/shellfox-tailscale.exe"),
            )
        } else {
            StoragePolicy::default()
        };
        let directory = Arc::new(PrivateDirectory::open(temp.path().join("ssh"), policy).unwrap());
        Self {
            directory,
            _temp: temp,
        }
    }
    pub(super) fn manager(&self) -> SshManager {
        self.with_probe(Arc::new(Probe))
    }
    pub(super) fn with_probe(&self, runner: Arc<dyn ProcessRunner>) -> SshManager {
        let executable = std::env::current_exe().unwrap();
        let backend = Arc::new(
            SystemSshAdapter::new(
                SshPrograms {
                    ssh: executable.clone(),
                    keyscan: executable.clone(),
                },
                runner,
                self.directory.clone(),
            )
            .unwrap(),
        );
        SshManager::new(
            self.directory.clone(),
            backend,
            Arc::new(Platform),
            executable,
            CancellationToken::new(),
        )
        .unwrap()
    }
}
#[derive(Default)]
pub(super) struct Starter {
    pub(super) starts: usize,
    pub(super) identity: Option<shellfox_runtime::Identity>,
    pub(super) allow_close: bool,
    pub(super) targets: HashMap<shellfox_core::TabId, SourceConnectTarget>,
    pub(super) releases: usize,
    pub(super) closes: usize,
    pub(super) claims: usize,
    pub(super) reservation_seen: Option<Arc<AtomicBool>>,
    pub(super) command: Option<String>,
    pub(super) deny_reserve: bool,
}
impl OwnedRuntimeStarter for Starter {
    fn reserve_target(&mut self, _: u64, target: SourceConnectTarget) -> Result<()> {
        anyhow::ensure!(
            !self.deny_reserve
                && !self.targets.contains_key(&target.tab())
                && self.identity.is_none_or(|i| i.tab_id != target.tab()),
            "Local or other source already owns target."
        );
        self.targets.insert(target.tab(), target);
        if let Some(flag) = &self.reservation_seen {
            flag.store(true, Ordering::SeqCst)
        }
        Ok(())
    }
    fn release_target(&mut self, target: SourceConnectTarget) -> Result<()> {
        anyhow::ensure!(
            self.targets.get(&target.tab()) == Some(&target),
            "Stale reservation release."
        );
        anyhow::ensure!(
            self.identity.is_none_or(|i| i.tab_id != target.tab()),
            "Runtime still owns target."
        );
        self.targets.remove(&target.tab());
        self.releases += 1;
        Ok(())
    }
    fn runtime_is_current(&self, identity: shellfox_runtime::Identity) -> Result<bool> {
        Ok(self.identity == Some(identity))
    }
    fn start(
        &mut self,
        _: u64,
        target: SourceConnectTarget,
        request: &shellfox_ssh::system::SshLaunchRequest,
        local: &std::collections::BTreeMap<String, String>,
    ) -> Result<shellfox_runtime::Identity> {
        anyhow::ensure!(
            self.targets.get(&target.tab()) == Some(&target),
            "SSH source was not reserved."
        );
        self.starts += 1;
        assert!(request.authentication.saved_password.is_none());
        assert!(!format!("{:?}", request.args).contains("fixture-secret"));
        assert!(!local.values().any(|v| v == "fixture-secret"));
        self.command = request
            .args
            .last()
            .map(|s| s.to_string_lossy().into_owned());
        let identity = shellfox_runtime::Identity {
            session_id: shellfox_core::SessionId::new(),
            tab_id: target.tab(),
            generation: shellfox_core::Generation::new(),
        };
        self.identity = Some(identity);
        Ok(identity)
    }
    fn close(&mut self, identity: shellfox_runtime::Identity) -> Result<bool> {
        self.closes += 1;
        if self.allow_close && self.identity == Some(identity) {
            self.identity = None;
            return Ok(true);
        }
        Ok(false)
    }
}
pub(super) fn input() -> Input {
    Input {
        id: Uuid::new_v4(),
        name: "fixture".into(),
        host: "fixture.invalid".into(),
        port: 22,
        user: "fixture".into(),
        password: Password::Set(Secret::new("fixture-secret".into()).unwrap()),
        key_file: None,
        remote_cwd: None,
    }
}
pub(super) fn drive(
    manager: &mut SshManager,
    validator: &dyn NativeControllerValidator,
    starter: &mut dyn OwnedRuntimeStarter,
    test: impl Fn(&SshManager) -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(8);
    while !test(manager) {
        manager.poll(validator, starter);
        assert!(Instant::now() < deadline, "SSH fixture deadline");
        std::thread::sleep(Duration::from_millis(10));
    }
}
pub(super) fn connect(
    manager: &mut SshManager,
    validator: &dyn NativeControllerValidator,
    starter: &mut dyn OwnedRuntimeStarter,
) -> Uuid {
    let profile = input();
    let id = profile.id;
    manager
        .request(7, Request::Save(profile), validator, starter)
        .unwrap();
    let Response::Status(status) = manager
        .request(
            7,
            Request::Connect {
                profile: id,
                tab: shellfox_core::TabId::new(),
                cols: 80,
                rows: 24,
            },
            validator,
            starter,
        )
        .unwrap()
    else {
        panic!("status")
    };
    status[0].connection
}
pub(super) fn approve(
    manager: &mut SshManager,
    validator: &dyn NativeControllerValidator,
    starter: &mut dyn OwnedRuntimeStarter,
    id: Uuid,
) {
    drive(manager, validator, starter, |m| {
        matches!(m.connections[&id].status.state, State::AwaitingHost(_))
    });
    let State::AwaitingHost(challenge) = manager.connections[&id].status.state.clone() else {
        panic!("challenge")
    };
    manager
        .request(
            7,
            Request::ConfirmHost {
                connection: id,
                challenge: challenge.challenge,
                decision: TrustDecision::TrustAndSave,
            },
            validator,
            starter,
        )
        .unwrap();
}
pub(super) fn authenticate(
    manager: &mut SshManager,
    validator: &dyn NativeControllerValidator,
    starter: &mut Starter,
    id: Uuid,
) -> shellfox_runtime::Identity {
    approve(manager, validator, starter, id);
    drive(manager, validator, starter, |m| {
        matches!(m.connections[&id].status.state, State::Authenticating)
    });
    let command = starter.command.as_ref().unwrap();
    let marker = command
        .split("printf '\\033")
        .nth(1)
        .and_then(|s| s.split("\\007'").next())
        .unwrap();
    let identity = starter.identity.unwrap();
    manager.observe_output(identity, format!("\x1b{marker}\x07").as_bytes());
    assert_eq!(manager.connections[&id].status.state, State::Ready);
    identity
}
