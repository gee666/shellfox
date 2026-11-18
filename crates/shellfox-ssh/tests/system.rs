mod common;
use base64::{engine::general_purpose::STANDARD, Engine};
use common::*;
use shellfox_ssh::{
    pty::RemotePty, system::*, Error, PasswordUpdate, ProcessOutput, ProcessRequest, ProcessRunner,
    ProfileStore, Secret, TrustDecision,
};
use std::{
    fs,
    sync::{Arc, Mutex},
};
struct Probe {
    output: Vec<u8>,
    calls: Mutex<usize>,
}
impl ProcessRunner for Probe {
    fn run(&self, request: &ProcessRequest) -> shellfox_ssh::Result<ProcessOutput> {
        *self.calls.lock().unwrap() += 1;
        assert!(request.args.contains(&"--".into()));
        assert!(!format!("{request:?}").contains("stored-fixture-password"));
        Ok(ProcessOutput {
            success: true,
            stdout: self.output.clone(),
            stderr: vec![],
        })
    }
}
#[test]
fn prepare_only_pins_trust_and_keeps_credentials_out_of_command() {
    let fixture = Fixture::new();
    let store = ProfileStore::new(fixture.directory.clone());
    let mut input = profile("system");
    let id = input.id;
    input.password = PasswordUpdate::Set(Secret::new("stored-fixture-password".into()).unwrap());
    input.remote_cwd = Some("~/x'; touch BAD; '".into());
    store.save(input).unwrap();
    let profile = store.get_for_connection(id).unwrap().unwrap();
    let key = public_key();
    let probe = Arc::new(Probe {
        output: format!("example.test ssh-ed25519 {}\n", STANDARD.encode(&key)).into_bytes(),
        calls: Mutex::new(0),
    });
    // This existing executable is only a configured identity, never run by prepare_launch.
    let executable = std::env::current_exe().unwrap();
    let adapter = SystemSshAdapter::new(
        SshPrograms {
            ssh: executable.clone(),
            keyscan: executable,
        },
        probe.clone(),
        fixture.directory.clone(),
    )
    .unwrap();
    assert!(matches!(
        adapter.prepare_launch(&profile, RemotePty::default(), [], |_, _, _| Ok(
            TrustDecision::Reject
        )),
        Err(Error::HostNotTrusted)
    ));
    let mut request = adapter
        .prepare_launch(&profile, RemotePty::default(), [], |_, _, _| {
            Ok(TrustDecision::TrustAndSave)
        })
        .unwrap();
    assert!(!request
        .expected_authentication
        .observe(b"process spawn succeeded"));
    let args = request
        .args
        .iter()
        .map(|s| s.to_string_lossy())
        .collect::<Vec<_>>()
        .join(" ");
    for setting in [
        "StrictHostKeyChecking=yes",
        "UpdateHostKeys=no",
        "IdentityFile=none",
        "ProxyCommand=none",
        "PermitLocalCommand=no",
        "HostKeyAlias=shellfox-pin-",
    ] {
        assert!(args.contains(setting), "{setting}");
    }
    assert!(!args.contains("stored-fixture-password"));
    assert!(!format!("{request:?}").contains("shellfox-auth="));
    assert!(!format!("{:?}", request.environment).contains("stored-fixture-password"));
    assert_eq!(
        request.authentication.saved_password.unwrap().expose(),
        "stored-fixture-password"
    );
    assert!(args.contains("'x'\\''; touch BAD; '\\'''"));
    assert!(args.contains("printf '\\033]777;shellfox-auth="));
    let pins: Vec<_> = fs::read_dir(&fixture.path)
        .unwrap()
        .filter_map(|e| {
            let p = e.unwrap().path();
            p.extension().is_some_and(|s| s == "hosts").then_some(p)
        })
        .collect();
    assert_eq!(pins.len(), 1);
    assert!(fs::read_to_string(&pins[0])
        .unwrap()
        .contains(&STANDARD.encode(&key)));
    adapter
        .prepare_launch(&profile, RemotePty::default(), [], |_, _, _| {
            panic!("known")
        })
        .unwrap();
    assert_eq!(*probe.calls.lock().unwrap(), 3);
}
