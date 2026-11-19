use super::*;
const SOURCE: &str = "8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c";

#[test]
fn commands_scope_both_values_to_the_original_scheme() {
    let scheme = Uuid::parse_str(SOURCE).unwrap();
    assert_eq!(
        arguments(scheme, CommandKind::Query),
        ["/qh", SOURCE, "SUB_BUTTONS", "LIDACTION"]
    );
    assert_eq!(
        arguments(scheme, CommandKind::SetAc(0)),
        ["/setacvalueindex", SOURCE, "SUB_BUTTONS", "LIDACTION", "0"]
    );
    assert_eq!(
        arguments(scheme, CommandKind::SetDc(2)),
        ["/setdcvalueindex", SOURCE, "SUB_BUTTONS", "LIDACTION", "2"]
    );
    assert_eq!(
        arguments(scheme, CommandKind::Apply),
        ["/setactive", SOURCE]
    );
}

#[test]
fn restore_uses_the_recorded_scheme_and_never_forgets_an_unconfirmed_restore() {
    let scheme = Uuid::parse_str(SOURCE).unwrap();
    let record = Journal {
        scheme: Some(scheme),
        original: Values { ac: 1, dc: 2 },
    };
    let result = restore_values(
        record,
        || panic!("no active-scheme fallback for Rust journals"),
        |target, values| {
            assert_eq!(target, scheme);
            assert_eq!(values, Values { ac: 1, dc: 2 });
            Err(PowerError("write refused".into()))
        },
        || panic!("journal lost after failed verification"),
    );
    assert_eq!(result, Err(PowerError("write refused".into())));
    let record = Journal::parse(b"1,2").unwrap();
    let forgotten = std::cell::Cell::new(false);
    restore_values(
        record,
        || Ok(scheme),
        |target, values| {
            assert_eq!(target, scheme);
            assert_eq!(values, Values { ac: 1, dc: 2 });
            Ok(())
        },
        || {
            forgotten.set(true);
            Ok(())
        },
    )
    .unwrap();
    assert!(forgotten.get());
}

#[test]
#[ignore = "changes this machine's Windows lid policy; run once with operator permission"]
fn windows_live_lid_roundtrip() {
    use crate::power::PowerLease;
    let directory = PathBuf::from(
        std::env::var_os("SHELLFOX_LIVE_LID_TEST_DIR").expect("explicit isolated data directory"),
    );
    assert!(directory.is_absolute() && directory.is_dir());
    let path = directory.join(JOURNAL);
    assert!(!path.exists());
    let power = PowerCfg::new().unwrap();
    let scheme = power.active_scheme().unwrap();
    let before = power.read(scheme).unwrap();
    eprintln!("Scheme {scheme}: before AC={} DC={}", before.ac, before.dc);
    assert_ne!(
        before, KEEP_AWAKE,
        "both settings already use Do Nothing; no observable round trip"
    );
    let mut lease = match super::super::acquire(Some(&path)) {
        Ok(lease) => lease,
        Err(error) => {
            if power.read(scheme).ok() != Some(before) {
                power
                    .set(scheme, before)
                    .expect("restore recorded values immediately");
            }
            panic!("Enable failed: {error}");
        }
    };
    let held = power.read(scheme);
    eprintln!("Enabled: {held:?}");
    let restored = lease.release();
    let after = power.read(scheme);
    if after.as_ref().ok() != Some(&before) {
        power
            .set(scheme, before)
            .expect("restore recorded values immediately");
    }
    eprintln!("Disabled: {after:?}");
    restored.unwrap();
    assert_eq!(held.unwrap(), KEEP_AWAKE);
    assert_eq!(after.unwrap(), before);
    assert!(!path.exists());
}

#[test]
fn legacy_and_rust_journals_restore_the_same_values() {
    let original = Values { ac: 1, dc: 3 };
    assert_eq!(
        Journal::parse(b"1,3\r\n").unwrap(),
        Journal {
            scheme: None,
            original
        }
    );
    assert_eq!(
        Journal::parse(format!("{SOURCE},1,3\n").as_bytes()).unwrap(),
        Journal {
            scheme: Uuid::parse_str(SOURCE).ok(),
            original
        }
    );
    assert_eq!(
        parse_query(b"settings\r\n AC: 0x00000001\r\n DC: 0x00000003").unwrap(),
        original
    );
    for malformed in ["1", "0,999", "-1,2", "0,1,2", "0,1,garbage"] {
        assert!(Journal::parse(malformed.as_bytes()).is_err());
    }
}
