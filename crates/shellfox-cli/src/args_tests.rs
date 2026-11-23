use super::*;

const ID: &str = "af847e3d-12c9-45de-86a2-1e5bc565bf84";
const DIGEST: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn parse_command_args(words: &[&str]) -> Result<Args> {
    let root = std::env::current_dir()?;
    let mut args = vec![
        OsString::from("--data-dir"),
        root.join("tmp/cli-parser-data").into_os_string(),
        OsString::from("--helper"),
        root.join("tmp/cli-parser-helper").into_os_string(),
        OsString::from("--helper-sha256"),
        OsString::from(DIGEST),
    ];
    args.extend(words.iter().map(OsString::from));
    parse(args)
}

#[test]
fn both_absolute_paths_are_mandatory() {
    assert!(parse(Vec::<OsString>::new()).is_err());
    for flag in ["--data-dir", "--helper"] {
        assert!(parse([OsString::from(flag)]).is_err());
        assert!(parse([OsString::from(flag), OsString::from("relative")]).is_err());
        assert!(parse([
            OsString::from(flag),
            std::env::current_dir().unwrap().into_os_string()
        ])
        .is_err());
    }
    let root = std::env::current_dir().unwrap().into_os_string();
    assert!(parse([
        OsString::from("--data-dir"),
        root.clone(),
        OsString::from("--data-dir"),
        root
    ])
    .is_err());
}

#[test]
fn trusted_digest_is_mandatory_and_has_exact_hex_encoding() {
    let root = std::env::current_dir().unwrap().into_os_string();
    let mut args = vec![
        OsString::from("--data-dir"),
        root.clone(),
        OsString::from("--helper"),
        root,
    ];
    let mut missing = args.clone();
    missing.extend([OsString::from("session"), OsString::from("list")]);
    assert!(parse(missing).is_err());
    args.push(OsString::from("--helper-sha256"));
    assert!(parse(args.clone()).is_err());
    for bad in [
        "",
        "00",
        &"g".repeat(64),
        &"0".repeat(63),
        &"0".repeat(65),
        &"界".repeat(64),
    ] {
        let mut invalid = args.clone();
        invalid.extend([
            OsString::from(bad),
            OsString::from("session"),
            OsString::from("list"),
        ]);
        assert!(parse(invalid).is_err());
    }
    args.push(OsString::from(DIGEST));
    let mut duplicate = args.clone();
    duplicate.extend([OsString::from("--helper-sha256"), OsString::from(DIGEST)]);
    assert!(parse(duplicate).is_err());
    assert_eq!(
        parse_digest(DIGEST).unwrap(),
        parse_digest(&DIGEST.to_uppercase()).unwrap()
    );
    let parsed = parse_command_args(&["session", "list"]).unwrap();
    assert_eq!(
        parsed.helper_sha256[0..8],
        [1, 35, 69, 103, 137, 171, 205, 239]
    );
}

#[test]
fn host_and_list_commands() {
    assert!(matches!(
        parse_command_args(&["host", "serve"]).unwrap().command,
        Command::Serve
    ));
    assert!(matches!(
        parse_command_args(&["host", "stop"]).unwrap().command,
        Command::Request(LocalApiRequest::Stop {})
    ));
    assert!(matches!(
        parse_command_args(&["session", "list"]).unwrap().command,
        Command::Sessions
    ));
    assert!(matches!(
        parse_command_args(&["tab", "list"]).unwrap().command,
        Command::Tabs(None)
    ));
    assert!(
        matches!(parse_command_args(&["tab", "list", ID]).unwrap().command,
        Command::Tabs(Some(id)) if id.to_string() == ID)
    );
}

#[test]
fn session_create_uses_core_input_and_fresh_request_id() {
    let Command::Request(LocalApiRequest::CreateSession { input }) = parse_command_args(&[
        "session",
        "create",
        "--cwd",
        "/work",
        "--title",
        "Build",
        "--profile",
        "cmd",
    ])
    .unwrap()
    .command
    else {
        panic!("Expected create session");
    };
    assert_eq!(input.cwd, "/work");
    assert_eq!(input.title.as_deref(), Some("Build"));
    assert_eq!(input.profile_id.as_str(), "cmd");
    let opaque = "wsl:unavailable fixture";
    let Command::Request(LocalApiRequest::CreateSession { input: other }) =
        parse_command_args(&["session", "create", "--cwd", "/work", "--profile", opaque])
            .unwrap()
            .command
    else {
        panic!("Expected create session");
    };
    assert!(other.title.is_none());
    assert_eq!(other.profile_id.as_str(), opaque);
    assert_ne!(input.request_id, other.request_id);
}

#[test]
fn tab_create_can_inherit_cwd_but_profile_is_explicit() {
    let Command::Request(LocalApiRequest::AddTab { id, input }) =
        parse_command_args(&["tab", "create", ID, "--profile", "pwsh"])
            .unwrap()
            .command
    else {
        panic!("Expected add tab");
    };
    assert_eq!(id.to_string(), ID);
    assert!(input.cwd.is_none());
    assert!(input.title.is_none());
    assert_eq!(input.profile_id.as_str(), "pwsh");
    let Command::Request(LocalApiRequest::AddTab { input, .. }) = parse_command_args(&[
        "tab",
        "create",
        ID,
        "--profile",
        "cmd",
        "--cwd",
        "C:\\work",
        "--title",
        "Two",
    ])
    .unwrap()
    .command
    else {
        panic!("Expected add tab");
    };
    assert_eq!(input.cwd.as_deref(), Some("C:\\work"));
    assert_eq!(input.title.as_deref(), Some("Two"));
}

