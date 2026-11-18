use super::*;
use std::{
    io::Read,
    process::{Child, Command, Stdio},
};
#[test]
fn private_askpass_ipc_rejects_wrong_capability_and_returns_no_password_in_environment() {
    let fixture = Fixture::new();
    let (tx, _rx) = std::sync::mpsc::sync_channel(8);
    let work = Work {
        connection: Uuid::new_v4(),
        events: tx,
        cancel: CancellationToken::new(),
        lost: CancellationToken::new(),
    };
    let mut broker = broker::Broker::start(
        fixture.directory.path(),
        &std::env::current_exe().unwrap(),
        work,
        Some(Secret::new("fixture-secret".into()).unwrap()),
        None,
    )
    .unwrap();
    assert!(!broker.environment.values().any(|v| v == "fixture-secret"));
    let endpoint = &broker.environment["SHELLFOX_SSH_ASKPASS_ENDPOINT"];
    let token = &broker.environment["SHELLFOX_SSH_ASKPASS_TOKEN"];
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()
        .unwrap();
    runtime.block_on(async {
        assert!(broker::request(
            endpoint,
            fixture.directory.path(),
            Secret::new("wrong".into()).unwrap(),
            "fixture@fixture.invalid's password: "
        )
        .await
        .is_err());
        let answer = broker::request(
            endpoint,
            fixture.directory.path(),
            Secret::new(token.clone()).unwrap(),
            "fixture@fixture.invalid's password: ",
        )
        .await
        .unwrap();
        assert!(answer.expose() == "fixture-secret");
    });
    broker.stop().unwrap();
}
struct FakeProcessStarter {
    base: Starter,
    child: Option<Child>,
}
impl OwnedRuntimeStarter for FakeProcessStarter {
    fn reserve_target(&mut self, controller: u64, target: SourceConnectTarget) -> Result<()> {
        self.base.reserve_target(controller, target)
    }
    fn release_target(&mut self, target: SourceConnectTarget) -> Result<()> {
        self.base.release_target(target)
    }
    fn runtime_is_current(&self, identity: shellfox_runtime::Identity) -> Result<bool> {
        self.base.runtime_is_current(identity)
    }
    fn start(
        &mut self,
        controller: u64,
        target: SourceConnectTarget,
        launch: &shellfox_ssh::system::SshLaunchRequest,
        local: &std::collections::BTreeMap<String, String>,
    ) -> Result<shellfox_runtime::Identity> {
        let identity = self.base.start(controller, target, launch, local)?;
        // Test-only trusted program, no real SSH/keyscan/network/account access.
        // Production uses the injected existing service Runtime/PtyCommand supervisor.
        let child = Command::new(std::env::current_exe()?)
            .args([
                "--exact",
                "ssh::tests::broker_tests::fixture_ssh_process",
                "--nocapture",
            ])
            .envs(local)
            .env("SHELLFOX_SSH_FIXTURE_COMMAND", launch.args.last().unwrap())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        self.child = Some(child);
        Ok(identity)
    }
    fn close(&mut self, identity: shellfox_runtime::Identity) -> Result<bool> {
        if let Some(mut child) = self.child.take() {
            if child.try_wait()?.is_none() {
                child.kill()?;
            }
            child.wait()?;
        }
        self.base.allow_close = true;
        self.base.close(identity)
    }
}
impl Drop for FakeProcessStarter {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
#[test]
fn local_fake_ssh_process_uses_private_broker_and_real_output_proof_not_spawn() {
    let fixture = Fixture::new();
    let mut manager = fixture.manager();
    let validator = Validator(AtomicBool::new(true));
    let mut starter = FakeProcessStarter {
        base: Starter::default(),
        child: None,
    };
    let id = connect(&mut manager, &validator, &mut starter);
    approve(&mut manager, &validator, &mut starter, id);
    drive(&mut manager, &validator, &mut starter, |m| {
        matches!(m.connections[&id].status.state, State::Authenticating)
    });
    let identity = starter.base.identity.unwrap();
    assert_eq!(manager.connections[&id].status.state, State::Authenticating);
    let child = starter.child.as_mut().unwrap();
    let deadline = Instant::now() + Duration::from_secs(8);
    while child.try_wait().unwrap().is_none() {
        assert!(Instant::now() < deadline, "fake SSH process deadline");
        std::thread::sleep(Duration::from_millis(10));
    }
    let status = child.wait().unwrap();
    assert!(status.success(), "synthetic SSH process failed");
    let mut output = Vec::new();
    child
        .stdout
        .take()
        .unwrap()
        .take(65536)
        .read_to_end(&mut output)
        .unwrap();
    assert!(!output
        .windows(b"fixture-secret".len())
        .any(|b| b == b"fixture-secret"));
    let mut wrong = identity;
    wrong.generation = shellfox_core::Generation::new();
    manager.observe_output(wrong, &output);
    assert_eq!(manager.connections[&id].status.state, State::Authenticating);
    for part in output.chunks(3) {
        manager.observe_output(identity, part);
    }
    assert_eq!(manager.connections[&id].status.state, State::Ready);
    manager
        .request(
            7,
            Request::Disconnect { connection: id },
            &validator,
            &mut starter,
        )
        .unwrap();
    drive(
        &mut manager,
        &validator,
        &mut starter,
        SshManager::is_closed,
    );
}
#[test]
fn fixture_ssh_process() {
    let Ok(command) = std::env::var("SHELLFOX_SSH_FIXTURE_COMMAND") else {
        return;
    };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()
        .unwrap();
    let secret = runtime
        .block_on(askpass_from_environment(
            "fixture@fixture.invalid's password: ",
        ))
        .unwrap();
    assert!(secret.expose() == "fixture-secret");
    drop(secret);
    let marker = command
        .split("printf '\\033")
        .nth(1)
        .and_then(|s| s.split("\\007'").next())
        .unwrap();
    use std::io::Write;
    std::io::stdout()
        .write_all(format!("\x1b{marker}\x07").as_bytes())
        .unwrap();
}
