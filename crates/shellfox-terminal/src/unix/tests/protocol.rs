use crate::unix::{Capability, ControlCommand, SupervisorStatus};
use serde_json::{json, Value};
fn status() -> Value {
    json!({"version":1,"ok":true,"state":"open","supervisorPid":123,"shellPid":456,
        "shellBirth":"darwin:1711111111:000123","shellAlive":true,"exitCode":null,"reason":null})
}
fn frame(value: &Value) -> Vec<u8> {
    format!("{value}\n").into_bytes()
}
#[test]
fn validates_capability_against_configured_platform_arch() {
    let v = json!({"version":1,"platform":"darwin","arch":"arm64","available":true,"reason":null,
        "ownedSession":true,"guardians":true,"termination":true});
    assert!(Capability::parse(v.to_string().as_bytes(), "arm64").is_ok());
    assert!(Capability::parse(v.to_string().as_bytes(), "x64").is_err());
    let mut bad = v;
    bad["guardians"] = json!(false);
    assert!(Capability::parse(bad.to_string().as_bytes(), "arm64").is_err());
}
#[test]
fn token_request_and_identity_are_pinned() {
    assert_eq!(
        ControlCommand::Status.frame(&"a".repeat(64)).unwrap(),
        format!("1\t{}\tSTATUS\n", "a".repeat(64))
    );
    assert!(ControlCommand::Close.frame("not-a-token").is_err());
    let value = status();
    let ready = SupervisorStatus::parse(&frame(&value), 123, None).unwrap();
    let identity = ready.ready_identity().unwrap();
    let mut changed = value;
    for field in ["shellPid", "supervisorPid", "shellBirth"] {
        changed[field] = match field {
            "shellBirth" => json!("darwin:1711111111:000124"),
            _ => json!(789),
        };
        assert!(SupervisorStatus::parse(&frame(&changed), 123, Some(&identity)).is_err());
        changed = status();
    }
}
#[test]
fn refuses_malformed_frames_and_unproven_cleanup() {
    let mut value = status();
    let identity = SupervisorStatus::parse(&frame(&value), 123, None)
        .unwrap()
        .ready_identity()
        .unwrap();
    value["state"] = json!("closed");
    assert!(SupervisorStatus::parse(&frame(&value), 123, Some(&identity)).is_err());
    value["shellAlive"] = json!(false);
    value["ok"] = json!(false);
    assert!(
        SupervisorStatus::parse(&frame(&value), 123, Some(&identity))
            .unwrap()
            .confirm_closed()
            .is_err()
    );
    value["ok"] = json!(true);
    value["exitCode"] = json!(137);
    SupervisorStatus::parse(&frame(&value), 123, Some(&identity))
        .unwrap()
        .confirm_closed()
        .unwrap();
    for invalid in [
        b"\xff\n".to_vec(),
        frame(&value).repeat(2),
        vec![b'x'; 2049],
        value.to_string().into_bytes(),
    ] {
        assert!(SupervisorStatus::parse(&invalid, 123, Some(&identity)).is_err());
    }
    for key in ["exitCode", "reason"] {
        let mut missing = value.clone();
        missing.as_object_mut().unwrap().remove(key);
        assert!(SupervisorStatus::parse(&frame(&missing), 123, None).is_err());
    }
    value["reason"] = json!("invalid\nreason");
    assert!(SupervisorStatus::parse(&frame(&value), 123, None).is_err());
    value["reason"] = json!(null);
    value["extra"] = json!(true);
    assert!(SupervisorStatus::parse(&frame(&value), 123, None).is_err());
}
