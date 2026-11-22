#![cfg(feature = "test-support")]
#[cfg(windows)]
#[path = "support/elevated_hold.rs"]
mod hold;
#[path = "support/elevated_fixture.rs"]
mod support;
use shellfox_terminal::elevated::{
    fixture_session, AdapterError, Event, FixtureScenario, Generation, Phase, Session,
};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use support::Fixture;

fn generation() -> Generation {
    Generation::new(11).unwrap()
}
fn next(session: &mut Session) -> Event {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(event) = session.poll(generation()).unwrap() {
            return event;
        }
        assert!(Instant::now() < deadline, "owned helper event timed out");
        std::thread::sleep(Duration::from_millis(2));
    }
}
#[test]
fn owned_stdio_delivers_bytes_ordered_acks_resize_and_final_output_before_close() {
    let fixture = Fixture::new();
    let mut session = fixture_session(
        fixture.verified(),
        (),
        FixtureScenario::Normal,
        generation(),
        fixture.launch(),
    )
    .unwrap();
    assert_eq!(session.phase(), Phase::Starting);
    assert!(matches!(next(&mut session), Event::Ready(_)));
    assert_eq!(session.identity().unwrap().pid.get(), 424242);
    assert_eq!(
        session.write(generation(), "owned 雪".as_bytes()).unwrap(),
        1
    );
    session.resize(generation(), 100, 500).unwrap();
    let mut bytes = Vec::new();
    let mut acknowledged = false;
    while !acknowledged {
        match next(&mut session) {
            Event::Data(data) => bytes.extend(data),
            Event::InputAcknowledged(1) => acknowledged = true,
            other => panic!("unexpected owned fixture event {other:?}"),
        }
    }
    session.begin_close(generation()).unwrap();
    loop {
        match next(&mut session) {
            Event::Data(data) => bytes.extend(data),
            Event::ExitConfirmed(0) => break,
            other => panic!("unexpected owned fixture event {other:?}"),
        }
    }
    let output = String::from_utf8(bytes).unwrap();
    assert!(
        output.contains("OWNED_ELEVATED_FIXTURE")
            && output.contains("owned 雪")
            && output.contains("OWNED_FINAL_BYTES")
    );
    assert!(!session.cleanup_confirmed());
    session
        .finish(generation(), Duration::from_secs(3))
        .unwrap();
    assert!(session.cleanup_confirmed());
    assert!(session.observation().launcher_exited());
    assert!(session.observation().peer_exit_confirmed());
    assert!(!session.observation().resources_held());
}

