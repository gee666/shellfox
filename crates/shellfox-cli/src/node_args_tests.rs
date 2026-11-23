use super::*;

fn parse_words(words: &[&str]) -> Result<Invocation> {
    let arguments: Vec<OsString> = words.iter().map(OsString::from).collect();
    parse(&arguments)
}
fn action(words: &[&str]) -> Action {
    parse_words(words).unwrap().action
}
fn host_words(words: &[&str]) -> Vec<String> {
    match action(words) {
        Action::Host { words, .. } => words,
        other => panic!("expected host words, got {other:?}"),
    }
}

#[test]
fn service_unit_command_lines_from_0_9_keep_working() {
    let invocation = parse_words(&["--user-data", "/srv/shellfox-data", "host", "run"]).unwrap();
    assert_eq!(invocation.action, Action::HostRun);
    assert_eq!(
        invocation.user_data().unwrap(),
        std::path::absolute("/srv/shellfox-data").unwrap()
    );
    assert_eq!(
        data_dir(&invocation.user_data().unwrap()),
        invocation.user_data().unwrap().join("rust-v1").join("host")
    );
    let trailing = parse_words(&["host", "status", "--json", "--user-data=/srv/d"]).unwrap();
    assert!(trailing.json);
    assert_eq!(trailing.action, Action::HostStatus);
    let stop = parse_words(&["--user-data", "/srv/d", "host", "stop", "--yes", "--json"]).unwrap();
    assert!(stop.yes && stop.json);
    assert_eq!(stop.action, Action::HostStop);
}

#[test]
fn namespaces_and_aliases_translate_to_host_commands() {
    assert_eq!(host_words(&["ss", "list"]), ["session", "list"]);
    assert_eq!(host_words(&["tm", "list", "S"]), ["tab", "list", "S"]);
    assert_eq!(
        host_words(&["terminals", "rename", "S", "T", "new"]),
        ["tab", "rename", "T", "new"]
    );
    assert_eq!(
        host_words(&["network", "enable", "--hostname", "box"]),
        ["host", "network", "on", "--hostname", "box"]
    );
    assert_eq!(
        host_words(&["approvals", "approve", "P"]),
        ["host", "network", "peer", "P", "approve"]
    );
    assert_eq!(
        host_words(&["host", "keep-awake", "on"]),
        ["host", "power", "on"]
    );
    match action(&["session", "rm", "ID"]) {
        Action::Host { words, confirm } => {
            assert_eq!(words, ["session", "close", "ID"]);
            assert!(confirm.is_some());
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(
        action(&["update", "--check", "--json"]),
        Action::Update { check: true }
    );
    assert_eq!(action(&["--version"]), Action::Version);
    assert_eq!(action(&[]), Action::Help);
}

#[test]
fn run_arguments_after_double_dash_stay_literal() {
    let words = host_words(&[
        "run",
        "--cwd",
        "/w",
        "--",
        "node",
        "--json",
        "--user-data",
        "x",
    ]);
    assert_eq!(
        words,
        [
            "run",
            "--cwd",
            "/w",
            "--",
            "node",
            "--json",
            "--user-data",
            "x"
        ]
    );
    let invocation = parse_words(&["run", "--", "tool", "--yes"]).unwrap();
    assert!(!invocation.yes);
}

#[test]
fn option_values_are_never_globals_and_errors_are_validation() {
    match action(&["session", "create", "/", "--title", "--json"]) {
        Action::CreateSession { title, .. } => assert_eq!(title.as_deref(), Some("--json")),
        other => panic!("{other:?}"),
    }
    for bad in [
        &["--json=1", "version"][..],
        &["--user-data", "a", "--user-data", "b", "version"],
        &["update", "now"],
    ] {
        let error = parse_words(bad).unwrap_err();
        assert_eq!(
            error.downcast_ref::<CliError>().unwrap().code,
            "VALIDATION",
            "{bad:?}"
        );
    }
    assert_eq!(
        action(&["session", "stats"]),
        Action::Unknown(vec!["session".into(), "stats".into()])
    );
    assert_eq!(action(&["host", "serve"]), Action::HostRun);
    match action(&[
        "session",
        "create",
        ".",
        "--request-id",
        "0b6f8f5e-6f62-4c1b-9d0e-6f1b1a2b3c4d",
    ]) {
        Action::CreateSession { request_id, .. } => assert!(request_id.is_some()),
        other => panic!("{other:?}"),
    }
    assert!(parse_words(&["session", "create", ".", "--request-id", "nope"]).is_err());
    let remote = parse_words(&["--host", "other", "session", "list"]).unwrap_err();
    assert_eq!(
        remote.downcast_ref::<CliError>().unwrap().code,
        "UNSUPPORTED"
    );
    assert!(parse_words(&["--host", "local", "session", "list"]).is_ok());
}

#[test]
fn old_host_terminal_shims_and_path_spellings_resolve_like_0_9() {
    let shim = parse_words(&["--shellfox-default-user-data", "/srv/a", "session", "list"]).unwrap();
    assert_eq!(
        shim.user_data().unwrap(),
        std::path::absolute("/srv/a").unwrap()
    );
    let explicit = parse_words(&[
        "--shellfox-default-user-data",
        "/srv/a",
        "--user-data",
        "/srv/b",
        "version",
    ])
    .unwrap();
    assert_eq!(
        explicit.user_data().unwrap(),
        std::path::absolute("/srv/b").unwrap()
    );
    assert!(parse_words(&["--shellfox-default-user-data", "a\nb", "version"]).is_err());
    let messy = parse_words(&["--user-data", "/srv/x/../data/./", "version"]).unwrap();
    assert_eq!(
        messy.user_data().unwrap(),
        std::path::absolute("/srv/data").unwrap()
    );
}
