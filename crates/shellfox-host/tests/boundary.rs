use serde_json::{json, Value};
use shellfox_host::*;
use std::path::PathBuf;
struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tmp")
            .join(format!("host-boundary-{}", SessionId::new()));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn send(host: &Host, value: Value) -> Snapshot {
    host.request_json(&value.to_string()).into_result().unwrap()
}

#[test]
fn camel_case_method_contracts_execute_real_metadata_mutations() {
    let scratch = Scratch::new();
    let host = Host::open(&scratch.0).unwrap();
    let request_id = SessionId::new();
    let create = json!({"method":"createSession","payload":{"requestId":request_id,"title":"app","cwd":"/work","profileId":"login-shell"}});
    let snapshot = send(&host, create.clone());
    let session = snapshot.sessions[0].id;
    let first = snapshot.sessions[0].tabs[0].id;
    assert_eq!(send(&host, create).revision, 1);
    let snapshot = send(
        &host,
        json!({"method":"addTab","payload":{"sessionId":session,"profileId":"pwsh","cwd":"C:\\work","title":"shell"}}),
    );
    let second = snapshot.sessions[0].tabs[1].id;
    send(
        &host,
        json!({"method":"renameTab","payload":{"sessionId":session,"tabId":first,"title":"editor"}}),
    );
    send(
        &host,
        json!({"method":"renameSession","payload":{"sessionId":session,"title":"real session"}}),
    );
    send(
        &host,
        json!({"method":"reorderTabs","payload":{"sessionId":session,"tabIds":[second,first]}}),
    );
    send(
        &host,
        json!({"method":"setSessionPinned","payload":{"sessionId":session,"pinned":true}}),
    );
    send(
        &host,
        json!({"method":"activateSession","payload":{"sessionId":session}}),
    );
    let snapshot = send(&host, json!({"method":"getSnapshot","payload":{}}));
    assert_eq!(snapshot.sessions[0].title, "real session");
    assert_eq!(snapshot.sessions[0].tabs[0].id, second);
    assert_eq!(snapshot.sessions[0].tabs[1].title, "editor");
    assert!(snapshot.sessions[0].pinned_at.is_some());
    let encoded = serde_json::to_value(snapshot).unwrap();
    assert!(encoded["sessions"][0].get("createdAt").is_some());
    assert!(encoded["selection"].get("sessionId").is_some());
}

#[test]
fn save_settings_is_atomic_and_omitted_environment_preserves_latest() {
    let scratch = Scratch::new();
    let host = Host::open(&scratch.0).unwrap();
    let snapshot = send(
        &host,
        json!({"method":"createSession","payload":{"requestId":SessionId::new(),"cwd":"/work","profileId":"cmd"}}),
    );
    let tab = snapshot.sessions[0].tabs[0].id;
    let session = snapshot.sessions[0].id;
    send(
        &host,
        json!({"method":"setTerminalEnv","payload":{"env":[{"name":"APP","value":"global"}]}}),
    );
    send(
        &host,
        json!({"method":"saveSettings","payload":{"fontFamily":"Consolas","fontSize":18.0,"defaultProfileId":"cmd"}}),
    );
    assert_eq!(host.terminal_launch(tab).unwrap().env[0].value, "global");
    assert_eq!(
        host.preferences().unwrap().default_profile_id.as_str(),
        "cmd"
    );
    send(
        &host,
        json!({"method":"setSessionEnv","payload":{"sessionId":session,"env":[{"name":"APP","value":"session"}]}}),
    );
    assert_eq!(host.terminal_launch(tab).unwrap().env[0].value, "session");
    let before = host.snapshot().unwrap();
    let failure = host
        .request_json(
            &json!({"method":"saveSettings","payload":{"fontSize":1.0,"terminalEnv":[]}})
                .to_string(),
        )
        .into_result()
        .unwrap_err();
    assert_eq!(failure.code, ErrorCode::Validation);
    assert_eq!(host.snapshot().unwrap(), before);
    assert_eq!(host.terminal_launch(tab).unwrap().env[0].value, "session");
}

#[test]
fn unknown_fields_capabilities_and_generation_cleanup_are_not_remote_calls() {
    let scratch = Scratch::new();
    let host = Host::open(&scratch.0).unwrap();
    for method in [
        "remote.connect",
        "listFiles",
        "commitClose",
        "registerTerminal",
        "writeTerminal",
    ] {
        let error = host
            .request_json(&json!({"method":method,"payload":{}}).to_string())
            .into_result()
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::Unsupported);
    }
    for payload in [
        json!({"requestId":SessionId::new(),"cwd":"/work","profileId":"cmd","command":"credential"}),
        json!({"requestId":SessionId::new(),"cwd":"/work","profileId":"cmd","output":"secret"}),
        json!({"requestId":SessionId::new(),"cwd":"/work","profileId":"cmd","environment":{"TOKEN":"secret"}}),
        json!({"requestId":"not-a-uuid","cwd":"/work","profileId":"cmd"}),
    ] {
        assert_eq!(
            host.request_json(&json!({"method":"createSession","payload":payload}).to_string())
                .into_result()
                .unwrap_err()
                .code,
            ErrorCode::Validation
        );
    }
    let serialized =
        serde_json::to_value(host.request_json(r#"{"method":"getSnapshot","payload":{}}"#))
            .unwrap();
    assert_eq!(serialized["ok"], true);
    assert!(host.snapshot().unwrap().sessions.is_empty());
}