struct Hold(Arc<AtomicBool>);
impl Drop for Hold {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}
#[test]
fn launcher_exit_does_not_release_resource_lease_before_authenticated_peer_exit() {
    let fixture = Fixture::new();
    let dropped = Arc::new(AtomicBool::new(false));
    let mut session = fixture_session(
        fixture.verified(),
        Hold(Arc::clone(&dropped)),
        FixtureScenario::LauncherFirst,
        generation(),
        fixture.launch(),
    )
    .unwrap();
    assert!(matches!(next(&mut session), Event::Ready(_)));
    let deadline = Instant::now() + Duration::from_secs(5);
    while !session.observation().launcher_exited() {
        session.service(generation()).unwrap();
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(!session.observation().peer_exit_confirmed());
    assert!(session.observation().resources_held());
    assert!(!dropped.load(Ordering::Acquire));
    assert_eq!(
        session.finish(generation(), Duration::ZERO),
        Err(AdapterError::CleanupUnconfirmed)
    );
    assert!(matches!(next(&mut session), Event::ExitConfirmed(0)));
    session
        .finish(generation(), Duration::from_secs(3))
        .unwrap();
    assert!(dropped.load(Ordering::Acquire));
}

#[test]
fn dropped_view_transfers_original_child_and_lease_to_prestarted_cleanup_owner() {
    let fixture = Fixture::new();
    let dropped = Arc::new(AtomicBool::new(false));
    let mut session = fixture_session(
        fixture.verified(),
        Hold(Arc::clone(&dropped)),
        FixtureScenario::LauncherFirst,
        generation(),
        fixture.launch(),
    )
    .unwrap();
    assert!(matches!(next(&mut session), Event::Ready(_)));
    let observation = session.observation();
    drop(session);
    let deadline = Instant::now() + Duration::from_secs(5);
    while observation.resources_held() {
        assert!(
            Instant::now() < deadline,
            "cleanup owner failed to retain/drain peer evidence"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(observation.launcher_exited() && observation.peer_exit_confirmed());
    assert!(dropped.load(Ordering::Acquire));
}

#[test]
fn denial_confirms_no_peer_started_without_claiming_ready() {
    let fixture = Fixture::new();
    let mut session = fixture_session(
        fixture.verified(),
        (),
        FixtureScenario::Cancelled,
        generation(),
        fixture.launch(),
    )
    .unwrap();
    assert_eq!(next(&mut session), Event::Failed(AdapterError::Cancelled));
    assert!(!session.cleanup_confirmed());
    assert_eq!(next(&mut session), Event::ExitConfirmed(1));
    session
        .finish(generation(), Duration::from_secs(3))
        .unwrap();
    assert!(session.cleanup_confirmed());
    assert!(session.identity().is_none());
}

#[cfg(windows)]
#[test]
fn resource_hold_remains_locked_beyond_launcher_exit() {
    let fixture = Fixture::new();
    let lock = hold::lock(&fixture);
    let weak = Arc::downgrade(&lock);
    let lazy = fixture.root.join("lazy.dll");
    let mut session = fixture_session(
        fixture.verified(),
        Arc::clone(&lock),
        FixtureScenario::LauncherFirst,
        generation(),
        fixture.launch(),
    )
    .unwrap();
    drop(lock);
    assert!(matches!(next(&mut session), Event::Ready(_)));
    let deadline = Instant::now() + Duration::from_secs(5);
    while !session.observation().launcher_exited() {
        session.service(generation()).unwrap();
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(weak.upgrade().is_some());
    assert!(std::fs::write(&lazy, b"must remain protected").is_err());
    assert!(!session.observation().peer_exit_confirmed());
    assert!(matches!(next(&mut session), Event::ExitConfirmed(0)));
    session
        .finish(generation(), Duration::from_secs(3))
        .unwrap();
    assert!(weak.upgrade().is_none());
    std::fs::write(lazy, b"released only after both exit proofs").unwrap();
}

#[cfg(windows)]
#[test]
fn isolated_launch_matches_closed_windows_powershell_environment_contract() {
    use shellfox_terminal::{elevated::Launch, EnvironmentPolicy};
    let fixture = Fixture::new();
    let root = fixture.root.join("private-state");
    std::fs::create_dir(&root).unwrap();
    let launch = Launch::isolated(&fixture.root, &root, 80, 24, EnvironmentPolicy::Minimal)
        .unwrap()
        .prepare()
        .unwrap();
    let value: serde_json::Value = serde_json::from_slice(launch.as_bytes()).unwrap();
    assert_eq!(value["type"], "launch-isolated");
    assert!(value.get("executable").is_none() && value.get("args").is_none());
    let env = value["env"].as_object().unwrap();
    let relative = [
        ("HOME", "home"),
        ("USERPROFILE", "home"),
        ("ZDOTDIR", "home"),
        ("APPDATA", "config"),
        ("XDG_CONFIG_HOME", "config"),
        ("LOCALAPPDATA", "data"),
        ("XDG_DATA_HOME", "data"),
        ("XDG_CACHE_HOME", "cache"),
        ("npm_config_cache", "cache"),
        ("XDG_STATE_HOME", "state"),
        ("TMP", "temp"),
        ("TEMP", "temp"),
        ("TMPDIR", "temp"),
        ("PI_CODING_AGENT_DIR", "pi-agent"),
        ("PI_CODING_AGENT_SESSION_DIR", "pi-sessions"),
        ("npm_config_prefix", "npm-prefix"),
        ("PNPM_HOME", "pnpm-home"),
        ("HISTFILE", "state/shell-history"),
        ("PSREADLINE_HISTORYFILE", "state/powershell-history"),
        ("npm_config_userconfig", "config/npmrc"),
        ("npm_config_globalconfig", "config/npm-globalrc"),
        ("GIT_CONFIG_GLOBAL", "config/gitconfig"),
        ("GIT_CONFIG_SYSTEM", "config/gitconfig-system"),
    ];
    let normalize = |text: &str| text.replace('/', "\\").to_uppercase();
    let root = value["isolationRoot"].as_str().unwrap();
    for (name, path) in relative {
        assert_eq!(
            normalize(env[name].as_str().unwrap()),
            normalize(&std::path::Path::new(root).join(path).to_string_lossy())
        );
    }
    assert_eq!(env.len(), relative.len() + 8);
    let modules = env["PSModulePath"].as_str().unwrap();
    assert!(!modules.contains(';'));
    assert!(normalize(modules).ends_with("\\WINDOWSPOWERSHELL\\V1.0\\MODULES"));
    assert_eq!(env["PATHEXT"], ".COM;.EXE;.BAT;.CMD");
}
