#[path = "support/ssh_fixture.rs"]
mod fixture;
use fixture::*;
use shellfox_service::ssh::*;
use std::{
    io::Read,
    sync::atomic::Ordering,
    time::{Duration, Instant},
};

#[test]
fn pending_source_reservation_blocks_local_start_until_cancel_cleanup() {
    let mut f = Fixture::new(false);
    let (id, tab) = f.connect();
    let target = f.manager.source_target(tab).unwrap();
    assert_eq!(target.connection(), id);
    assert_eq!(f.starter.targets.get(&tab), Some(&target));
    assert!(f.manager.ensure_local_start_allowed(tab).is_err());
    f.drive(id, |s| matches!(s, State::AwaitingHost(_)));
    f.manager.detach_controller(7);
    assert!(f.manager.ensure_local_start_allowed(tab).is_err());
    f.shutdown();
    assert_eq!(f.starter.starts, 0);
    assert_eq!(f.starter.releases, 1);
    assert!(f.manager.ensure_local_start_allowed(tab).is_ok());
    assert!(f
        .manager
        .request(7, Request::List, &f.native, &mut f.starter)
        .is_err());
}
#[test]
fn actual_fake_process_proof_ready_detach_adoption_and_explicit_revocation() {
    let mut f = Fixture::new(true);
    let (id, tab) = f.connect();
    f.approve(id);
    assert_eq!(f.status(7, id).state, State::Authenticating);
    let identity = f.starter.identity.unwrap();
    let until = Instant::now() + Duration::from_secs(10);
    let mut bytes = Vec::new();
    while f.status(7, id).state != State::Ready {
        let chunk = f
            .starter
            .child
            .as_ref()
            .unwrap()
            .read(until.saturating_duration_since(Instant::now()))
            .unwrap();
        bytes.extend_from_slice(&chunk);
        assert!(bytes.len() <= 65536);
        for part in chunk.chunks(3) {
            f.manager.observe_output(identity, part);
        }
    }
    assert!(!bytes
        .windows(b"fixture-secret".len())
        .any(|b| b == b"fixture-secret"));
    assert!(f.starter.child.as_mut().unwrap().is_running().unwrap());
    assert_eq!(f.status(7, id).state, State::Ready);
    f.manager.detach_controller(7);
    f.native.valid.store(false, Ordering::SeqCst);
    f.manager.poll(&f.native, &mut f.starter);
    assert_eq!(f.starter.closes, 0);
    assert_eq!(f.starter.identity, Some(identity));
    assert!(f.starter.child.as_mut().unwrap().is_running().unwrap());
    assert!(f.manager.source_target(tab).is_some());
    f.native.valid.store(true, Ordering::SeqCst);
    assert_eq!(f.status(8, id).state, State::Ready);
    f.request(8, Request::AdoptReady { connection: id });
    assert_eq!(f.starter.starts, 1);
    assert_eq!(f.starter.closes, 0);
    assert_eq!(f.starter.identity, Some(identity));
    f.manager.revoke_native_capability(8);
    f.manager.poll(&f.native, &mut f.starter);
    assert_eq!(f.starter.identity, Some(identity));
    assert_eq!(f.starter.releases, 0);
    assert!(f.manager.source_target(tab).is_some());
    f.shutdown();
    assert!(f.starter.identity.is_none());
    assert_eq!(f.starter.releases, 1);
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
    std::io::stdout().flush().unwrap();
    // Remain alive like a remote shell until the fixture owner closes stdin.
    std::io::stdin()
        .take(1)
        .read_to_end(&mut Vec::new())
        .unwrap();
}
