#![cfg(feature = "local-host")]
use serde_json::json;
use shellfox_protocol::{automation::*, LocalRequest, Validate};

fn input() -> serde_json::Value {
    json!({"requestId":"11111111-1111-4111-8111-111111111111","executable":"fixture.exe","args":["","literal & value","\n"],"cwd":"C:\\owned", "env":{"FIXTURE":"\nvalue"},"detached":false})
}
#[test]
fn exact_six_local_methods_and_unchanged_literal_start_fields() {
    assert_eq!(
        [
            AutomationStart::METHOD,
            AutomationList::METHOD,
            AutomationRead::METHOD,
            AutomationWrite::METHOD,
            AutomationStop::METHOD,
            AutomationShow::METHOD
        ],
        [
            "automation.start",
            "automation.list",
            "automation.read",
            "automation.write",
            "automation.stop",
            "automation.show"
        ]
    );
    let value = input();
    let request: AutomationStart = serde_json::from_value(value.clone()).unwrap();
    request.validate().unwrap();
    assert_eq!(serde_json::to_value(request).unwrap(), value);
}
#[test]
fn strict_wire_ids_unknown_fields_and_required_nullable_fields() {
    let mut value = input();
    value["extra"] = json!(true);
    assert!(serde_json::from_value::<AutomationStart>(value).is_err());
    let mut value = input();
    value["requestId"] = json!("11111111111141118111111111111111");
    assert!(serde_json::from_value::<AutomationStart>(value).is_err());
    let mut value = input();
    value["title"] = serde_json::Value::Null;
    assert!(serde_json::from_value::<AutomationStart>(value).is_err());
    assert!(serde_json::from_value::<AutomationRead>(json!({"id":"bad","after":0})).is_err());
    let run = json!({"id":"11111111-1111-4111-8111-111111111111","pid":1,"executable":"fixture.exe","cwd":"C:\\owned","title":"fixture","state":"running","exitCode":null,"signal":null,"sessionId":null,"tabId":null});
    let parsed: AutomationRun = serde_json::from_value(run.clone()).unwrap();
    parsed.validate().unwrap();
    assert_eq!(serde_json::to_value(parsed).unwrap(), run);
    let mut missing = run;
    missing.as_object_mut().unwrap().remove("exitCode");
    assert!(serde_json::from_value::<AutomationRun>(missing).is_err());
}
#[test]
fn nul_argument_oversized_text_and_duplicate_environment_are_rejected() {
    let mut value = input();
    value["args"] = json!(["bad\u{0}argument"]);
    assert!(serde_json::from_value::<AutomationStart>(value)
        .unwrap()
        .validate()
        .is_err());
    let mut value = input();
    value["executable"] = json!("x".repeat(32769));
    assert!(serde_json::from_value::<AutomationStart>(value)
        .unwrap()
        .validate()
        .is_err());
    let value = serde_json::to_string(&input()).unwrap().replace(
        "\"FIXTURE\":\"\\nvalue\"",
        "\"FIXTURE\":\"one\",\"FIXTURE\":\"two\"",
    );
    assert!(serde_json::from_str::<AutomationStart>(&value).is_err());
}
#[test]
fn bounded_canonical_bytes_sequences_and_actual_acceptance_shape() {
    let id = "11111111-1111-4111-8111-111111111111";
    let bytes = vec![255; 32768];
    let data = shellfox_protocol::encode_chunk(&bytes).unwrap();
    let request: AutomationWrite =
        serde_json::from_value(json!({"id":id,"data":data,"eof":true})).unwrap();
    request.validate().unwrap();
    assert_eq!(
        shellfox_protocol::decode_chunk(&request.data).unwrap(),
        bytes
    );
    let maximum = shellfox_protocol::encode_chunk(&vec![1; AUTOMATION_INPUT_BYTES]).unwrap();
    serde_json::from_value::<AutomationWrite>(json!({"id":id,"data":maximum,"eof":false}))
        .unwrap()
        .validate()
        .unwrap();
    for data in ["/x==".into(), "!!!!".into(), "A".repeat(65540)] {
        assert!(serde_json::from_value::<AutomationWrite>(
            json!({"id":id,"data":data,"eof":false})
        )
        .unwrap()
        .validate()
        .is_err());
    }
    assert!(AutomationAccepted { accepted: false }.validate().is_err());
    assert!(AutomationStopped { stopped: false }.validate().is_err());
    assert!(AutomationFrame {
        sequence: 0,
        stream: AutomationStream::Stderr,
        data: String::new()
    }
    .validate()
    .is_err());
    assert!(AutomationRead {
        id: id.parse().unwrap(),
        after: u64::MAX
    }
    .validate()
    .is_err());
}
