use shellfox_host::{
    CreateSession, Generation, Host, ProfileId, SessionId, TerminalIdentity, WslLaunchAccount,
    WslLaunchBinding, STORE_FILENAME,
};
use std::path::Path;

fn scratch() -> tempfile::TempDir {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tmp");
    std::fs::create_dir_all(&root).unwrap();
    tempfile::Builder::new()
        .prefix("wsl-launch-")
        .tempdir_in(root)
        .unwrap()
}

fn add_wsl_tab(host: &Host) -> TerminalIdentity {
    let initial = host
        .create_session(CreateSession {
            request_id: SessionId::new(),
            title: Some("WSL fixture".into()),
            cwd: r"\\wsl.localhost\Fixture\home\alpha".into(),
            profile_id: ProfileId::new("wsl:Fixture").unwrap(),
        })
        .unwrap();
    let tab = &initial.sessions[0].tabs[0];
    assert_eq!(tab.wsl_launch, Some(WslLaunchBinding::Pending));
    assert!(tab.wsl_account().is_none());
    TerminalIdentity {
        tab_id: tab.id,
        generation: Generation::new(),
    }
}

fn account(uid: u32, name: &str) -> WslLaunchAccount {
    serde_json::from_value(serde_json::json!({"uid": uid, "name": name})).unwrap()
}

#[test]
fn verified_account_is_written_once_and_cannot_follow_a_new_default() {
    let dir = scratch();
    let host = Host::open(dir.path()).unwrap();
    let launch = add_wsl_tab(&host);
    let alpha = account(1101, "alpha");
    let beta = account(1102, "beta");
    assert!(host
        .record_wsl_launch_account(launch, alpha.clone())
        .is_err());
    host.register_terminal(launch).unwrap();
    let stale = TerminalIdentity {
        generation: Generation::new(),
        ..launch
    };
    assert!(host.record_wsl_launch_account(stale, beta.clone()).is_err());
    host.record_wsl_launch_account(launch, alpha.clone())
        .unwrap();
    let revision = host.snapshot().unwrap().revision;
    host.record_wsl_launch_account(launch, alpha.clone())
        .unwrap();
    assert_eq!(host.snapshot().unwrap().revision, revision);
    assert!(host
        .record_wsl_launch_account(launch, beta.clone())
        .is_err());
    host.terminal_stopped(launch).unwrap();
    drop(host);

    let reopened = Host::open(dir.path()).unwrap();
    let intent = reopened.terminal_launch(launch.tab_id).unwrap();
    assert_eq!(
        intent.wsl_launch,
        Some(WslLaunchBinding::Bound {
            account: alpha.clone()
        })
    );
    let next = TerminalIdentity {
        generation: Generation::new(),
        ..launch
    };
    reopened.register_terminal(next).unwrap();
    assert!(reopened.record_wsl_launch_account(next, beta).is_err());
    assert_eq!(
        reopened.snapshot().unwrap().sessions[0].tabs[0].wsl_account(),
        Some(&alpha)
    );
}

#[test]
fn legacy_tab_without_uid_cannot_adopt_the_active_default() {
    let dir = scratch();
    let host = Host::open(dir.path()).unwrap();
    let launch = add_wsl_tab(&host);
    drop(host);
    let db = rusqlite::Connection::open(dir.path().join(STORE_FILENAME)).unwrap();
    let value: String = db
        .query_row(
            "SELECT value FROM tabs WHERE id=?1",
            [launch.tab_id.to_string()],
            |row| row.get(0),
        )
        .unwrap();
    let mut legacy: serde_json::Value = serde_json::from_str(&value).unwrap();
    assert!(legacy
        .as_object_mut()
        .unwrap()
        .remove("wslLaunch")
        .is_some());
    db.execute(
        "UPDATE tabs SET value=?1 WHERE id=?2",
        [legacy.to_string(), launch.tab_id.to_string()],
    )
    .unwrap();
    drop(db);
    let host = Host::open(dir.path()).unwrap();
    host.register_terminal(launch).unwrap();
    assert!(host.snapshot().unwrap().sessions[0].tabs[0]
        .wsl_account()
        .is_none());
    assert!(host
        .record_wsl_launch_account(launch, account(1102, "beta"))
        .is_err());
}
