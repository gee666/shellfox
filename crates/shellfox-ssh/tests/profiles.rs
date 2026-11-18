mod common;
use common::*;
use shellfox_ssh::{
    ConnectionOptions, Error, ImportSession, PasswordUpdate, ProfileInput, ProfileStore, Secret,
    UnsupportedProxy,
};
use std::{fs, sync::Arc};

#[test]
fn passwords_are_tri_state_and_dtos_redact_credentials() {
    let fixture = Fixture::new();
    let store = ProfileStore::new(fixture.directory.clone());
    let mut input = profile("Zulu");
    let id = input.id;
    input.password = PasswordUpdate::Set(Secret::new("fixture-secret".into()).unwrap());
    store.save(input.clone()).unwrap();
    store.save(profile("alpha")).unwrap();
    input.password = PasswordUpdate::Keep;
    let listed = store.save(input.clone()).unwrap();
    assert_eq!(listed[0].name, "alpha");
    assert!(listed[1].has_password);
    assert!(!serde_json::to_string(&listed)
        .unwrap()
        .contains("fixture-secret"));
    assert_eq!(
        store
            .get_for_connection(id)
            .unwrap()
            .unwrap()
            .password
            .unwrap()
            .expose(),
        "fixture-secret"
    );
    input.password = PasswordUpdate::Clear;
    store.save(input.clone()).unwrap();
    assert!(store
        .get_for_connection(id)
        .unwrap()
        .unwrap()
        .password
        .is_none());
    input.password = PasswordUpdate::Set(Secret::new("".into()).unwrap());
    store.save(input).unwrap();
    assert!(
        store
            .get_for_connection(id)
            .unwrap()
            .unwrap()
            .dto()
            .has_password
    );
    assert_eq!(
        fs::read_dir(&fixture.path)
            .unwrap()
            .filter(|p| p
                .as_ref()
                .unwrap()
                .path()
                .extension()
                .is_some_and(|e| e == "tmp"))
            .count(),
        0
    );
    assert_eq!(store.delete(id).unwrap().len(), 1);
}
#[test]
fn strict_json_and_hostile_limits() {
    let input = profile("valid");
    let mut json = serde_json::to_value(&input).unwrap();
    assert_eq!(
        ProfileInput::from_json(&serde_json::to_vec(&json).unwrap())
            .unwrap()
            .password,
        PasswordUpdate::Keep
    );
    json["password"] = serde_json::Value::Null;
    assert_eq!(
        ProfileInput::from_json(&serde_json::to_vec(&json).unwrap())
            .unwrap()
            .password,
        PasswordUpdate::Clear
    );
    json["password"] = "".into();
    assert!(matches!(
        ProfileInput::from_json(&serde_json::to_vec(&json).unwrap())
            .unwrap()
            .password,
        PasswordUpdate::Set(_)
    ));
    assert_eq!(
        ProfileInput::from_json(&vec![0; 512 * 1024 + 1]).unwrap_err(),
        Error::Limit
    );
    json["connectionOptions"] = serde_json::json!({"tryAgent":true});
    assert!(ProfileInput::from_json(&serde_json::to_vec(&json).unwrap()).is_err());
    for host in ["a\nb", "x\u{a0}y", "", "a\0b"] {
        let mut p = input.clone();
        p.host = host.into();
        assert!(p.validate().is_err());
    }
    let mut p = input.clone();
    p.port = 0;
    assert!(p.validate().is_err());
    let mut p = input;
    p.name = "x".repeat(201);
    assert_eq!(p.validate().unwrap_err(), Error::Limit);
}
#[test]
fn concurrent_stores_merge_and_reject_case_insensitive_names() {
    let fixture = Fixture::new();
    let stores: Vec<_> = (0..8)
        .map(|_| ProfileStore::new(fixture.directory.clone()))
        .collect();
    std::thread::scope(|scope| {
        for (i, store) in stores.iter().enumerate() {
            scope.spawn(move || store.save(profile(&format!("test{i}"))).unwrap());
        }
    });
    let store = &stores[0];
    assert_eq!(store.list().unwrap().len(), 8);
    assert_eq!(
        store.save(profile("TEST0")).unwrap_err(),
        Error::DuplicateName
    );
    let independently_opened =
        Arc::new(shellfox_ssh::PrivateDirectory::open(&fixture.path, policy()).unwrap());
    let second = ProfileStore::new(independently_opened);
    second.save(profile("second")).unwrap();
    assert_eq!(store.list().unwrap().len(), 9);
}
#[test]
fn corrupt_storage_is_not_overwritten_and_import_preserves_private_state() {
    let fixture = Fixture::new();
    let store = ProfileStore::new(fixture.directory.clone());
    let mut input = profile("Prod");
    input.password = PasswordUpdate::Set(Secret::new("test-only".into()).unwrap());
    input.key_file = Some("/private/key".into());
    let id = input.id;
    store.save(input).unwrap();
    let import = || ImportSession {
        name: "prod".into(),
        host: "first@account@new.test:99".into(),
        port: 2200,
        user: "ignored".into(),
        key_file: Some("/new/key".into()),
        connection_options: Some(ConnectionOptions {
            try_agent: Some(false),
            unsupported_proxy: Some(UnsupportedProxy {
                method: "SOCKS5".into(),
                host: Some("proxy.test".into()),
                port: Some(1080),
            }),
            ..Default::default()
        }),
    };
    let result = store.import_sessions(vec![import()]).unwrap();
    assert_eq!((result.added, result.updated, result.found), (0, 1, 1));
    let p = store.get_for_connection(id).unwrap().unwrap();
    assert_eq!(p.password.unwrap().expose(), "test-only");
    assert_eq!(p.remote_cwd.as_deref(), Some("/srv"));
    assert_eq!(
        (p.host.as_str(), p.user.as_str(), p.port),
        ("new.test", "first@account", 2200)
    );
    assert!(!serde_json::to_string(&store.list().unwrap())
        .unwrap()
        .contains("connectionOptions"));
    let remote = serde_json::to_string(&store.list_remote().unwrap()).unwrap();
    for private in [
        "password\"",
        "keyFile",
        "proxy.test",
        "/new/key",
        "test-only",
    ] {
        assert!(!remote.contains(private));
    }
    assert_eq!(store.import_sessions(vec![import()]).unwrap().updated, 0);
    let file = fixture.path.join("ssh-profiles.json");
    let original: serde_json::Value = serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
    let mut missing_password = original.clone();
    missing_password["profiles"][0]
        .as_object_mut()
        .unwrap()
        .remove("password");
    let mut duplicate = original.clone();
    duplicate["profiles"]
        .as_array_mut()
        .unwrap()
        .push(original["profiles"][0].clone());
    for invalid in [missing_password, duplicate] {
        fs::write(&file, serde_json::to_vec(&invalid).unwrap()).unwrap();
        assert_eq!(store.list().unwrap_err(), Error::Corrupt);
    }
    for content in [
        "broken",
        r#"{"version":2,"profiles":[]}"#,
        r#"{"version":1,"profiles":[],"extra":1}"#,
    ] {
        fs::write(&file, content).unwrap();
        assert_eq!(store.list().unwrap_err(), Error::Corrupt);
        assert_eq!(store.save(profile("other")).unwrap_err(), Error::Corrupt);
        assert_eq!(fs::read_to_string(&file).unwrap(), content);
    }
}
#[test]
fn mutation_guard_denial_keeps_original_private_store() {
    let fixture = Fixture::new();
    let store = ProfileStore::new(fixture.directory.clone());
    let p = profile("saved");
    let id = p.id;
    store.save(p).unwrap();
    let file = fixture.path.join("ssh-profiles.json");
    let before = fs::read(&file).unwrap();
    assert_eq!(
        store.save_guarded(profile("denied"), || Err(Error::Permissions)),
        Err(Error::Permissions)
    );
    assert_eq!(
        store.delete_guarded(id, || Err(Error::Permissions)),
        Err(Error::Permissions)
    );
    assert_eq!(fs::read(file).unwrap(), before);
}
#[test]
fn endpoint_normalization_matches_active_putty_behavior() {
    for (host, expected, user) in [
        (" \tlogin@ser ver\t.test:2222", "server.test", "login"),
        ("user@realm@server", "server", "user@realm"),
        ("login@[2001:db8::1]:99", "2001:db8::1", "login"),
        ("[fe80::1%eth0]", "fe80::1%eth0", "old"),
        ("host:not-a-port", "host", "old"),
        ("[not-ipv6]", "[not-ipv6]", "old"),
    ] {
        assert_eq!(
            shellfox_ssh::normalize_endpoint(host, "old").unwrap(),
            (expected.into(), user.into())
        );
    }
}