#[test]
fn rename_and_close_map_to_typed_ids() {
    assert!(
        matches!(parse_command_args(&["session", "rename", ID, "New"]).unwrap().command,
        Command::Request(LocalApiRequest::RenameSession { id, title }) if id.to_string() == ID && title == "New")
    );
    assert!(
        matches!(parse_command_args(&["session", "close", ID]).unwrap().command,
        Command::Request(LocalApiRequest::DeleteSession { id }) if id.to_string() == ID)
    );
    assert!(
        matches!(parse_command_args(&["tab", "rename", ID, "New"]).unwrap().command,
        Command::Request(LocalApiRequest::RenameTab { id, title }) if id.to_string() == ID && title == "New")
    );
    assert!(
        matches!(parse_command_args(&["tab", "close", ID]).unwrap().command,
        Command::Request(LocalApiRequest::CloseTab { id }) if id.to_string() == ID)
    );
}

#[test]
fn settings_patch_is_typed_and_validated() {
    assert!(matches!(
        parse_command_args(&["settings", "get"]).unwrap().command,
        Command::Request(LocalApiRequest::Settings {
            request: SettingsRequest::Get {}
        })
    ));
    let Command::Request(LocalApiRequest::Settings {
        request: SettingsRequest::Save(patch),
    }) = parse_command_args(&[
        "settings",
        "set",
        r##"{"historyPageSize":16,"accentColor":"#123456","terminalProfileId":"pwsh"}"##,
    ])
    .unwrap()
    .command
    else {
        panic!("Expected settings patch");
    };
    assert_eq!(patch.history_page_size, Some(16));
    assert_eq!(patch.terminal_profile_id, Some(Some("pwsh".into())));
    for patch in [
        r#"{"unknown":true}"#,
        r#"{"historyPageSize":101}"#,
        r#"{"fontFamily":""}"#,
        r#"{"accentColor":"red"}"#,
        r#"{"terminalProfileId":"\u0000"}"#,
        "[]",
        "null",
        "{",
    ] {
        assert!(
            parse_command_args(&["settings", "set", patch]).is_err(),
            "Accepted {patch}"
        );
    }
}

#[test]
fn attach_requires_a_tab_and_takeover_is_opt_in() {
    assert!(
        matches!(parse_command_args(&["attach", ID]).unwrap().command,
        Command::Attach { tab, takeover: false } if tab.to_string() == ID)
    );
    assert!(matches!(
        parse_command_args(&["attach", ID, "--takeover"])
            .unwrap()
            .command,
        Command::Attach { takeover: true, .. }
    ));
}

#[test]
fn rejects_unknown_duplicate_missing_and_extra_arguments() {
    for words in [
        vec!["host"],
        vec!["host", "install"],
        vec!["host", "serve", "extra"],
        vec!["session", "list", "extra"],
        vec!["tab", "list", "not-an-id"],
        vec!["attach"],
        vec!["attach", ID, "--force"],
        vec!["attach", ID, "--takeover", "extra"],
        vec!["session", "rename", ID],
        vec!["session", "rename", "invalid", "Name"],
        vec!["session", "rename", ID, ""],
        vec!["tab", "rename", ID, "line\nbreak"],
        vec!["tab", "close", ID, "extra"],
        vec!["settings", "get", "extra"],
        vec!["session", "create", "--cwd", "/work"],
        vec!["session", "create", "--profile", "cmd"],
        vec!["session", "create", "--cwd", "relative", "--profile", "cmd"],
        vec![
            "session",
            "create",
            "--cwd",
            "/work",
            "--profile",
            "bad\nprofile",
        ],
        vec![
            "session",
            "create",
            "--cwd",
            "/work",
            "--profile",
            "cmd",
            "--unknown",
            "x",
        ],
        vec![
            "session",
            "create",
            "--cwd",
            "/work",
            "--profile",
            "cmd",
            "--title",
        ],
        vec![
            "session",
            "create",
            "--cwd",
            "/work",
            "--profile",
            "cmd",
            "--cwd",
            "/other",
        ],
        vec!["tab", "create", ID],
        vec!["tab", "create", ID, "--profile", "cmd", "--title", ""],
        vec!["session", "list", "--data-dir", "/work"],
    ] {
        assert!(parse_command_args(&words).is_err(), "Accepted {words:?}");
    }
}

#[cfg(unix)]
#[test]
fn native_paths_can_be_non_utf8_but_metadata_cannot() {
    use std::os::unix::ffi::OsStringExt;
    let data = OsString::from_vec(b"/tmp/cli-data-\xff".to_vec());
    let helper = OsString::from_vec(b"/tmp/cli-helper-\xff".to_vec());
    let parsed = parse([
        OsString::from("--data-dir"),
        data.clone(),
        OsString::from("--helper"),
        helper.clone(),
        OsString::from("--helper-sha256"),
        OsString::from(DIGEST),
        OsString::from("session"),
        OsString::from("list"),
    ])
    .unwrap();
    assert_eq!(parsed.data_dir.as_os_str(), data);
    assert_eq!(parsed.helper.as_os_str(), helper);
    assert_eq!(
        parsed.application.remote_state_dir,
        parsed.data_dir.join("rust-service-v1/go-network/node-v1")
    );
    let error = parse([
        OsString::from("--data-dir"),
        data,
        OsString::from("--helper"),
        helper,
        OsString::from("--helper-sha256"),
        OsString::from(DIGEST),
        OsString::from_vec(vec![0xff]),
    ])
    .unwrap_err();
    assert_eq!(error.to_string(), "Command arguments must be valid UTF-8.");
}
