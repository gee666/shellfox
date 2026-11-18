use super::*;
use crate::Service;
use shellfox_core::{CreateSession, ProfileId, SessionId, SettingsAdapter, SettingsShell};
use shellfox_ssh::{
    system::SshPrograms, ProcessOutput, ProcessRequest, ProcessRunner, StoragePolicy,
};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};
struct Programs(HashSet<(SshProgramRole, PathBuf)>);
impl SshProgramPolicy for Programs {
    fn validate_current(&self, role: SshProgramRole, path: &Path) -> Result<()> {
        anyhow::ensure!(
            self.0.contains(&(role, path.to_owned())) && path.is_file(),
            "Unapproved SSH fixture program"
        );
        Ok(())
    }
}
struct Probe;
impl ProcessRunner for Probe {
    fn run(&self, _: &ProcessRequest) -> shellfox_ssh::Result<ProcessOutput> {
        Ok(ProcessOutput{success:true,stdout:b"fixture.invalid ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAINdamAGCsQq31Uv+08lkBzoO4XLz2qYjJa8CGmj3B1Ea\n".to_vec(),stderr:vec![]})
    }
}
struct Imports;
impl NativePlatform for Imports {
    fn import_sessions(&self) -> Result<Vec<shellfox_ssh::ImportSession>> {
        Ok(vec![])
    }
}
struct Fixture {
    service: Service,
    _dirs: Vec<Arc<PrivateDirectory>>,
    _temp: tempfile::TempDir,
}
fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .into()
}
impl Fixture {
    fn new(program: PathBuf, supervisor: PathBuf) -> Self {
        Self::with_askpass(program.clone(), supervisor, program)
    }
    fn with_askpass(program: PathBuf, supervisor: PathBuf, askpass: PathBuf) -> Self {
        std::fs::create_dir_all(root().join("tmp")).unwrap();
        let temp = tempfile::Builder::new()
            .prefix("service-ssh-native-")
            .tempdir_in(root().join("tmp"))
            .unwrap();
        let policy = if cfg!(windows) {
            StoragePolicy::with_windows_helper(
                root().join("tmp/tailscale/win32-x64/shellfox-tailscale.exe"),
            )
        } else {
            StoragePolicy::default()
        };
        let data =
            Arc::new(PrivateDirectory::open(temp.path().join("data"), policy.clone()).unwrap());
        let endpoint = Arc::new(
            PrivateDirectory::open(data.path().join("rust-service-v1"), policy.clone()).unwrap(),
        );
        let directory =
            Arc::new(PrivateDirectory::open(endpoint.path().join("ssh"), policy).unwrap());
        let adapter = Arc::new(
            SystemSshAdapter::new(
                SshPrograms {
                    ssh: program.clone(),
                    keyscan: program.clone(),
                },
                Arc::new(Probe),
                directory.clone(),
            )
            .unwrap(),
        );
        let mut service = Service::open_owned(data.path(), &supervisor).unwrap();
        service
            .install_ssh_native(NativeSshInstall {
                directory: directory.clone(),
                adapter,
                imports: Arc::new(Imports),
                askpass: askpass.clone(),
                program: Arc::new(Programs(HashSet::from([
                    (SshProgramRole::Ssh, program.clone()),
                    (SshProgramRole::Keyscan, program.clone()),
                    (SshProgramRole::Askpass, askpass),
                ]))),
            })
            .unwrap();
        Self {
            service,
            _dirs: vec![data, endpoint, directory],
            _temp: temp,
        }
    }
    fn profile(&mut self) -> Uuid {
        self.service.admit_ssh_native(7).unwrap();
        let profile = super::support::input();
        let id = profile.id;
        self.service
            .handle_ssh_native(Request::Save(profile), 7)
            .unwrap();
        id
    }
    fn status(&mut self, id: Uuid) -> ConnectionStatus {
        let Response::Status(rows) = self
            .service
            .handle_ssh_native(
                Request::GetStatus {
                    connection: Some(id),
                },
                7,
            )
            .unwrap()
        else {
            panic!("status")
        };
        rows[0].clone()
    }
    fn state(&mut self, id: Uuid) -> State {
        self.status(id).state
    }
    fn drive(&mut self, id: Uuid, test: impl Fn(&State) -> bool) {
        let until = Instant::now() + Duration::from_secs(15);
        while !test(&self.state(id)) {
            assert!(
                !matches!(self.state(id), State::Failed { .. }),
                "native SSH failed: {:?}",
                self.state(id)
            );
            self.service.pump();
            assert!(
                Instant::now() < until,
                "Native SSH fixture deadline: {:?}, phases {:?}",
                self.state(id),
                self.service
                    .terminals
                    .values()
                    .map(|t| t.runtime.phase())
                    .collect::<Vec<_>>()
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    fn stop(&mut self) {
        self.service.begin_stop_ssh_native();
        let until = Instant::now() + Duration::from_secs(15);
        while !self.service.stop_ssh_native().unwrap() {
            assert!(Instant::now() < until, "Native SSH cleanup deadline");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
#[test]
fn native_owner_grants_root_binding_and_existing_history_are_real() {
    let executable = std::env::current_exe().unwrap();
    let mut f = Fixture::new(executable.clone(), executable);
    assert!(f.service.handle_ssh_native(Request::List, 999).is_err());
    let profile = f.profile();
    let creation = shellfox_host::SessionCreationFacts::new(
        SettingsAdapter::EmbeddedPty,
        SettingsShell::WindowsPowershell,
    );
    let snapshot = f
        .service
        .host
        .create_session_with_facts(
            CreateSession {
                request_id: SessionId::new(),
                title: Some("Original local intent".into()),
                cwd: f.service.isolation.to_string_lossy().into_owned(),
                profile_id: ProfileId::new("windows-powershell").unwrap(),
            },
            creation.clone(),
        )
        .unwrap();
    let session = snapshot.sessions[0].id;
    let tab = snapshot.sessions[0].tabs[0].id;
    let Response::Status(rows) = f
        .service
        .handle_ssh_native(
            Request::Connect {
                profile,
                tab,
                cols: 80,
                rows: 24,
            },
            7,
        )
        .unwrap()
    else {
        panic!("status")
    };
    f.drive(rows[0].connection, |s| matches!(s, State::AwaitingHost(_)));
    assert!(
        f.status(rows[0].connection).runtime.is_none(),
        "pending metadata is not a live identity"
    );
    f.service.admit_ssh_native(8).unwrap();
    let Response::Status(foreign) = f
        .service
        .handle_ssh_native(
            Request::GetStatus {
                connection: Some(rows[0].connection),
            },
            8,
        )
        .unwrap()
    else {
        panic!("scoped status")
    };
    assert!(
        foreign.is_empty(),
        "pending connection identity must remain originating-controller scoped"
    );
    assert_eq!(
        f.service
            .host
            .session_creation_facts()
            .unwrap()
            .get(&session),
        Some(&creation)
    );
    assert!(f.service.ensure_ssh_local_start_allowed(tab).is_err());
    assert!(f.service.attach(tab, 8, false).is_err());
    assert_eq!(f.service.active_terminals(), 0);
    f.stop();
}
#[test]
fn open_profile_uses_original_none_facts_and_retry_does_not_make_another_source() {
    let executable = std::env::current_exe().unwrap();
    let mut f = Fixture::new(executable.clone(), executable);
    let profile = f.profile();
    let input = OpenProfile {
        request_id: SessionId::new(),
        profile,
        cols: 80,
        rows: 24,
    };
    let crate::native::NativeResponse::SshOpened(opened) = f
        .service
        .execute_native(crate::native::NativeAction::SshOpen(input.clone()), 7)
        .unwrap()
    else {
        panic!("opened profile")
    };
    let tab = opened.connection.tab;
    assert!(f.service.host.session_creation_facts().unwrap().is_empty());
    assert!(f.service.ensure_ssh_local_start_allowed(tab).is_err());
    let crate::native::NativeResponse::SshOpened(retry) = f
        .service
        .execute_native(crate::native::NativeAction::SshOpen(input), 7)
        .unwrap()
    else {
        panic!("opened profile")
    };
    assert_eq!(retry.connection.connection, opened.connection.connection);
    assert_eq!(f.service.host.snapshot().unwrap().sessions.len(), 1);
    assert_eq!(f.service.active_terminals(), 0);
    f.stop();
}
#[path = "ssh_native_live_tests.rs"]
mod live;
