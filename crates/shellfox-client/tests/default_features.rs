#![cfg(not(feature = "local-host"))]
use shellfox_client::protocol::{RemoteCommand, REMOTE_METHODS};

#[test]
fn default_remote_client_allowlist_still_excludes_every_local_automation_method() {
    assert_eq!(REMOTE_METHODS.len(), 43);
    for method in [
        "private.local.v1",
        "private.desktop.launch.v1",
        "automation.start",
        "automation.list",
        "automation.read",
        "automation.write",
        "automation.stop",
        "automation.show",
    ] {
        assert!(!REMOTE_METHODS.contains(&method));
        let json = serde_json::json!({"method":method,"payload":{}});
        assert!(serde_json::from_value::<RemoteCommand>(json).is_err());
    }
}
