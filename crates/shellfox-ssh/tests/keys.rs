mod common;
use base64::{engine::general_purpose::STANDARD, Engine};
use common::*;
use serde::Deserialize;
use shellfox_ssh::{
    keys::{parse_private_key, KeyStore},
    Error, Secret,
};
use std::{fs, process::Command};
use zeroize::Zeroizing;

#[derive(Deserialize)]
struct Fixtures {
    cases: Vec<Case>,
    legacy: Vec<Legacy>,
}
#[derive(Deserialize)]
struct Legacy {
    name: String,
    data: String,
    password: Option<String>,
}
#[derive(Deserialize)]
struct Case {
    name: String,
    data: String,
    public: String,
    passphrase: Option<String>,
}
#[test]
fn all_active_algorithms_and_containers_match_independent_node_fixtures() {
    let fixture = Fixture::new();
    let helper = root().join("tmp/dev-helper/shellfox-tailscale.exe");
    let result = Command::new("node")
        .arg("--experimental-strip-types")
        .arg(root().join("crates/shellfox-ssh/tests/generate-fixtures.mjs"))
        .arg(&fixture.path)
        .env("SHELLFOX_SSH_TEST_PERMISSIONS_HELPER", helper)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "fixture generation failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    let bytes = Zeroizing::new(fs::read(fixture.path.join("fixtures.json")).unwrap());
    let cases: Fixtures = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(cases.cases.len(), 62);
    let store = KeyStore::new(fixture.directory.clone());
    for case in cases.cases {
        let data = Zeroizing::new(STANDARD.decode(&case.data).unwrap());
        let passphrase = case.passphrase.map(|p| Secret::new(p).unwrap());
        let key = parse_private_key(&data, passphrase.as_ref())
            .unwrap_or_else(|e| panic!("{}: {e}", case.name));
        assert_eq!(
            key.public_blob().unwrap(),
            STANDARD.decode(&case.public).unwrap(),
            "{}",
            case.name
        );
        assert!(!format!("{key:?}").contains("PRIVATE KEY"));
        let reencoded = key.to_openssh_in_memory().unwrap();
        assert_eq!(
            parse_private_key(reencoded.as_bytes(), None)
                .unwrap()
                .public_blob()
                .unwrap(),
            key.public_blob().unwrap()
        );
        if passphrase.is_some() {
            assert_eq!(
                parse_private_key(&data, None).unwrap_err(),
                Error::PassphraseRequired,
                "{}",
                case.name
            );
            assert!(
                parse_private_key(&data, Some(&Secret::new("wrong".into()).unwrap())).is_err(),
                "{}",
                case.name
            );
        }
        if case.name == "ed25519-ppk3-encrypted" {
            let imported = store.import(&data, passphrase.as_ref()).unwrap();
            assert_eq!(
                fs::read(fixture.path.join(KeyStore::file_name(imported.id))).unwrap(),
                *data
            );
            assert_eq!(
                store
                    .load(imported.id, passphrase.as_ref())
                    .unwrap()
                    .public_blob()
                    .unwrap(),
                key.public_blob().unwrap()
            );
            let text = String::from_utf8(data.to_vec()).unwrap();
            assert_eq!(
                parse_private_key(
                    text.replace("generated clé / ключ", "tampered").as_bytes(),
                    passphrase.as_ref()
                )
                .unwrap_err(),
                Error::InvalidKey
            );
            assert_eq!(
                parse_private_key(
                    text.replace("Argon2-Memory: 32", "Argon2-Memory: 4294967295")
                        .as_bytes(),
                    passphrase.as_ref()
                )
                .unwrap_err(),
                Error::Limit
            );
        }
    }
    assert_eq!(cases.legacy.len(), 6);
    for legacy in cases.legacy {
        let bytes = Zeroizing::new(STANDARD.decode(&legacy.data).unwrap());
        let password = legacy.password.map(|p| Secret::new(p).unwrap());
        let imported = store
            .import_preserving_legacy(&bytes, password.as_ref())
            .unwrap_or_else(|e| panic!("{}: {e}", legacy.name));
        assert!(!imported.cryptographically_validated);
        assert!(imported.metadata.limitation.is_some());
        assert_eq!(
            fs::read(fixture.path.join(KeyStore::file_name(imported.id))).unwrap(),
            *bytes
        );
        assert!(
            !store
                .metadata(imported.id)
                .unwrap()
                .unwrap()
                .cryptographically_validated
        );
        assert!(store.load(imported.id, password.as_ref()).is_err());
    }
    assert_eq!(
        parse_private_key(&vec![0; 128 * 1024 + 1], None).unwrap_err(),
        Error::Limit
    );
    assert_eq!(
        parse_private_key(b"broken", None).unwrap_err(),
        Error::InvalidKey
    );
}
#[test]
fn cipher_schedules_and_big_integer_intermediates_have_zeroization_support() {
    fn on_drop<T: zeroize::ZeroizeOnDrop>() {}
    fn erasable<T: zeroize::Zeroize>() {}
    on_drop::<aes::Aes256>();
    erasable::<num_bigint_dig::BigUint>();
    let mut scratch = Zeroizing::new(vec![0x42u8; 80]);
    zeroize::Zeroize::zeroize(&mut *scratch);
    assert!(scratch.iter().all(|b| *b == 0));
    let secret = Secret::new("synthetic-only".into()).unwrap();
    assert_eq!(format!("{secret:?}"), "[REDACTED]");
}
#[test]
fn encrypted_openssh_checks_password_and_bounds() {
    let original =
        ssh_key::PrivateKey::random(&mut rand::rngs::OsRng, ssh_key::Algorithm::Ed25519).unwrap();
    let encrypted = original
        .encrypt(&mut rand::rngs::OsRng, "test-password")
        .unwrap()
        .to_openssh(ssh_key::LineEnding::LF)
        .unwrap();
    assert_eq!(
        parse_private_key(encrypted.as_bytes(), None).unwrap_err(),
        Error::PassphraseRequired
    );
    let password = Secret::new("test-password".into()).unwrap();
    assert_eq!(
        parse_private_key(encrypted.as_bytes(), Some(&password))
            .unwrap()
            .public_blob()
            .unwrap(),
        original.public_key().to_bytes().unwrap()
    );
}
