mod common;
use common::*;
use shellfox_ssh::{Error, KnownHosts, TrustDecision, TrustStatus};
use std::{
    fs,
    sync::{Arc, Barrier},
};

#[test]
fn unknown_requires_explicit_approval_and_changed_never_prompts() {
    let fixture = Fixture::new();
    let known = KnownHosts::new(fixture.directory.clone());
    let key = public_key();
    assert!(matches!(
        known.inspect("host", 22, &key).unwrap(),
        TrustStatus::Unknown(_)
    ));
    assert_eq!(
        known.verify("host", 22, &key, |_, _, _| Ok(TrustDecision::Reject)),
        Err(Error::HostNotTrusted)
    );
    assert!(!fixture.path.join("ssh-known-hosts.json").exists());
    known
        .verify("host", 22, &key, |host, port, info| {
            assert_eq!((host, port), ("host", 22));
            assert_eq!(info.key_type, "ssh-ed25519");
            assert!(info.fingerprint.starts_with("SHA256:"));
            Ok(TrustDecision::TrustAndSave)
        })
        .unwrap();
    known
        .verify("HOST", 22, &key, |_, _, _| panic!("known must not prompt"))
        .unwrap();
    assert!(matches!(
        known.inspect("host", 2200, &key).unwrap(),
        TrustStatus::Unknown(_)
    ));
    let before = fs::read(fixture.path.join("ssh-known-hosts.json")).unwrap();
    assert_eq!(
        known.verify("host", 22, &public_key(), |_, _, _| panic!(
            "changed must not prompt"
        )),
        Err(Error::HostKeyChanged)
    );
    assert_eq!(
        fs::read(fixture.path.join("ssh-known-hosts.json")).unwrap(),
        before
    );
    assert!(shellfox_ssh::host_key_info(&[0, 0, 0, 3, b'a', b'b', b'c']).is_err());
    assert_eq!(
        shellfox_ssh::host_key_info(&vec![0; 65537]),
        Err(Error::Limit)
    );
}
#[test]
fn concurrent_approvals_recheck_and_never_replace_conflicting_trust() {
    let fixture = Fixture::new();
    let known = KnownHosts::new(fixture.directory.clone());
    let barrier = Arc::new(Barrier::new(2));
    let keys = [public_key(), public_key()];
    let results = std::thread::scope(|s| {
        let handles: Vec<_> = keys
            .iter()
            .map(|key| {
                s.spawn(|| {
                    known.verify("host", 22, key, |_, _, _| {
                        barrier.wait();
                        Ok(TrustDecision::TrustAndSave)
                    })
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().unwrap())
            .collect::<Vec<_>>()
    });
    assert_eq!(
        results.iter().filter(|r| r.is_ok()).count(),
        1,
        "{results:?}"
    );
    assert_eq!(
        results
            .iter()
            .filter(|r| **r == Err(Error::HostKeyChanged))
            .count(),
        1
    );
    std::thread::scope(|s| {
        for host in ["a", "b", "c"] {
            let known = &known;
            s.spawn(move || {
                known
                    .verify(host, 22, &public_key(), |_, _, _| {
                        Ok(TrustDecision::TrustAndSave)
                    })
                    .unwrap()
            });
        }
    });
    let json: serde_json::Value =
        serde_json::from_slice(&fs::read(fixture.path.join("ssh-known-hosts.json")).unwrap())
            .unwrap();
    assert_eq!(json["hosts"].as_object().unwrap().len(), 4);
}
#[test]
fn authority_is_rechecked_after_confirmation_before_persisting_trust() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let fixture = Fixture::new();
    let known = KnownHosts::new(fixture.directory.clone());
    let authority = AtomicBool::new(true);
    let result = known.verify_guarded(
        "host",
        22,
        &public_key(),
        |_, _, _| {
            authority.store(false, Ordering::SeqCst);
            Ok(TrustDecision::TrustAndSave)
        },
        || {
            if authority.load(Ordering::SeqCst) {
                Ok(())
            } else {
                Err(Error::Permissions)
            }
        },
    );
    assert_eq!(result, Err(Error::Permissions));
    assert!(!fixture.path.join("ssh-known-hosts.json").exists());
}
#[test]
fn malformed_trust_fails_closed_without_prompt_or_write() {
    let fixture = Fixture::new();
    let known = KnownHosts::new(fixture.directory.clone());
    let key = public_key();
    known
        .verify("host", 22, &key, |_, _, _| Ok(TrustDecision::TrustAndSave))
        .unwrap();
    let file = fixture.path.join("ssh-known-hosts.json");
    for text in [
        "broken",
        r#"{"version":2,"hosts":{}}"#,
        r#"{"version":1,"hosts":{"not-a-tuple":{"type":"ssh-rsa","fingerprint":"SHA256:bad"}}}"#,
        r#"{"version":1,"hosts":{"duplicate":{},"duplicate":{}}}"#,
    ] {
        fs::write(&file, text).unwrap();
        assert_eq!(
            known.verify("host", 22, &key, |_, _, _| panic!(
                "corrupt must not prompt"
            )),
            Err(Error::Corrupt)
        );
        assert_eq!(fs::read_to_string(&file).unwrap(), text);
    }
}

#[test]
fn electron_host_keys_are_added_without_replacing_saved_ones() {
    let fixture = Fixture::new();
    let known = KnownHosts::new(fixture.directory.clone());
    let saved = public_key();
    known
        .verify("saved", 22, &saved, |_, _, _| {
            Ok(TrustDecision::TrustAndSave)
        })
        .unwrap();
    let electron_key = public_key();
    let info = shellfox_ssh::host_key_info(&electron_key).unwrap();
    // src/main/ssh/known-hosts.ts writes JSON.stringify([host.toLowerCase(), port]) keys.
    let document = serde_json::json!({"version":1,"hosts":{
        "[\"vm.example\",2222]": {"type": info.key_type, "fingerprint": info.fingerprint},
        "[\"saved\",22]": {"type": info.key_type, "fingerprint": info.fingerprint},
        "[\"bad host\",22]": {"type": info.key_type, "fingerprint": info.fingerprint}
    }});
    assert_eq!(
        known
            .import_electron(document.to_string().as_bytes())
            .unwrap(),
        1
    );
    assert!(matches!(
        known.inspect("vm.example", 2222, &electron_key).unwrap(),
        TrustStatus::Known(_)
    ));
    assert!(matches!(
        known.inspect("saved", 22, &saved).unwrap(),
        TrustStatus::Known(_)
    ));
    assert_eq!(
        known
            .import_electron(document.to_string().as_bytes())
            .unwrap(),
        0
    );
    assert_eq!(known.import_electron(b"{}"), Err(Error::Corrupt));
}
