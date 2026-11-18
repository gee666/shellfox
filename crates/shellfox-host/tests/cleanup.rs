use shellfox_host::*;
use std::path::PathBuf;
struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tmp")
            .join(format!("host-cleanup-{}", SessionId::new()));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
#[test]
fn commit_requires_every_current_generation_to_be_confirmed_stopped() {
    let scratch = Scratch::new();
    let host = Host::open(&scratch.0).unwrap();
    let snapshot = host
        .create_session(CreateSession {
            request_id: SessionId::new(),
            title: None,
            cwd: "/work".into(),
            profile_id: ProfileId::default(),
        })
        .unwrap();
    let id = snapshot.sessions[0].id;
    let snapshot = host
        .add_tab(
            id,
            AddTab {
                title: None,
                cwd: None,
                profile_id: ProfileId::default(),
            },
        )
        .unwrap();
    let first = TerminalIdentity {
        tab_id: snapshot.sessions[0].tabs[0].id,
        generation: Generation::new(),
    };
    let second = TerminalIdentity {
        tab_id: snapshot.sessions[0].tabs[1].id,
        generation: Generation::new(),
    };
    host.register_terminal(first).unwrap();
    host.register_terminal(second).unwrap();
    let prepared = host.prepare_delete_session(id).unwrap();
    assert_eq!(prepared.terminals.len(), 2);
    assert_eq!(
        host.commit_close(prepared.token).unwrap_err().code,
        ErrorCode::CleanupRequired
    );
    assert_eq!(host.snapshot().unwrap(), snapshot);
    assert_eq!(
        host.terminal_stopped(TerminalIdentity {
            generation: Generation::new(),
            ..first
        })
        .unwrap_err()
        .code,
        ErrorCode::Conflict
    );
    assert_eq!(
        host.commit_close(prepared.token).unwrap_err().code,
        ErrorCode::CleanupRequired
    );
    host.terminal_stopped(first).unwrap();
    assert_eq!(
        host.commit_close(prepared.token).unwrap_err().code,
        ErrorCode::CleanupRequired
    );
    assert_eq!(host.snapshot().unwrap(), snapshot);
    assert!(host
        .register_terminal(TerminalIdentity {
            generation: Generation::new(),
            ..first
        })
        .is_err());
    host.terminal_stopped(second).unwrap();
    let closed = host.commit_close(prepared.token).unwrap();
    assert!(closed.sessions.is_empty());
    assert_eq!(closed.revision, snapshot.revision + 1);
    assert!(host.commit_close(prepared.token).is_err());
}

#[test]
fn canceled_cleanup_retains_owner_and_can_be_prepared_again() {
    let scratch = Scratch::new();
    let host = Host::open(&scratch.0).unwrap();
    let snapshot = host
        .create_session(CreateSession {
            request_id: SessionId::new(),
            title: None,
            cwd: "/work".into(),
            profile_id: ProfileId::default(),
        })
        .unwrap();
    let tab_id = snapshot.sessions[0].tabs[0].id;
    let owner = TerminalIdentity {
        tab_id,
        generation: Generation::new(),
    };
    host.register_terminal(owner).unwrap();
    let first = host.prepare_close_tab(tab_id).unwrap();
    host.cancel_close(first.token).unwrap();
    assert_eq!(
        host.close_tab(tab_id).unwrap_err().code,
        ErrorCode::CleanupRequired
    );
    let retry = host.prepare_close_tab(tab_id).unwrap();
    assert_eq!(retry.terminals, vec![owner]);
    host.terminal_stopped(owner).unwrap();
    assert!(host.commit_close(retry.token).unwrap().sessions[0]
        .tabs
        .is_empty());
}
