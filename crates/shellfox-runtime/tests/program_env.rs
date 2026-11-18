#![cfg(windows)]
use shellfox_core::{Generation, SessionId, TabId};
use shellfox_runtime::*;
use shellfox_terminal::{
    AskpassBroker, PtyCommand, PtyConfig, PtyOutput, PtySession, TrustedProgramEnv,
    TrustedProgramLease,
};
use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
struct Lease(PathBuf);
impl TrustedProgramLease for Lease {
    fn path(&self) -> &Path {
        &self.0
    }
    fn revalidate(&self) -> shellfox_terminal::program_env::ProgramLeaseResult {
        std::fs::metadata(&self.0).map(|_| ()).map_err(Into::into)
    }
}
struct Sink(AtomicU64);
impl LiveOutputSink for Sink {
    fn output(&self, _: Identity, _: u64, _: &[u8], _: Instant) -> Result<(), LiveOutputFailure> {
        self.0.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
}
struct Fixture {
    root: PathBuf,
    executable: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tmp")
            .join(format!("runtime-program-{}", Generation::new()));
        std::fs::create_dir_all(&root).unwrap();
        Self {
            root,
            executable: PathBuf::from(env!("CARGO_BIN_EXE_shellfox-runtime-fixture")),
        }
    }
    fn config(&self) -> PtyConfig {
        let mut config = PtyConfig::new(&self.root);
        config.supervisor = Some(self.executable.clone());
        config.command = Some(PtyCommand {
            program: self.executable.clone().into_os_string(),
            args: vec!["--child-program-env".into()],
        });
        config
    }
    fn capability(&self) -> TrustedProgramEnv {
        let broker = AskpassBroker::new(
            self.root.clone(),
            r"\\.\pipe\shellfox-rust-v1-11111111-1111-4111-8111-111111111111".into(),
            "a".repeat(64),
        )
        .unwrap();
        TrustedProgramEnv::ssh_askpass(
            Arc::new(Lease(self.executable.clone())),
            Arc::new(Lease(self.executable.clone())),
            broker,
        )
        .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
#[test]
fn trusted_fixture_env_reaches_only_owned_target_after_normal_isolation() {
    let fixture = Fixture::new();
    let cap = fixture.capability();
    assert_eq!(format!("{cap:?}"), "TrustedProgramEnv([redacted])");
    assert!(!format!("{cap:?}").contains(&"a".repeat(64)));
    let mut session = PtySession::spawn_program(fixture.config(), cap).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut output = Vec::new();
    while !String::from_utf8_lossy(&output).contains("ISOLATION_COLORS=true") {
        assert!(
            Instant::now() < deadline,
            "trusted fixture did not report validation"
        );
        match session.try_read() {
            Ok(Some(PtyOutput::Data(bytes))) => output.extend(bytes),
            _ => std::thread::sleep(Duration::from_millis(1)),
        }
    }
    assert!(String::from_utf8_lossy(&output).contains("TRUSTED_TARGET_ENV=true"));
    session.shutdown().unwrap();
}
#[test]
fn verified_program_and_helper_leases_remain_held_until_confirmed_owned_cleanup() {
    let fixture = Fixture::new();
    let lease = Arc::new(Lease(fixture.executable.clone()));
    let weak = Arc::downgrade(&lease);
    let broker = AskpassBroker::new(
        fixture.root.clone(),
        r"\\.\pipe\shellfox-rust-v1-11111111-1111-4111-8111-111111111111".into(),
        "a".repeat(64),
    )
    .unwrap();
    let cap = TrustedProgramEnv::ssh_askpass(lease.clone(), lease.clone(), broker).unwrap();
    drop(lease);
    let mut session = PtySession::spawn_program(fixture.config(), cap).unwrap();
    assert!(weak.upgrade().is_some());
    session.shutdown().unwrap();
    assert!(weak.upgrade().is_none());
}
#[test]
fn protected_user_env_and_wrong_target_are_rejected_before_any_program_launch() {
    let fixture = Fixture::new();
    for key in [
        "TERM",
        "COLORTERM",
        "SHELLFOX_SSH_ASKPASS_TOKEN",
        "shellfox_unapproved",
    ] {
        let mut config = fixture.config();
        config.env.insert(key.into(), "untrusted".into());
        let error = PtySession::spawn(config).unwrap_err();
        assert!(error.take_session().is_none());
    }
    for key in ["SSH_ASKPASS", "ssh_askpass_require", "DISPLAY"] {
        let mut config = fixture.config();
        config.env.insert(key.into(), "untrusted".into());
        let error = PtySession::spawn_program(config, fixture.capability()).unwrap_err();
        assert!(error.take_session().is_none());
    }
    let mut config = fixture.config();
    config.command.as_mut().unwrap().program =
        PathBuf::from(std::env::var_os("SystemRoot").unwrap())
            .join("System32/cmd.exe")
            .into_os_string();
    let error = PtySession::spawn_program(config, fixture.capability()).unwrap_err();
    assert!(error.take_session().is_none());
}
#[test]
fn runtime_program_overload_keeps_the_same_activity_and_pty_owner() {
    let fixture = Fixture::new();
    let sink = Arc::new(Sink(AtomicU64::new(0)));
    let runtime = Runtime::start_program_with_hooks(
        Identity {
            session_id: SessionId::new(),
            tab_id: TabId::new(),
            generation: Generation::new(),
        },
        fixture.config(),
        fixture.capability(),
        RuntimeHooks {
            live_output: Some(sink.clone()),
        },
    )
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while sink.0.load(Ordering::Relaxed) == 0 {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(runtime.source_kind(), SourceKind::OwnedPty);
    assert_eq!(runtime.live_output_status(), LiveOutputStatus::Active);
    runtime
        .try_send(Request {
            id: RequestId(1),
            command: Command::RetryClose,
        })
        .unwrap();
    while runtime.phase() != Phase::Closed {
        while runtime.try_recv().is_ok() {}
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
}
