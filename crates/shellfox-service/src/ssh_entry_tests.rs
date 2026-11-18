use super::*;
use std::{
    io::Read,
    path::PathBuf,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};
use zeroize::Zeroizing;
struct Owned(Child);
impl Drop for Owned {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
        }
        let _ = self.0.wait();
    }
}
fn invoke(
    program: &std::path::Path,
    env: &std::collections::BTreeMap<String, String>,
    args: &[&str],
) -> (i32, Zeroizing<Vec<u8>>, Zeroizing<Vec<u8>>) {
    let mut command = Command::new(program);
    command
        .args(args)
        .env_remove("SHELLFOX_SSH_ASKPASS_ENDPOINT")
        .env_remove("SHELLFOX_SSH_ASKPASS_DIRECTORY")
        .env_remove("SHELLFOX_SSH_ASKPASS_TOKEN")
        .envs(env)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = Owned(command.spawn().expect("owned configured askpass fixture"));
    let out = child.0.stdout.take().unwrap();
    let err = child.0.stderr.take().unwrap();
    let collect = |stream: Box<dyn Read + Send>| {
        std::thread::spawn(move || {
            let mut bytes = Zeroizing::new(Vec::new());
            stream.take(8193).read_to_end(&mut bytes).unwrap();
            bytes
        })
    };
    let stdout = collect(Box::new(out));
    let stderr = collect(Box::new(err));
    let until = Instant::now() + Duration::from_secs(8);
    let status = loop {
        if let Some(status) = child.0.try_wait().unwrap() {
            break status;
        }
        assert!(Instant::now() < until, "owned callback fixture deadline");
        std::thread::sleep(Duration::from_millis(10));
    };
    (
        status.code().unwrap_or(1),
        stdout.join().unwrap(),
        stderr.join().unwrap(),
    )
}
#[test]
#[ignore = "requires explicitly configured private rebuilt CLI artifact; synthetic broker answer only"]
fn configured_early_entry_uses_owned_broker_and_errors_are_silent() {
    let program = std::env::var_os("SHELLFOX_SSH_TEST_ASKPASS_PROGRAM")
        .map(PathBuf::from)
        .expect("explicit configured test helper, never inferred from current_exe");
    assert!(program.is_absolute() && program.is_file());
    let fixture = Fixture::new();
    let (tx, rx) = std::sync::mpsc::sync_channel(8);
    let answers = std::thread::spawn(move || {
        while let Ok(event) = rx.recv_timeout(Duration::from_secs(8)) {
            if let WorkEvent::Prompt { reply, .. } = event {
                let _ = reply.try_send(Some(
                    Secret::new("synthetic-captured-answer".into()).unwrap(),
                ));
            }
        }
    });
    let work = Work {
        connection: Uuid::new_v4(),
        events: tx,
        cancel: CancellationToken::new(),
        lost: CancellationToken::new(),
    };
    let mut broker =
        broker::Broker::start(fixture.directory.path(), &program, work, None, None).unwrap();
    let (code, out, err) = invoke(
        &program,
        &broker.environment,
        &["fixture@fixture.invalid's password: "],
    );
    assert_eq!(code, 0);
    assert!(out.as_slice() == b"synthetic-captured-answer\n");
    assert!(err.is_empty());
    let mut invalid = broker.environment.clone();
    invalid.insert("SHELLFOX_SSH_ASKPASS_TOKEN".into(), "f".repeat(64));
    let (code, out, err) = invoke(&program, &invalid, &["synthetic prompt must not be dumped"]);
    assert_eq!(code, 1);
    assert!(out.is_empty());
    assert!(err.is_empty());
    let (code, out, err) = invoke(&program, &broker.environment, &[]);
    assert_eq!(code, 1);
    assert!(out.is_empty());
    assert!(err.is_empty());
    for flag in ["--shellfox-pty-supervisor", "--shellfox-raw-supervisor"] {
        let (code, out, err) = invoke(&program, &broker.environment, &[flag]);
        assert_eq!(code, 0);
        assert!(out.as_slice() == b"synthetic-captured-answer\n");
        assert!(err.is_empty());
    }
    let partial =
        std::collections::BTreeMap::from([("SHELLFOX_SSH_ASKPASS_TOKEN".into(), "0".repeat(64))]);
    let (code, out, err) = invoke(&program, &partial, &["partial callback"]);
    assert_eq!(code, 1);
    assert!(out.is_empty());
    assert!(err.is_empty());
    for args in [
        &["extra prompt", "extra argument"][..],
        &[
            "--shellfox-pty-supervisor",
            "Local\\ShellfoxPty-00000000000000000000000000000000-GO",
            "Local\\ShellfoxPty-00000000000000000000000000000000-READY",
        ][..],
        &[
            "--shellfox-raw-supervisor",
            "Local\\ShellfoxPty-00000000000000000000000000000000-GO",
            "Local\\ShellfoxPty-00000000000000000000000000000000-READY",
        ][..],
    ] {
        let (code, out, err) = invoke(&program, &broker.environment, args);
        assert_eq!(code, 1);
        assert!(out.is_empty());
        assert!(err.is_empty());
    }
    broker.stop().unwrap();
    answers.join().unwrap();
    let (code, out, err) = invoke(&program, &std::collections::BTreeMap::new(), &["--help"]);
    assert_eq!(code, 0);
    assert!(!out.is_empty());
    assert!(err.is_empty());
    let (code, out, err) = invoke(
        &program,
        &std::collections::BTreeMap::new(),
        &["--shellfox-pty-supervisor"],
    );
    assert_eq!(code, 1);
    assert!(out.is_empty());
    assert!(
        !err.is_empty(),
        "normal invalid supervisor diagnostics must be unchanged"
    );
}
#[test]
fn early_entry_source_order_is_supervisors_then_callback_then_normal_startup() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_owned();
    for (source, normal) in [
        (
            std::fs::read_to_string(root.join("shellfox-cli/src/main.rs")).unwrap(),
            "match run()",
        ),
        (
            std::fs::read_to_string(root.join("shellfox-desktop/src/main.rs")).unwrap(),
            "Options::parse(",
        ),
    ] {
        let dispatch = source
            .find("shellfox_service::ssh::dispatch_early_entry(&arguments)")
            .unwrap();
        let normal = source.find(normal).unwrap();
        assert!(dispatch < normal);
        assert!(!source.contains("shellfox_terminal::supervisor_entry()"));
        assert!(source.contains("i32::from_ne_bytes(code.to_ne_bytes())"));
    }
}
