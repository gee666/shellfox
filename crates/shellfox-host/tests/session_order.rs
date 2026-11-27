//! 0.9.15 list order and new-tab rules the CLI and every client rely on.
use shellfox_host::{AddTab, CreateSession, Host, ProfileId, SessionId};

fn host() -> (tempfile::TempDir, Host) {
    let scratch = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tmp");
    std::fs::create_dir_all(&scratch).unwrap();
    let directory = tempfile::tempdir_in(scratch).unwrap();
    let host = Host::open(directory.path()).unwrap();
    (directory, host)
}

fn create(host: &Host, title: &str) -> SessionId {
    let snapshot = host
        .create_session(CreateSession {
            request_id: SessionId::new(),
            title: Some(title.into()),
            cwd: "/work/order".into(),
            profile_id: ProfileId::new("login-shell").unwrap(),
        })
        .unwrap();
    snapshot.selection.session_id.unwrap()
}

fn untitled() -> AddTab {
    AddTab {
        title: None,
        cwd: None,
        profile_id: ProfileId::new("login-shell").unwrap(),
    }
}

#[test]
fn snapshot_lists_pins_first_then_unsettled_then_newest() {
    let (_directory, host) = host();
    let oldest = create(&host, "oldest");
    std::thread::sleep(std::time::Duration::from_millis(5));
    let settled = create(&host, "settled");
    std::thread::sleep(std::time::Duration::from_millis(5));
    let first_pin = create(&host, "first pin");
    std::thread::sleep(std::time::Duration::from_millis(5));
    create(&host, "newest");
    host.set_session_pinned(first_pin, true).unwrap();
    host.set_session_pinned(oldest, true).unwrap();
    host.set_session_settled(first_pin, true).unwrap();
    host.set_session_settled(settled, true).unwrap();
    let order: Vec<_> = host
        .snapshot()
        .unwrap()
        .sessions
        .iter()
        .map(|session| session.title.clone())
        .collect();
    assert_eq!(order, ["oldest", "first pin", "newest", "settled"]);
}

#[test]
fn adding_a_tab_keeps_the_session_settled() {
    let (_directory, host) = host();
    let session = create(&host, "work");
    let settled = host.set_session_settled(session, true).unwrap();
    let added = host.add_tab(session, untitled()).unwrap();
    assert_eq!(added.sessions[0].settled_at, settled.sessions[0].settled_at);
    assert!(added.sessions[0].settled_at.is_some());
}

#[test]
fn a_default_title_replaces_shell_n_but_keeps_live_titles() {
    let (_directory, host) = host();
    let session = create(&host, "work");
    let label = "Administrator Windows PowerShell";
    let added = host
        .add_tab_with_default_title(session, untitled(), label)
        .unwrap();
    let tab = &added.sessions[0].tabs[1];
    assert_eq!(tab.title, label);
    assert!(tab.use_process_title);
    let titled = AddTab {
        title: Some("Mine".into()),
        ..untitled()
    };
    let added = host
        .add_tab_with_default_title(session, titled, label)
        .unwrap();
    assert_eq!(added.sessions[0].tabs[2].title, "Mine");
    let plain = host.add_tab(session, untitled()).unwrap();
    assert_eq!(plain.sessions[0].tabs[3].title, "Shell 4");
}
