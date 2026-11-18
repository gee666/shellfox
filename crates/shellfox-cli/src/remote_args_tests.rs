use super::*;
#[test]
fn all_thirteen_closed_operators_have_explicit_grammar() {
    for (words, operation) in [
        (vec!["status"], "state"),
        (vec!["login"], "login"),
        (
            vec!["cancel", "00000000-0000-4000-8000-000000000001"],
            "cancel",
        ),
        (
            vec!["continue", "00000000-0000-4000-8000-000000000001"],
            "continue",
        ),
        (vec!["peer", "n-work", "connect"], "connect"),
        (vec!["peer", "n-work", "disconnect"], "disconnect"),
        (vec!["peer", "n-work", "reconnect"], "reconnect"),
        (vec!["peer", "n-work", "approve"], "approve"),
        (vec!["peer", "n-work", "reject"], "reject"),
        (vec!["peer", "n-work", "revoke"], "revoke"),
        (vec!["peer", "n-work", "forget"], "forget"),
    ] {
        let words: Vec<_> = ["host", "network"].into_iter().chain(words).collect();
        let NetworkCommand::Request(request) = parse(&words).unwrap().unwrap() else {
            panic!("Expected closed request");
        };
        assert_eq!(
            serde_json::to_value(request).unwrap()["operation"],
            operation
        );
    }
    for (flag, enabled) in [("on", true), ("off", false)] {
        assert!(
            matches!(parse(&["host", "network", flag]).unwrap().unwrap(), NetworkCommand::Configure { enabled:value, hostname:None } if value == enabled)
        );
    }
    assert!(matches!(
        parse(&["host", "network", "login", "--auth-key-stdin"]).unwrap(),
        Some(NetworkCommand::LoginFromStdin)
    ));
}
#[test]
fn invalid_or_secret_argv_is_refused_without_echo_or_connection() {
    let secret = "tskey-auth-PRIVATE";
    for suffix in [
        vec!["login", secret],
        vec!["login", "--auth-key", secret],
        vec!["login", "--auth-key-stdin", secret],
        vec!["pair", "n-work"],
        vec!["peer", "n-work", "takeover"],
        vec!["peer", "bad/peer", "approve"],
        vec!["cancel", "00000000-0000-0000-0000-000000000000"],
        vec!["continue", "not-a-uuid"],
        vec!["on", "--hostname", "bad/host"],
        vec!["status", "--control-url", "http://127.0.0.1:1"],
    ] {
        let words: Vec<_> = ["host", "network"].into_iter().chain(suffix).collect();
        let error = parse(&words).unwrap_err();
        assert!(!format!("{error:#}").contains(secret));
    }
    assert!(parse(&["host", "power", "on"]).unwrap().is_none());
}
