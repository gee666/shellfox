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
    assert!(matches!(action(&["ss", "list"]), Action::Manager(_)));
    assert!(matches!(action(&["tm", "list"]), Action::Manager(_)));
    // 0.9 usage errors for a bare or unknown operation, never a fallback.
    for (words, message) in [
        (
            &["session"][..],
            "Unknown session operation. Run shellfox session --help.",
        ),
        (&["tm"][..], "Unknown terminals operation."),
        (&["host"][..], "Usage: shellfox host run|status|stop"),
        (
            &["host", "keep-awake", "maybe"][..],
            "Usage: shellfox host keep-awake [on|off]",
        ),
        (
            &["settings"][..],
            "Usage: shellfox settings read|update --input <JSON>",
        ),
        (&["settings", "update"][..], "Provide --input with JSON."),
        (
            &["settings", "update", "--input", "[1]"][..],
            "Settings input must be a JSON object.",
        ),
        (
            &["approvals", "approve", "not a peer"][..],
            "Invalid peer ID.",
        ),
        (&["machines", "frob"][..], "Unknown machines operation."),
    ] {
        let error = parse_words(words).unwrap_err();
        let error = error.downcast_ref::<crate::output::CliError>().unwrap();
        assert!(
            error.code == "VALIDATION" && error.message == message,
            "{words:?}: {}",
            error.message
        );
    }
    // 0.9 also took --input=JSON.
    assert_eq!(
        action(&["settings", "update", r#"--input={"fontSize":14}"#]),
        action(&["settings", "update", "--input", r#"{"fontSize":14}"#])
    );
    assert_eq!(
        action(&["session", "pin", "--help"]),
        Action::Help(Some("session".into()))
    );
    assert!(parse_words(&["help", "nope"]).is_err());
    assert_eq!(
        host_words(&["network", "enable", "--hostname", "box"]),
        ["host", "network", "on", "--hostname", "box"]
    );
    assert_eq!(
        host_words(&["approvals", "approve", "P"]),
        ["host", "network", "peer", "P", "approve"]
    );
    assert_eq!(
        action(&["host", "keep-awake", "on"]),
        Action::KeepAwake(Some(true))
    );
    assert_eq!(
        host_words(&["network", "login", "--auth-key-file=/k"]),
        ["host", "network", "login", "--auth-key-file", "/k"]
    );
    assert_eq!(
        action(&["completion", "bash"]),
        Action::Completion(Some("bash".into()))
    );
    assert_eq!(
        action(&["update", "--check", "--json"]),
        Action::Update { check: true }
    );
    assert_eq!(action(&["--version"]), Action::Version);
    assert_eq!(action(&[]), Action::Help(None));
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
    assert!(parse_words(&["session", "frobnicate"]).is_err());
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
    let remote = parse_words(&["--host", "other", "session", "list"]).unwrap();
    assert_eq!(remote.host.as_deref(), Some("other"));
    assert!(parse_words(&["--host", "local", "session", "list"])
        .unwrap()
        .host
        .is_none());
    for local in [
        &["host", "stop"][..],
        &["network", "status"],
        &["run", "--", "x"],
        &["cmd", "--ls"],
        &["remote", "api", "getState"],
    ] {
        let words: Vec<&str> = ["--host", "other"]
            .into_iter()
            .chain(local.iter().copied())
            .collect();
        let error = parse_words(&words).unwrap_err();
        assert_eq!(
            error.downcast_ref::<CliError>().unwrap().code,
            "VALIDATION",
            "{local:?}"
        );
    }
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

#[test]
fn everything_after_a_custom_command_name_is_the_scripts() {
    let script = parse_words(&["cmd", "deploy", "--json", "--yes", "--host", "x"]).unwrap();
    assert!(!script.json && !script.yes && script.host.is_none());
    assert_eq!(
        script.action,
        Action::Commands(
            ["deploy", "--json", "--yes", "--host", "x"]
                .map(String::from)
                .to_vec()
        )
    );
    let run = parse_words(&["commands", "--run", "deploy", "--json"]).unwrap();
    assert!(!run.json);
    let admin = parse_words(&["cmd", "--register", "a", "/s.sh", "--json"]).unwrap();
    assert!(admin.json);
    assert_eq!(
        admin.action,
        Action::Commands(["--register", "a", "/s.sh"].map(String::from).to_vec())
    );
}
