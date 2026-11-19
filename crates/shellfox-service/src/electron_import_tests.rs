use super::*;
use rusqlite::Connection;
use std::fs;

#[test]
fn imports_stable_ids_and_rejects_incompatible_records() {
    let root = crate::tests::scratch();
    let host = Host::open(root.path()).unwrap();
    let mut report = Report::default();
    import_approvals(&host, br#"{"version":1,"approvals":[{"peerId":"n-phone","hostname":"Phone","approvedAt":"2026-11-18T00:00:00.000Z"},{"peerId":"n-legacy","hostname":"Legacy","approvedAt":"invalid"}]}"#, &mut report).unwrap();
    assert_eq!(report.approved, 1);
    assert_eq!(report.revoked[0].peer_id.as_deref(), Some("n-legacy"));
    let saved = host.approval_snapshot().unwrap();
    assert_eq!(saved.approvals.len(), 1);
    assert_eq!(saved.approvals[0].peer_id, "n-phone");
}

#[test]
fn imports_electron_write_formats_once_without_changing_source_files() {
    let root = crate::tests::scratch();
    let electron = root.path().join("Shellfox");
    let data = electron.join("rust-v1/host");
    let control = data.join("rust-service-v1");
    fs::create_dir_all(&data).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        fs::DirBuilder::new().mode(0o700).create(&control).unwrap();
    }
    #[cfg(windows)]
    fs::create_dir(&control).unwrap();
    #[cfg(not(windows))]
    let policy = StoragePolicy::default();
    #[cfg(windows)]
    let policy = {
        let helper = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tmp/dev-helper/shellfox-tailscale.exe");
        if !helper.is_file() {
            eprintln!("Fixture requires tmp/dev-helper/shellfox-tailscale.exe; skipping Windows SSH import test.");
            return;
        }
        StoragePolicy::with_windows_helper(helper.canonicalize().unwrap())
    };
    // The Electron Repository writes the settings row in WAL mode.
    let db = Connection::open(electron.join(DATABASE)).unwrap();
    db.execute_batch(
        "PRAGMA journal_mode=WAL; PRAGMA user_version=5;
        CREATE TABLE settings (id INTEGER PRIMARY KEY CHECK(id=1), value TEXT NOT NULL);",
    )
    .unwrap();
    let mut settings = shellfox_core::Settings::default();
    settings.accent_color = "#0075ff".into();
    settings.background_color = "#ddeeff".into();
    settings.auto_update = false;
    settings.history_page_size = 42;
    settings.session_settle_hours = 12.5;
    settings.session_delete_hours = 50.;
    settings.terminal_profile_id = Some(Some("wsl:Debian".into()));
    settings.shell_id = shellfox_core::SettingsShell::Wsl;
    settings.shell_executable = Some(r"C:\Windows\System32\wsl.exe".into());
    settings.process_rules = vec![shellfox_core::SettingsProcessRule {
        id: "custom-fixture-ab12".into(),
        label: "Build agent".into(),
        enabled: true,
        executable_basenames: vec!["agent".into()],
        executable_paths: vec![],
        script_path_suffixes: vec!["bin/agent.js".into()],
        process_names: None,
    }];
    db.execute(
        "INSERT INTO settings VALUES (1, ?)",
        [serde_json::to_string(&settings).unwrap()],
    )
    .unwrap();
    let profile_id = "e64ec926-9af9-43dd-9221-084847d93ffe";
    // src/main/ssh/storage.ts writes this v1 document, including host-only fields.
    fs::write(
        electron.join(SSH),
        serde_json::json!({"version":1,"profiles":[{
            "id":profile_id,"name":"Build VM","host":"vm.example","port":2222,"user":"build",
            "password":"fixture-secret","keyFile":null,"remoteCwd":"/work","source":"putty",
            "connectionOptions":{"addressFamily":4,"tryAgent":false,"agentForward":false}
        }]})
        .to_string(),
    )
    .unwrap();
    fs::write(electron.join(REMOTE), br#"{"version":1,"enabled":true,"hostname":"workstation","peers":[],"approvals":[{"peerId":"n-phone","hostname":"Phone","approvedAt":"2026-11-18T00:00:00.000Z"}]}"#).unwrap();
    let snapshot = || {
        [DATABASE, WAL, "manager.sqlite3-shm", SSH, REMOTE]
            .map(|name| fs::read(electron.join(name)).unwrap())
    };
    let original = snapshot();
    let pass = || run(&electron, &data, &control, policy.clone(), || Ok(())).unwrap();
    let report = pass().unwrap();
    assert!(report.settings);
    assert_eq!(report.ssh_profiles, 1);
    assert_eq!(report.approved, 1);
    assert_eq!(original, snapshot());
    assert!(pass().is_none());
    let host = Host::open(data).unwrap();
    let stored = host.global_settings().unwrap();
    assert_eq!(stored.accent_color, settings.accent_color);
    assert_eq!(stored.background_color, settings.background_color);
    assert_eq!(stored.history_page_size, settings.history_page_size);
    assert_eq!(stored.session_settle_hours, settings.session_settle_hours);
    assert_eq!(stored.session_delete_hours, settings.session_delete_hours);
    assert_eq!(stored.terminal_profile_id, settings.terminal_profile_id);
    assert_eq!(stored.shell_id, shellfox_core::SettingsShell::Wsl);
    assert_eq!(host.snapshot().unwrap().preferences.default_profile_id.as_str(), "wsl:Debian");
    assert_eq!(stored.process_rules, settings.process_rules);
    assert!(!stored.auto_update);
    assert_eq!(
        host.approval_snapshot().unwrap().approvals[0].peer_id,
        "n-phone"
    );
    let store = ProfileStore::new(Arc::new(
        PrivateDirectory::open(control.join("ssh"), policy).unwrap(),
    ));
    let list = store.list().unwrap();
    assert_eq!(list.len(), 1);
    assert!(list[0].has_password);
    assert_eq!(list[0].remote_cwd.as_deref(), Some("/work"));
    assert_eq!(list[0].source, shellfox_ssh::ProfileSource::Putty);
    assert!(!control.join("tailscale").exists());
}
