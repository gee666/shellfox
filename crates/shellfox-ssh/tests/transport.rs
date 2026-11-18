mod common;
use common::*;
use shellfox_ssh::{
    pty::{remote_environment, terminal_modes, RemotePty},
    transport::*,
    Error, KnownHosts, Result, StoredProfile, TrustDecision,
};
use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};
struct Factory {
    key: Vec<u8>,
    authenticated: Arc<AtomicBool>,
}
struct Pending {
    key: Vec<u8>,
    authenticated: Arc<AtomicBool>,
}
impl TransportFactory for Factory {
    fn handshake(&self, target: &ConnectionTarget) -> Result<Box<dyn PendingTransport>> {
        assert_eq!(target.host, "example.test");
        assert_eq!(target.port, 22);
        Ok(Box::new(Pending {
            key: self.key.clone(),
            authenticated: self.authenticated.clone(),
        }))
    }
}
impl PendingTransport for Pending {
    fn host_key(&self) -> &[u8] {
        &self.key
    }
    fn authenticate_and_open(
        self: Box<Self>,
        _: &StoredProfile,
        pty: &RemotePty,
        environment: &BTreeMap<String, String>,
    ) -> Result<Box<dyn SshSession>> {
        self.authenticated.store(true, Ordering::SeqCst);
        assert_eq!(pty.term, "xterm-256color");
        assert_eq!(
            environment.get("COLORTERM").map(String::as_str),
            Some("truecolor")
        );
        // A transport error must never be turned into fake PTY success.
        Err(Error::Process)
    }
}
#[test]
fn driver_never_authenticates_untrusted_or_changed_hosts() {
    let fixture = Fixture::new();
    let store = shellfox_ssh::ProfileStore::new(fixture.directory.clone());
    let input = profile("one");
    let id = input.id;
    store.save(input).unwrap();
    let profile = store.get_for_connection(id).unwrap().unwrap();
    let known = KnownHosts::new(fixture.directory.clone());
    let authenticated = Arc::new(AtomicBool::new(false));
    let factory = Factory {
        key: public_key(),
        authenticated: authenticated.clone(),
    };
    let denied = connect(
        &factory,
        &known,
        &profile,
        RemotePty::default(),
        [],
        |_, _, _| Ok(TrustDecision::Reject),
    );
    assert!(matches!(denied, Err(Error::HostNotTrusted)));
    assert!(!authenticated.load(Ordering::SeqCst));
    let failed = connect(
        &factory,
        &known,
        &profile,
        RemotePty::default(),
        [],
        |_, _, _| Ok(TrustDecision::TrustAndSave),
    );
    assert!(matches!(failed, Err(Error::Process)));
    assert!(authenticated.load(Ordering::SeqCst));
    authenticated.store(false, Ordering::SeqCst);
    let changed = Factory {
        key: public_key(),
        authenticated: authenticated.clone(),
    };
    assert!(matches!(
        connect(
            &changed,
            &known,
            &profile,
            RemotePty::default(),
            [],
            |_, _, _| panic!("changed")
        ),
        Err(Error::HostKeyChanged)
    ));
    assert!(!authenticated.load(Ordering::SeqCst));
    assert!(matches!(
        connect(
            &UnavailableTransport,
            &known,
            &profile,
            RemotePty::default(),
            [],
            |_, _, _| panic!("unavailable")
        ),
        Err(Error::AdapterUnavailable)
    ));
}
#[test]
fn pty_modes_and_locale_are_ported_without_forwarding_host_secrets() {
    let modes = terminal_modes();
    assert_eq!(modes.last(), Some(&0));
    let decoded: BTreeMap<_, _> = modes[..modes.len() - 1]
        .chunks_exact(5)
        .map(|b| (b[0], u32::from_be_bytes(b[1..].try_into().unwrap())))
        .collect();
    assert_eq!(decoded.get(&42), Some(&1));
    assert_eq!(decoded.get(&3), Some(&127));
    assert!(RemotePty::new(0, 24).is_err());
    assert_eq!(RemotePty::new(132, 43).unwrap().cols, 132);
    let env = remote_environment([
        ("LANG", "C.UTF-8"),
        ("LC_CTYPE", "en_US.UTF-8"),
        ("LC_ALL", "bad\nvalue"),
        ("NO_COLOR", "1"),
        ("TOKEN", "secret"),
    ]);
    assert_eq!(env.len(), 4);
    assert_eq!(env.get("TERM").map(String::as_str), Some("xterm-256color"));
    assert!(!env.contains_key("TOKEN"));
    assert!(!env.contains_key("NO_COLOR"));
}
