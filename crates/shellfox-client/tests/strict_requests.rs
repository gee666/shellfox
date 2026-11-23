use serde_json::json;
use shellfox_client::protocol::*;
const ID: &str = "10000000-0000-4000-8000-000000000001";

#[test]
fn remote_dispatch_is_typed_and_rejects_privileged_or_unknown_methods() {
    for method in [
        "saveSettings",
        "stopHost",
        "openExternal",
        "getKeepAwake",
        "private.local.v1",
        "arbitrary",
    ] {
        let json = json!({"method":method,"payload":{}});
        assert!(
            serde_json::from_value::<RemoteCommand>(json).is_err(),
            "{method}"
        );
    }
    let command: RemoteCommand = serde_json::from_value(
        json!({"method":"renameSession","payload":{"sessionId":ID,"title":"hello"}}),
    )
    .unwrap();
    command.validate().unwrap();
}
#[test]
fn core_uuid_shorthand_or_invalid_version_is_not_a_wire_uuid() {
    for id in [
        "10000000000040008000000000000001",
        "urn:uuid:10000000-0000-4000-8000-000000000001",
        "10000000-0000-0000-0000-000000000001",
    ] {
        assert!(
            serde_json::from_value::<RenameSession>(json!({"sessionId":id,"title":"ok"})).is_err()
        );
        assert!(
            serde_json::from_value::<AttachTerminal>(json!({"tabId":ID,"generation":id})).is_err()
        );
    }
    let constructed = RenameSession {
        session_id: "10000000-0000-0000-0000-000000000001".parse().unwrap(),
        title: "ok".into(),
    };
    assert!(constructed.validate_wire().is_err());
}
#[test]
fn optional_nonnullable_request_fields_reject_null_but_settlement_preserves_null() {
    assert!(
        serde_json::from_value::<AttachTerminal>(json!({"tabId":ID,"generation":null})).is_err()
    );
    assert!(serde_json::from_value::<CreateSession>(
        json!({"requestId":ID,"cwd":"/tmp","title":null})
    )
    .is_err());
    let input: WriteTerminal = serde_json::from_value(
        json!({"tabId":ID,"generation":ID,"controlEpoch":1,"data":"x","settledAt":null}),
    )
    .unwrap();
    assert_eq!(input.settled_at, Some(None));
    input.validate_wire().unwrap();
    assert!(serde_json::from_value::<WriteTerminal>(
        json!({"tabId":ID,"generation":ID,"controlEpoch":1,"data":"x","userInitiated":null})
    )
    .is_err());
}
#[test]
fn payload_bounds_duplicate_ids_paths_and_remote_environment_remain_validated() {
    let base = json!({"tabId":ID,"generation":ID,"controlEpoch":1,"data":"x"});
    for data in ["".to_owned(), "💙".repeat(16385)] {
        let mut input = base.clone();
        input["data"] = data.into();
        assert!(serde_json::from_value::<WriteTerminal>(input)
            .unwrap()
            .validate()
            .is_err());
    }
    let tabs: ReorderTabs =
        serde_json::from_value(json!({"sessionId":ID,"tabIds":[ID,ID]})).unwrap();
    assert!(tabs.validate().is_err());
    let env: SetTerminalEnv =
        serde_json::from_value(json!({"env":[{"name":"A","value":"x"},{"name":"a","value":"y"}]}))
            .unwrap();
    env.validate().unwrap(); // Host applies its own OS-specific case policy.
    let env: SetTerminalEnv =
        serde_json::from_value(json!({"env":[{"name":"A","value":"x\ny"}]})).unwrap();
    assert!(env.validate().is_err());
    let input: ListFiles = serde_json::from_value(json!({"tabId":ID,"path":"notes.txt"})).unwrap();
    assert!(input.validate().is_err());
}
#[test]
fn file_response_validates_required_null_and_canonical_base64() {
    let valid = json!({"path":"/tmp","parentPath":null,"entries":[],"truncated":false});
    let list: files::FileList = serde_json::from_value(valid.clone()).unwrap();
    list.validate().unwrap();
    let mut invalid = valid;
    invalid.as_object_mut().unwrap().remove("parentPath");
    assert!(serde_json::from_value::<files::FileList>(invalid).is_err());
    let image = json!({"kind":"image","path":"image.png","mimeType":"image/png","size":1,"revision":"a".repeat(64),"dataUrl":"data:image/png;base64,AQ=="});
    let read: files::FileRead = serde_json::from_value(image.clone()).unwrap();
    read.validate().unwrap();
    let mut bad = image;
    bad["dataUrl"] = "data:image/png;base64,AB==".into();
    assert!(serde_json::from_value::<files::FileRead>(bad)
        .unwrap()
        .validate()
        .is_err());
}
