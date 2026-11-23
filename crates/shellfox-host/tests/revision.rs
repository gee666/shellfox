use shellfox_host::*;
use std::sync::Arc;

struct DenyCommit;
impl SettingsRuntime for DenyCommit {
    fn normalize(&self, _: &Settings, candidate: Settings) -> Result<Settings> {
        Ok(candidate)
    }
    fn check_commit(&self) -> Result<()> {
        Err(Error::new(ErrorCode::Internal, "Denied before commit."))
    }
}

fn scratch() -> tempfile::TempDir {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tmp");
    std::fs::create_dir_all(&root).unwrap();
    tempfile::tempdir_in(root).unwrap()
}

#[test]
fn revision_follows_commits_and_survives_reopen() {
    let directory = scratch();
    let host = Host::open(directory.path()).unwrap();
    assert_eq!(host.revision(), host.snapshot().unwrap().revision);
    let created = host
        .create_session(CreateSession {
            request_id: SessionId::new(),
            title: None,
            cwd: "/work/revision".into(),
            profile_id: ProfileId::new("login-shell").unwrap(),
        })
        .unwrap();
    assert_eq!(host.revision(), created.revision);
    let tab = created.sessions[0].tabs[0].id;
    let renamed = host.rename_tab(tab, "Renamed").unwrap();
    assert!(renamed.revision > created.revision);
    assert_eq!(host.revision(), renamed.revision);
    let settings = host
        .save_global_settings(SettingsPatch {
            accent_color: Some("#123456".into()),
            ..Default::default()
        })
        .unwrap();
    drop(settings);
    let after_settings = host.revision();
    assert!(after_settings > renamed.revision);
    assert_eq!(after_settings, host.snapshot().unwrap().revision);
    drop(host);
    assert_eq!(
        Host::open(directory.path()).unwrap().revision(),
        after_settings
    );
}

#[test]
fn rolled_back_write_publishes_no_revision() {
    let directory = scratch();
    let host = Host::open(directory.path()).unwrap();
    let before = host.revision();
    host.install_settings_runtime(Arc::new(DenyCommit)).unwrap();
    assert!(host
        .update_preferences(PreferencesPatch {
            accent_color: Some("#654321".into()),
            ..Default::default()
        })
        .is_err());
    assert_eq!(host.revision(), before);
    assert_eq!(host.snapshot().unwrap().revision, before);
}
