use super::*;
use rusqlite::Connection;
use std::{fs, path::PathBuf};

const SESSION: &str = "f919fb1a-fb03-4a93-8b9b-1cde465d5870";
const NODE: &str = r#"{"_machinekey":"bWFjaGluZQ==","_current-profile":"cHJvZmlsZQ=="}"#;

/// `<root>/Shellfox` with the Rust host at `Shellfox/rust-v1/host`.
pub(super) struct Fixture {
    _root: tempfile::TempDir,
    pub electron: PathBuf,
    pub data: PathBuf,
    pub control: PathBuf,
}
impl Fixture {
    pub fn new() -> Self {
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
        {
            // The host protects its control dir before the import runs.
            fs::create_dir(&control).unwrap();
            let runtime = tokio::runtime::Builder::new_current_thread()
                .build()
                .unwrap();
            runtime
                .block_on(
                    verifier().protect(&control, shellfox_transport::PrivatePathKind::Directory),
                )
                .unwrap();
        }
        Self {
            _root: root,
            electron,
            data,
            control,
        }
    }
    /// Repository schema v5 (src/main/repository.ts) in WAL mode, one session with env.
    pub fn database(&self, settings: &shellfox_core::Settings) {
        let db = Connection::open(self.electron.join(files::DATABASE)).unwrap();
        db.execute_batch(include_str!(
            "../../shellfox-host/tests/fixtures/legacy-v1.sql"
        ))
        .unwrap();
        db.execute_batch(
            "PRAGMA journal_mode=WAL;
            CREATE TABLE terminal_metadata (tabId TEXT PRIMARY KEY REFERENCES tabs(id), value TEXT NOT NULL);
            ALTER TABLE sessions ADD COLUMN env TEXT NOT NULL DEFAULT '[]';
            UPDATE sessions SET env='[{\"name\":\"PROJECT\",\"value\":\"alpha\"}]';
            ALTER TABLE sessions ADD COLUMN pinnedAt TEXT;
            DELETE FROM preferences;
            PRAGMA user_version=5;",
        )
        .unwrap();
        db.execute(
            "UPDATE settings SET value=?1",
            [serde_json::to_string(settings).unwrap()],
        )
        .unwrap();
    }
    pub fn remote(&self) {
        fs::write(
            self.electron.join(remote::REMOTE),
            br#"{"version":1,"enabled":true,"hostname":"workstation","peers":[{"id":"n-laptop","hostname":"laptop","addresses":["100.64.0.2"],"online":false}],"approvals":[{"peerId":"n-phone","hostname":"Phone","approvedAt":"2026-11-18T00:00:00.000Z"}]}"#,
        )
        .unwrap();
        fs::create_dir(self.electron.join("tailscale")).unwrap();
        fs::write(self.electron.join("tailscale/tailscaled.state"), NODE).unwrap();
    }
    pub fn run(&self, node: bool) -> Result<Option<Report>> {
        let verifier = verifier();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let access = node.then_some((&verifier, runtime.handle()));
        run(
            &self.electron,
            &self.data,
            &self.control,
            policy(),
            access,
            || Ok(()),
        )
    }
    pub fn leaf(&self) -> PathBuf {
        crate::app_paths::default_remote_state(&self.data)
    }
    pub fn age_import(&self) {
        fs::write(self.control.join(MARKER_V2), br#"{"importedAtMs":1}"#).unwrap();
    }
}

fn verifier() -> PermissionVerifier {
    let (helper, digest) = crate::tests::helper();
    PermissionVerifier::new(shellfox_transport::VerifiedHelper::new(helper, digest).unwrap())
}

fn policy() -> StoragePolicy {
    #[cfg(not(windows))]
    return StoragePolicy::default();
    #[cfg(windows)]
    StoragePolicy::with_windows_helper(crate::tests::helper().0)
}

fn settings() -> shellfox_core::Settings {
    let mut settings = shellfox_core::Settings::default();
    settings.accent_color = "#0075ff".into();
    settings.auto_update = false;
    settings.history_page_size = 42;
    settings
}

#[test]
fn only_the_rust_v1_host_layout_names_an_electron_root() {
    let root = Path::new("/data/Shellfox");
    assert_eq!(electron_root(&root.join("rust-v1/host")), Some(root));
    assert_eq!(
        electron_root(Path::new("/custom/user-data/rust-v1/host")),
        Some(Path::new("/custom/user-data"))
    );
    for other in [
        "/data/host",
        "/data/rust-v1/other",
        "/data/rust-v2/host",
        "/host",
    ] {
        assert_eq!(electron_root(Path::new(other)), None, "{other}");
    }
}

#[test]
fn imports_every_item_once_without_changing_electron_files() {
    let fixture = Fixture::new();
    fixture.database(&settings());
    fixture.remote();
    fs::write(
        fixture.electron.join(SSH),
        serde_json::json!({"version":1,"profiles":[{
            "id":"e64ec926-9af9-43dd-9221-084847d93ffe","name":"Build VM","host":"vm.example","port":2222,"user":"build",
            "password":"fixture-secret","keyFile":null,"remoteCwd":"/work","source":"putty",
            "connectionOptions":{"addressFamily":4,"tryAgent":false,"agentForward":false}
        }]})
        .to_string(),
    )
    .unwrap();
    let kept = [
        files::DATABASE,
        SSH,
        remote::REMOTE,
        "tailscale/tailscaled.state",
    ];
    let original = kept.map(|name| fs::read(fixture.electron.join(name)).unwrap());
    let report = fixture.run(true).unwrap().unwrap();
    assert!(report.settings && report.remote_preferences);
    assert_eq!(
        (report.sessions, report.ssh_profiles, report.approved),
        (1, 1, 1)
    );
    assert_eq!(report.node, Some(remote::Node::Adopted));
    assert_eq!(
        original,
        kept.map(|name| fs::read(fixture.electron.join(name)).unwrap())
    );
    assert!(fixture.run(true).unwrap().is_none());

    let host = Host::open(&fixture.data).unwrap();
    assert_eq!(host.global_settings().unwrap().accent_color, "#0075ff");
    let sessions = host.snapshot().unwrap().sessions;
    assert_eq!(
        (
            sessions[0].id.to_string().as_str(),
            sessions[0].title.as_str()
        ),
        (SESSION, "Legacy project")
    );
    assert_eq!(sessions[0].env[0].value, "alpha");
    assert!(sessions[0].tabs.is_empty());
    assert_eq!(
        host.approval_snapshot().unwrap().approvals[0].peer_id,
        "n-phone"
    );
    let preferences: serde_json::Value =
        serde_json::from_slice(&fs::read(fixture.control.join("remote-settings-v1.json")).unwrap())
            .unwrap();
    assert_eq!(
        preferences,
        serde_json::json!({"version":1,"enabled":true,"hostname":"workstation",
            "peers":[{"id":"n-laptop","hostname":"laptop","addresses":["100.64.0.2"]}]})
    );
    assert_eq!(
        fs::read_to_string(fixture.leaf().join("tailscaled.state")).unwrap(),
        NODE
    );
    assert!(fixture
        .leaf()
        .join(shellfox_transport::IsolatedStateDirectory::MARKER_FILE)
        .is_file());
    let profiles = ProfileStore::new(Arc::new(
        PrivateDirectory::open(fixture.control.join("ssh"), policy()).unwrap(),
    ));
    assert_eq!(
        profiles.list().unwrap()[0].remote_cwd.as_deref(),
        Some("/work")
    );
}

#[test]
fn a_v1_import_gets_the_new_items_without_repeating_the_old_ones() {
    let fixture = Fixture::new();
    fixture.database(&settings());
    fixture.remote();
    // What the v1 build left: settings copied, markers written, no sessions or node.
    let host = Host::open(&fixture.data).unwrap();
    let snapshot = fixture.electron.join(files::DATABASE);
    assert!(host.import_electron_settings(&snapshot).unwrap());
    drop(host);
    write_marker(&fixture.control, MARKER, &false, &|| Ok(())).unwrap();
    write_marker(&fixture.control, WINDOW_MARKER, &false, &|| Ok(())).unwrap();
    let report = fixture.run(true).unwrap().unwrap();
    assert!(!report.settings);
    assert_eq!((report.sessions, report.approved), (1, 0));
    assert!(report.remote_preferences);
    assert_eq!(report.node, Some(remote::Node::Adopted));
    assert!(Host::open(&fixture.data)
        .unwrap()
        .approval_snapshot()
        .unwrap()
        .approvals
        .is_empty());
    assert!(fixture.run(true).unwrap().is_none());
}

#[test]
fn a_rust_store_in_use_keeps_its_own_state() {
    let fixture = Fixture::new();
    fixture.database(&settings());
    fixture.remote();
    let host = Host::open(&fixture.data).unwrap();
    host.update_preferences(shellfox_core::PreferencesPatch {
        font_size: Some(17.),
        ..Default::default()
    })
    .unwrap();
    drop(host);
    let own = br#"{"version":1,"enabled":false,"hostname":"rust-node","peers":[]}"#;
    fs::write(fixture.control.join("remote-settings-v1.json"), own).unwrap();
    let first = fixture.run(true).unwrap().unwrap();
    assert!(!first.settings && !first.remote_preferences);
    assert_eq!(first.sessions, 0);
    // Give the Rust node an identity of its own, then run the v2 pass again.
    let leaf_state = fixture.leaf().join("tailscaled.state");
    let rust_node = br#"{"_machinekey":"cnVzdA=="}"#;
    fs::write(&leaf_state, rust_node).unwrap();
    fs::remove_file(fixture.control.join(MARKER_V2)).unwrap();
    let report = fixture.run(true).unwrap().unwrap();
    assert_eq!(report.node, Some(remote::Node::RustHasState));
    assert!(!report.settings && !report.remote_preferences);
    assert_eq!(report.sessions, 0);
    assert_eq!(fs::read(&leaf_state).unwrap(), rust_node);
    assert_eq!(
        fs::read(fixture.control.join("remote-settings-v1.json")).unwrap(),
        own
    );
    assert!(Host::open(&fixture.data)
        .unwrap()
        .snapshot()
        .unwrap()
        .sessions
        .is_empty());
}

#[test]
fn a_running_electron_defers_the_import_and_the_cleanup() {
    let fixture = Fixture::new();
    fixture.database(&settings());
    fixture.remote();
    fs::create_dir(fixture.electron.join(".host-lock")).unwrap();
    let lock = fs::File::create(fixture.electron.join(".host-lock/host.lock")).unwrap();
    lock.lock().unwrap();
    assert!(fixture
        .run(true)
        .unwrap_err()
        .to_string()
        .contains("running"));
    assert!(!fixture.control.join(MARKER).exists() && !fixture.control.join(MARKER_V2).exists());
    assert!(!fixture.leaf().exists());
    lock.unlock().unwrap();
    assert!(fixture.run(true).unwrap().is_some());
    fixture.age_import();
    lock.lock().unwrap();
    assert!(fixture.run(true).is_err());
    assert!(fixture.electron.join(remote::REMOTE).exists());
    lock.unlock().unwrap();
}

#[test]
fn caches_go_after_the_import_and_electron_data_after_the_retention() {
    let fixture = Fixture::new();
    fixture.database(&settings());
    fixture.remote();
    for dir in [
        "Cache/Cache_Data",
        "GPUCache",
        "Service Worker/CacheStorage/x",
        "uploads",
        "host",
    ] {
        fs::create_dir_all(fixture.electron.join(dir)).unwrap();
    }
    fs::write(fixture.electron.join("Cache/Cache_Data/f_000001"), b"cache").unwrap();
    fs::write(
        fixture.electron.join("Service Worker/Database"),
        b"kept until expiry",
    )
    .unwrap();
    fs::write(fixture.electron.join("Local State"), b"{}").unwrap();
    fs::write(fixture.electron.join("notes.txt"), b"unknown").unwrap();
    fs::write(fixture.electron.join("uploads/photo.jpg"), b"user file").unwrap();
    fixture.run(true).unwrap().unwrap();
    for gone in ["Cache", "GPUCache", "Service Worker/CacheStorage"] {
        assert!(!fixture.electron.join(gone).exists(), "{gone}");
    }
    for kept in [
        "Service Worker/Database",
        "Local State",
        remote::REMOTE,
        "tailscale",
        files::DATABASE,
    ] {
        assert!(fixture.electron.join(kept).exists(), "{kept}");
    }
    // Before the retention ends nothing else is touched.
    assert!(fixture.run(true).unwrap().is_none());
    assert!(fixture.electron.join("tailscale/tailscaled.state").exists());
    fixture.age_import();
    assert!(fixture.run(true).unwrap().is_none());
    for gone in [
        files::DATABASE,
        remote::REMOTE,
        "tailscale",
        "Local State",
        "Service Worker",
    ] {
        assert!(!fixture.electron.join(gone).exists(), "{gone}");
    }
    for kept in ["notes.txt", "uploads/photo.jpg", "host", "rust-v1/host"] {
        assert!(fixture.electron.join(kept).exists(), "{kept}");
    }
    assert!(fixture.control.join(CLEANUP_MARKER).is_file());
    assert!(fixture.leaf().join("tailscaled.state").is_file());
    let host = Host::open(&fixture.data).unwrap();
    assert_eq!(host.snapshot().unwrap().sessions.len(), 1);
}
