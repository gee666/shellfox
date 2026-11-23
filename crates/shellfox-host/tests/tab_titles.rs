use shellfox_host::{AddTab, CreateSession, Host, ProfileId, SessionId};

#[test]
fn saving_the_same_title_disables_live_titles_and_survives_reopen() {
    let scratch = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tmp");
    std::fs::create_dir_all(&scratch).unwrap();
    let directory = tempfile::tempdir_in(scratch).unwrap();
    let host = Host::open(directory.path()).unwrap();
    let created = host
        .create_session(CreateSession {
            request_id: SessionId::new(),
            title: None,
            cwd: "/work/title-test".into(),
            profile_id: ProfileId::new("login-shell").unwrap(),
        })
        .unwrap();
    let session = &created.sessions[0];
    let session_id = session.id;
    let tab = &session.tabs[0];
    assert!(tab.use_process_title);
    let renamed = host.rename_tab(tab.id, &tab.title).unwrap();
    assert!(renamed.revision > created.revision);
    assert!(!renamed.sessions[0].tabs[0].use_process_title);
    let added = host
        .add_tab(
            session_id,
            AddTab {
                cwd: None,
                profile_id: ProfileId::new("login-shell").unwrap(),
                title: Some("User label".into()),
            },
        )
        .unwrap();
    assert!(!added.sessions[0].tabs[1].use_process_title);
    drop(host);
    let snapshot = Host::open(directory.path()).unwrap().snapshot().unwrap();
    assert_eq!(snapshot.sessions[0].tabs[0].title, tab.title);
    assert!(snapshot.sessions[0]
        .tabs
        .iter()
        .all(|tab| !tab.use_process_title));
}
