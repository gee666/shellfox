use super::*;
use rand::RngCore;
use zeroize::Zeroizing;
// RFC 8410 PKCS8 framing, followed by a fresh synthetic Ed25519 seed.
fn private_fixture() -> Zeroizing<String> {
    let mut der = Zeroizing::new(vec![
        0x30, 0x2e, 0x02, 0x01, 0x00, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x04, 0x22, 0x04,
        0x20,
    ]);
    let mut seed = Zeroizing::new([0u8; 32]);
    rand::rngs::OsRng.fill_bytes(seed.as_mut());
    der.extend_from_slice(seed.as_ref());
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut encoded = Zeroizing::new(String::new());
    for bytes in der.chunks(3) {
        let a = bytes[0];
        let b = bytes.get(1).copied().unwrap_or(0);
        let c = bytes.get(2).copied().unwrap_or(0);
        for value in [
            ALPHABET[(a >> 2) as usize],
            ALPHABET[(((a & 3) << 4) | (b >> 4)) as usize],
            if bytes.len() > 1 {
                ALPHABET[(((b & 15) << 2) | (c >> 6)) as usize]
            } else {
                b'='
            },
            if bytes.len() > 2 {
                ALPHABET[(c & 63) as usize]
            } else {
                b'='
            },
        ] {
            encoded.push(char::from(value));
        }
    }
    Zeroizing::new(format!(
        "-----BEGIN PRIVATE KEY-----\n{}\n-----END PRIVATE KEY-----\n",
        encoded.as_str()
    ))
}
#[test]
fn user_selected_key_reports_actual_validation_and_no_server_dialog() {
    let fixture = Fixture::new();
    let mut manager = fixture.manager();
    let validator = Validator(AtomicBool::new(true));
    let mut starter = Starter::default();
    let store = shellfox_ssh::keys::KeyStore::new(fixture.directory.clone());
    let original = private_fixture();
    let imported = store.import(original.as_bytes(), None).unwrap();
    let file = fixture
        .directory
        .path()
        .join(shellfox_ssh::keys::KeyStore::file_name(imported.id));
    let Response::SelectedKey(selected) = manager
        .request(
            7,
            Request::ValidateKey {
                selected: SelectedKey::new(file).unwrap(),
            },
            &validator,
            &mut starter,
        )
        .unwrap()
    else {
        panic!("selected key")
    };
    assert!(selected.cryptographically_validated);
    assert!(!selected.metadata.encrypted);
    let encrypted = store
        .prepare_for_system(imported.id, None, &mut rand::rngs::OsRng)
        .unwrap();
    let Response::SelectedKey(selected) = manager
        .request(
            7,
            Request::ValidateKey {
                selected: SelectedKey::new(encrypted.path().into()).unwrap(),
            },
            &validator,
            &mut starter,
        )
        .unwrap()
    else {
        panic!("selected key")
    };
    assert!(!selected.cryptographically_validated);
    assert!(selected.metadata.encrypted);
    encrypted.cleanup().unwrap();
    assert!(SelectedKey::new(PathBuf::from("relative-key")).is_err());
    assert_eq!(starter.starts, 0);
}
#[test]
fn native_secret_constructors_and_request_debug_redact_payloads() {
    let mut p = input();
    p.password = Password::set("synthetic-selection-secret".into()).unwrap();
    assert!(!format!("{:?}", Request::Save(p)).contains("synthetic-selection-secret"));
    assert!(!format!(
        "{:?}",
        Request::AnswerPrompt {
            connection: Uuid::new_v4(),
            prompt: Uuid::new_v4(),
            answer: Some(Secret::new("synthetic-answer-secret".into()).unwrap())
        }
    )
    .contains("synthetic-answer-secret"));
}
