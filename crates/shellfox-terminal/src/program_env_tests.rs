use super::*;
#[test]
fn private_payload_rejects_unknown_reserved_keys_and_modes_without_exposing_values() {
    #[cfg(windows)]
    let (program, askpass, directory) = (
        r"C:\trusted\target.exe",
        r"C:\trusted\askpass.exe",
        r"C:\private",
    );
    #[cfg(not(windows))]
    let (program, askpass, directory) = ("/trusted/target", "/trusted/askpass", "/private");
    #[cfg(windows)]
    let endpoint = r"\\.\pipe\shellfox-rust-v1-fixture";
    #[cfg(not(windows))]
    let endpoint = "/private/test.sock";
    let ids = [
        wire::FileId {
            first: 1,
            second: 2,
        },
        wire::FileId {
            first: 3,
            second: 4,
        },
    ];
    let packet = || {
        wire::encode(
            program,
            ids,
            [
                askpass,
                "force",
                "shellfox-askpass:0",
                endpoint,
                directory,
                &"a".repeat(64),
            ],
        )
        .unwrap()
    };
    assert!(wire::decode(&packet().0).is_ok());
    let mut invalid = packet();
    let offset = 40 + 4 + program.len();
    invalid.0[offset..offset + 4].copy_from_slice(&4u32.to_le_bytes());
    invalid.0.splice(
        offset + 4..offset + 4 + wire::KEYS[0].len(),
        b"TERM".iter().copied(),
    );
    assert!(wire::decode(&invalid.0).is_err());
    let mut invalid = packet();
    invalid.0[4] = 2;
    assert!(wire::decode(&invalid.0).is_err());
    assert!(wire::decode(&invalid.0[..20]).is_err());
}
#[test]
fn broker_rejects_non_capability_tokens_and_control_character_injection() {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    assert!(AskpassBroker::new(root.clone(), "bad\0endpoint".into(), "a".repeat(64)).is_err());
    assert!(AskpassBroker::new(
        root,
        "bad".into(),
        "password-is-not-an-ipc-capability".into()
    )
    .is_err());
}
