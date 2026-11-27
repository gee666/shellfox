use super::*;

const S: &str = "12345678-1234-4234-8234-123456789abc";
const T: &str = "22345678-1234-4234-8234-123456789abc";

fn parsed(words: &[&str]) -> ManagerCommand {
    parse(words).unwrap().unwrap()
}
fn id() -> SessionId {
    S.parse().unwrap()
}

#[test]
fn session_operations_take_an_id_or_the_current_terminal() {
    assert_eq!(
        parsed(&["ss", "pin", S]),
        ManagerCommand::Session(SessionOp::Pin(true), Target::Id(id()))
    );
    assert_eq!(
        parsed(&["session", "pin", "--off"]),
        ManagerCommand::Session(SessionOp::Pin(false), Target::Current)
    );
    assert_eq!(
        parsed(&["sessions", "settle", "--id", S, "--confirm-active"]),
        ManagerCommand::Session(
            SessionOp::Settle {
                confirm_active: true
            },
            Target::Id(id())
        )
    );
    assert_eq!(
        parsed(&["session", "rm", S]),
        ManagerCommand::Session(SessionOp::Delete, Target::Id(id()))
    );
    assert_eq!(
        parsed(&["session", "rename", "New"]),
        ManagerCommand::Session(SessionOp::Rename("New".into()), Target::Current)
    );
    assert_eq!(
        parsed(&["session", "env", S, "--input", "[]"]),
        ManagerCommand::Session(
            SessionOp::Env(Some(serde_json::json!([]))),
            Target::Id(id())
        )
    );
    for op in ["focus", "refresh", "clear-error", "unsettle", "inspect"] {
        assert!(matches!(
            parsed(&["session", op, S]),
            ManagerCommand::Session(_, Target::Id(_))
        ));
    }
    for bad in [
        &["session", "rename", S][..],
        &["session", "pin", S, "--id", S],
        &["session", "pin", "1234"],
        &["session", "settle", "--off"],
        &["session", "nope"],
    ] {
        assert!(parse(bad).is_err(), "{bad:?}");
    }
    assert!(parse(&["session", "create"]).unwrap().is_none());
}

#[test]
fn listing_filters_follow_control_cli() {
    let ManagerCommand::Sessions(Listing::Stats, filters) = parsed(&["session", "stats"]) else {
        panic!()
    };
    assert!(filters.current);
    let ManagerCommand::Sessions(Listing::List, filters) =
        parsed(&["session", "list", "--running", "--name=api"])
    else {
        panic!()
    };
    assert!(filters.running && !filters.current && filters.name.as_deref() == Some("api"));
    let ManagerCommand::Sessions(Listing::Processes, filters) =
        parsed(&["session", "processes", S, "--pid", "42"])
    else {
        panic!()
    };
    assert_eq!(
        (filters.id, filters.pid, filters.current),
        (Some(id()), Some(42), false)
    );
    for bad in [
        &["session", "list", "--running", "--idle"][..],
        &["session", "stats", "--all", S],
        &["session", "list", "--pid", "4"],
        &["session", "processes", "--pid", "0"],
        &["session", "list", "--tab", "x"],
    ] {
        assert!(parse(bad).is_err(), "{bad:?}");
    }
}

#[test]
fn terminal_operations_keep_their_0_9_arity() {
    let tab: TabId = T.parse().unwrap();
    assert_eq!(
        parsed(&["tm", "list"]),
        ManagerCommand::Terminals(TerminalOp::List(None))
    );
    assert_eq!(
        parsed(&["tm", "list", "--current"]),
        ManagerCommand::Terminals(TerminalOp::List(Some(Target::Current)))
    );
    assert_eq!(
        parsed(&["terminals", "write", T, "ls\n", "--take"]),
        ManagerCommand::Terminals(TerminalOp::Write {
            tab,
            data: Some("ls\n".into()),
            take: true
        })
    );
    assert_eq!(
        parsed(&["terminals", "write", T, "--stdin"]),
        ManagerCommand::Terminals(TerminalOp::Write {
            tab,
            data: None,
            take: false
        })
    );
    assert_eq!(
        parsed(&["terminals", "reorder", S, T]),
        ManagerCommand::Terminals(TerminalOp::Reorder {
            session: Target::Id(id()),
            tabs: vec![tab]
        })
    );
    assert_eq!(
        parsed(&["terminals", "reorder", T]),
        ManagerCommand::Terminals(TerminalOp::Reorder {
            session: Target::Current,
            tabs: vec![tab]
        })
    );
    assert_eq!(
        parsed(&["terminals", "rename", T, "x"]),
        ManagerCommand::Terminals(TerminalOp::Rename {
            session: Target::Current,
            tab,
            title: "x".into()
        })
    );
    for bad in [
        &["terminals", "write", T][..],
        &["terminals", "write", "short", "x"],
        &["terminals", "nope"],
    ] {
        assert!(parse(bad).is_err(), "{bad:?}");
    }
    assert!(parse(&["terminals", "attach", T]).unwrap().is_none());
}

#[test]
fn api_path_and_profiles() {
    assert_eq!(parsed(&["profiles"]), ManagerCommand::Profiles);
    assert_eq!(parsed(&["profiles", "list"]), ManagerCommand::Profiles);
    assert_eq!(
        parsed(&["path", "home"]),
        ManagerCommand::Path(PathOp::Home)
    );
    assert_eq!(
        parsed(&["path", "complete", "~/s"]),
        ManagerCommand::Path(PathOp::Complete("~/s".into()))
    );
    assert_eq!(
        parsed(&["api", "getSnapshot"]),
        ManagerCommand::Api {
            method: "getSnapshot".into(),
            payload: serde_json::json!({})
        }
    );
    assert_eq!(
        parsed(&["remote", "api", "getState", "--input", "{}"]),
        ManagerCommand::RemoteApi {
            method: "getState".into(),
            payload: serde_json::json!({})
        }
    );
    assert!(parse(&["api", "x", "--input", "{"]).is_err());
    assert!(parse(&["path", "home", "extra"]).is_err());
    assert!(parse(&["profiles", "show"]).is_err());
}

#[test]
fn api_methods_and_payloads_are_checked_before_connecting() {
    let code = |words: &[&str]| {
        let error = parse(words).unwrap_err();
        error.downcast_ref::<CliError>().unwrap().code.to_string()
    };
    assert_eq!(code(&["api", "nope"]), "VALIDATION");
    assert_eq!(
        code(&["api", "focusSession", "--input", "{}"]),
        "VALIDATION"
    );
    assert_eq!(code(&["api", "installUpdate"]), "UNSUPPORTED");
    let read = format!(r#"{{"tabId":"{T}"}}"#);
    assert!(parse(&["api", "readTerminal", "--input", &read]).is_ok());
    let request = |method: &str| {
        format!(r#"{{"peerId":"p","method":"{method}","payload":{{"sessionId":"{S}"}}}}"#)
    };
    assert!(parse(&[
        "remote",
        "api",
        "request",
        "--input",
        &request("focusSession")
    ])
    .is_err());
    assert!(parse(&["remote", "api", "request", "--input", &request("nope")]).is_err());
    assert!(parse(&[
        "remote",
        "api",
        "request",
        "--input",
        &request("deleteSession")
    ])
    .is_ok());
}
