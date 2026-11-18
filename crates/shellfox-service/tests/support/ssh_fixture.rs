#[path = "ssh_process.rs"]
mod process;
use anyhow::Result;
use process::FakeProcess;
use shellfox_core::{Generation, SessionId, TabId};
use shellfox_protocol::CancellationToken;
use shellfox_runtime::Identity;
use shellfox_service::ssh::*;
use shellfox_ssh::{
    system::{SshPrograms, SystemSshAdapter},
    PrivateDirectory, ProcessOutput, ProcessRequest, ProcessRunner, Secret, StoragePolicy,
};
use std::{
    collections::{BTreeMap, HashMap},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use uuid::Uuid;
const PUBLIC: &str = "AAAAC3NzaC1lZDI1NTE5AAAAINdamAGCsQq31Uv+08lkBzoO4XLz2qYjJa8CGmj3B1Ea";
struct Probe(Arc<AtomicBool>);
impl ProcessRunner for Probe {
    fn run(&self, _: &ProcessRequest) -> shellfox_ssh::Result<ProcessOutput> {
        assert!(
            self.0.load(Ordering::SeqCst),
            "Preparation preceded source reservation"
        );
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
pub struct Native {
    pub valid: AtomicBool,
}
impl NativeControllerValidator for Native {
    fn validate_native(&self, controller: u64) -> Result<()> {
        anyhow::ensure!(
            [7, 8].contains(&controller) && self.valid.load(Ordering::SeqCst),
            "Native app capability denied."
        );
        Ok(())
    }
    fn validate_target(&self, controller: u64, _: TabId) -> Result<()> {
        self.validate_native(controller)
    }
}
pub struct Fixture {
    pub manager: SshManager,
    pub starter: Starter,
    pub native: Native,
    _temp: tempfile::TempDir,
}
impl Fixture {
    pub fn new(run_process: bool) -> Self {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .to_owned();
        std::fs::create_dir_all(root.join("tmp")).unwrap();
        let temp = tempfile::Builder::new()
            .prefix("service-ssh-public-")
            .tempdir_in(root.join("tmp"))
            .unwrap();
        let policy = if cfg!(windows) {
            StoragePolicy::with_windows_helper(
                root.join("tmp/tailscale/win32-x64/shellfox-tailscale.exe"),
            )
        } else {
            StoragePolicy::default()
        };
        let private = Arc::new(PrivateDirectory::open(temp.path().join("ssh"), policy).unwrap());
        let executable = std::env::current_exe().unwrap();
        let reserved = Arc::new(AtomicBool::new(false));
        let backend = Arc::new(
            SystemSshAdapter::new(
                SshPrograms {
                    ssh: executable.clone(),
                    keyscan: executable.clone(),
                },
                Arc::new(Probe(reserved.clone())),
                private.clone(),
            )
            .unwrap(),
        );
        let manager = SshManager::new(
            private,
            backend,
            Arc::new(Platform),
            executable,
            CancellationToken::new(),
        )
        .unwrap();
        Self {
            manager,
            starter: Starter {
                reserved,
                run_process,
                targets: HashMap::new(),
                identity: None,
                starts: 0,
                closes: 0,
                releases: 0,
                allow_close: false,
                child: None,
            },
            native: Native {
                valid: AtomicBool::new(true),
            },
            _temp: temp,
        }
    }
    pub fn request(&mut self, controller: u64, request: Request) -> Response {
        self.manager
            .request(controller, request, &self.native, &mut self.starter)
            .unwrap()
    }
    pub fn status(&mut self, controller: u64, id: Uuid) -> ConnectionStatus {
        let Response::Status(mut rows) = self.request(
            controller,
            Request::GetStatus {
                connection: Some(id),
            },
        ) else {
            panic!("status")
        };
        rows.pop().unwrap()
    }
    pub fn drive(&mut self, id: Uuid, ready: impl Fn(&State) -> bool) {
        let until = Instant::now() + Duration::from_secs(10);
        while !ready(&self.status(7, id).state) {
            assert!(Instant::now() < until, "SSH fixture deadline");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    pub fn connect(&mut self) -> (Uuid, TabId) {
        let profile = Uuid::new_v4();
        self.request(
            7,
            Request::Save(Input {
                id: profile,
                name: "fixture".into(),
                host: "fixture.invalid".into(),
                port: 22,
                user: "fixture".into(),
                password: Password::Set(Secret::new("fixture-secret".into()).unwrap()),
                key_file: None,
                remote_cwd: None,
            }),
        );
        let tab = TabId::new();
        let Response::Status(rows) = self.request(
            7,
            Request::Connect {
                profile,
                tab,
                cols: 80,
                rows: 24,
            },
        ) else {
            panic!("status")
        };
        (rows[0].connection, tab)
    }
    pub fn approve(&mut self, id: Uuid) {
        self.drive(id, |s| matches!(s, State::AwaitingHost(_)));
        let State::AwaitingHost(challenge) = self.status(7, id).state else {
            panic!("host")
        };
        self.request(
            7,
            Request::ConfirmHost {
                connection: id,
                challenge: challenge.challenge,
                decision: shellfox_ssh::TrustDecision::TrustAndSave,
            },
        );
        self.drive(id, |s| *s == State::Authenticating);
    }
    pub fn shutdown(&mut self) {
        self.manager.begin_shutdown();
        self.starter.allow_close = true;
        let until = Instant::now() + Duration::from_secs(10);
        while !self.manager.is_closed() {
            self.manager.poll(&self.native, &mut self.starter);
            assert!(Instant::now() < until, "SSH cleanup deadline");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
#[derive(Default)]
pub struct Starter {
    pub targets: HashMap<TabId, SourceConnectTarget>,
    pub identity: Option<Identity>,
    pub reserved: Arc<AtomicBool>,
    pub starts: usize,
    pub closes: usize,
    pub releases: usize,
    pub allow_close: bool,
    pub run_process: bool,
    pub child: Option<FakeProcess>,
}
impl OwnedRuntimeStarter for Starter {
    fn reserve_target(&mut self, _: u64, target: SourceConnectTarget) -> Result<()> {
        anyhow::ensure!(!self.targets.contains_key(&target.tab()), "Source occupied");
        self.targets.insert(target.tab(), target);
        self.reserved.store(true, Ordering::SeqCst);
        Ok(())
    }
    fn release_target(&mut self, target: SourceConnectTarget) -> Result<()> {
        anyhow::ensure!(
            self.targets.get(&target.tab()) == Some(&target) && self.identity.is_none(),
            "Source still owned"
        );
        self.targets.remove(&target.tab());
        self.releases += 1;
        Ok(())
    }
    fn runtime_is_current(&self, identity: Identity) -> Result<bool> {
        Ok(self.identity == Some(identity)
            && self.child.as_ref().is_none_or(FakeProcess::output_open))
    }
    fn start(
        &mut self,
        _: u64,
        target: SourceConnectTarget,
        launch: &shellfox_ssh::system::SshLaunchRequest,
        local: &BTreeMap<String, String>,
    ) -> Result<Identity> {
        anyhow::ensure!(
            self.targets.get(&target.tab()) == Some(&target),
            "Unreserved SSH source"
        );
        assert!(launch.authentication.saved_password.is_none());
        assert!(!local.values().any(|v| v == "fixture-secret"));
        if self.run_process {
            self.child = Some(FakeProcess::spawn(launch.args.last().unwrap(), local)?);
        }
        let identity = Identity {
            session_id: SessionId::new(),
            tab_id: target.tab(),
            generation: Generation::new(),
        };
        self.starts += 1;
        self.identity = Some(identity);
        Ok(identity)
    }
    fn close(&mut self, identity: Identity) -> Result<bool> {
        self.closes += 1;
        if !self.allow_close {
            return Ok(false);
        }
        anyhow::ensure!(self.identity == Some(identity), "Stale runtime cleanup");
        if let Some(child) = &mut self.child {
            child.close()?;
        }
        self.child = None;
        self.identity = None;
        Ok(true)
    }
}
