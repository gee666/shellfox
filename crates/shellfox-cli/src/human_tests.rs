use super::*;
use serde_json::json;

fn plain(value: Value) -> String {
    render(&value, Style(false), usize::MAX)
}

#[test]
fn records_print_fields_then_nested_values_like_0_9() {
    assert_eq!(plain(json!({})), "Done.");
    assert_eq!(plain(json!({ "deleted": true })), "deleted  yes");
    assert_eq!(
        plain(json!({ "cwd": "/srv", "lastError": null, "env": [] })),
        "cwd         /srv\nlast Error  none\nenv\n  None."
    );
    assert_eq!(plain(json!("text\u{1b}")), r"text\u001b");
}

#[test]
fn sessions_terminals_and_machines_get_their_0_9_tables() {
    let sessions =
        json!([{ "title": "api", "id": "S", "status": "running", "tabs": [1, 2], "cwd": "/w" }]);
    assert_eq!(
        plain(sessions),
        "NAME  ID  STATUS   TERMINALS  PATH\napi   S   running  2          /w"
    );
    let tabs = json!([{ "title": "sh", "id": "T", "sessionId": "S", "lifecycle": "open", "profileId": "bash", "cwd": "/w" }]);
    assert!(plain(tabs).starts_with(
        "NAME  ID  SESSION  STATE  PROFILE  PATH\nsh    T   S        open   bash     /w"
    ));
    let machines = json!([{ "hostname": "box", "id": "n1", "online": true, "connection": "connected", "approved": false, "lastError": { "message": "x" } }]);
    assert_eq!(
        plain(machines),
        "MACHINE  ID  ONLINE  CONNECTION  APPROVED  ERROR\nbox      n1  yes     connected   no        x"
    );
}
