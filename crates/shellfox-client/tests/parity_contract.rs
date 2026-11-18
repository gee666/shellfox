use futures::executor::block_on;
use serde_json::{json, Value};
use shellfox_client::{protocol::*, *};
use std::sync::{Arc, Mutex};

fn cases() -> Vec<Value> {
    serde_json::from_str(include_str!("fixtures/remote-parity.json")).unwrap()
}
fn contract<R: Request>(fixture: &Value) {
    assert_eq!(R::METHOD, fixture["method"].as_str().unwrap());
    let request: R = serde_json::from_value(fixture["payload"].clone()).unwrap();
    request.validate_wire().unwrap();
    assert_eq!(serde_json::to_value(&request).unwrap(), fixture["payload"]);
    let bytes = serde_json::to_vec(&fixture["response"]).unwrap();
    let response: Reply<R::Response> = decode(&bytes).unwrap();
    assert_eq!(serde_json::to_value(response).unwrap(), fixture["response"]);
    let encoded = serde_json::to_value(RequestEnvelope::new(ClientId::nil(), &request)).unwrap();
    assert_eq!(encoded["method"], fixture["method"]);
}
#[test]
fn all_remote_command_names_match_the_complete_active_ts_allowlist() {
    let expected: Vec<String> =
        serde_json::from_str(include_str!("fixtures/remote-methods.json")).unwrap();
    assert_eq!(REMOTE_METHODS, expected);
    assert_eq!(REMOTE_METHODS.len(), 43);
}
#[test]
fn remaining_types_roundtrip_active_zod_validated_fixture_shapes() {
    for fixture in cases() {
        match fixture["method"].as_str().unwrap() {
            "beginFileRead" => contract::<BeginFileRead>(&fixture),
            "readFileChunk" => contract::<ReadFileChunk>(&fixture),
            "beginFileWrite" => contract::<BeginFileWrite>(&fixture),
            "writeFileChunk" => contract::<WriteFileChunk>(&fixture),
            "finishFileWrite" => contract::<FinishFileWrite>(&fixture),
            "cancelFileTransfer" => contract::<CancelFileTransfer>(&fixture),
            "writeImageFile" => contract::<WriteImageFile>(&fixture),
            "readFileAsset" => contract::<ReadFileAsset>(&fixture),
            "getSessionStats" => contract::<GetSessionStats>(&fixture),
            "resolveDirectory" => contract::<ResolveDirectory>(&fixture),
            "createDirectory" => contract::<CreateDirectory>(&fixture),
            "completeDirectory" => contract::<CompleteDirectory>(&fixture),
            "beginTerminalUpload" => contract::<BeginTerminalUpload>(&fixture),
            "writeTerminalUpload" => contract::<WriteTerminalUpload>(&fixture),
            "finishTerminalUpload" => contract::<FinishTerminalUpload>(&fixture),
            "cancelTerminalUpload" => contract::<CancelTerminalUpload>(&fixture),
            _ => panic!("Fixture method not covered"),
        }
    }
}
#[test]
fn canonical_chunks_reject_padding_garbage_and_encoded_limit_overflow() {
    for bad in ["A", "A===", "AB==", "AAF=", "YWJj\n", "====", "💙", "AA=A"] {
        assert!(chunk_size(bad, false).is_err(), "{bad}");
    }
    assert!(chunk_size("", false).is_err());
    assert_eq!(chunk_size("", true).unwrap(), 0);
    for size in [0, 1, 2, 3, 255, FILE_CHUNK_BYTES - 1, FILE_CHUNK_BYTES] {
        let bytes: Vec<u8> = (0..size).map(|n| (n % 256) as u8).collect();
        let data = encode_chunk(&bytes).unwrap();
        assert_eq!(decode_chunk(&data).unwrap(), bytes);
    }
    assert!(encode_chunk(&vec![0; FILE_CHUNK_BYTES + 1]).is_err());
    assert!(chunk_size(&"A".repeat(FILE_CHUNK_BYTES / 3 * 4 + 4), false).is_err());
}
#[test]
fn transfer_conditional_fields_and_bounds_remain_strict() {
    let original = cases()
        .into_iter()
        .find(|v| v["method"] == "beginFileWrite")
        .unwrap()["payload"]
        .clone();
    for (key, value) in [
        ("revision", Value::Null),
        ("mode", json!("create")),
        ("content", json!("image")),
        ("size", json!(1_073_741_825_u64)),
        ("mimeType", json!("image/png")),
    ] {
        let mut payload = original.clone();
        payload[key] = value;
        let parsed = serde_json::from_value::<BeginFileWrite>(payload);
        assert!(
            parsed.is_err() || parsed.unwrap().validate().is_err(),
            "{key}"
        );
    }
    let mut payload = original;
    payload["size"] = json!(0);
    serde_json::from_value::<BeginFileWrite>(payload)
        .unwrap()
        .validate_wire()
        .unwrap();
    assert!(serde_json::from_value::<GetSessionStats>(json!({"sessionId":null})).is_err());
    assert!(DirectoryMatches {
        matches: vec!["a".repeat(1000); 66]
    }
    .validate()
    .is_err());
    ResolveDirectory { path: "".into() }.validate().unwrap();
}
#[test]
fn statistics_require_redacted_environments_safe_measurements_and_required_nulls() {
    let source = cases()
        .into_iter()
        .find(|v| v["method"] == "getSessionStats")
        .unwrap()["response"]["value"]
        .clone();
    for path in ["memory", "elapsed", "env"] {
        let mut invalid = source.clone();
        match path {
            "memory" => invalid["tabs"][0]["memoryBytes"] = json!(9_007_199_254_740_992_u64),
            "elapsed" => invalid["tabs"][0]["elapsedSeconds"] = json!(-1),
            _ => invalid["sessions"][0]["env"] = json!([{"name":"A","value":"x"}]),
        }
        assert!(serde_json::from_value::<SessionStats>(invalid)
            .unwrap()
            .validate()
            .is_err());
    }
    let mut invalid = source;
    invalid["tabs"][0]
        .as_object_mut()
        .unwrap()
        .remove("elapsedSeconds");
    assert!(serde_json::from_value::<SessionStats>(invalid).is_err());
}

#[derive(Clone)]
struct UploadHost {
    response: Value,
    calls: Arc<Mutex<Vec<Value>>>,
}
impl OutboundTransport for UploadHost {
    fn send<'a>(
        &'a self,
        _peer: &'a AuthenticatedPeerId,
        endpoint: Endpoint,
        body: Vec<u8>,
        _limits: RequestLimits,
    ) -> TransportFuture<'a> {
        Box::pin(async move {
            let value: Value = serde_json::from_slice(&body).unwrap();
            self.calls.lock().unwrap().push(value);
            Ok(serde_json::to_vec(&if endpoint == Endpoint::Pair {
                json!({"ok":true,"value":{"paired":true,"terminalControl":"exclusive-v1"}})
            } else {
                self.response.clone()
            })
            .unwrap())
        })
    }
}
#[test]
fn execute_command_routes_all_sixteen_additions_through_typed_validated_responses() {
    for fixture in cases() {
        let command: RemoteCommand = serde_json::from_value(
            json!({"method":fixture["method"],"payload":fixture["payload"]}),
        )
        .unwrap();
        assert_eq!(command.method_name(), fixture["method"].as_str().unwrap());
        command.validate().unwrap();
        let calls = Arc::new(Mutex::new(Vec::new()));
        let host = UploadHost {
            response: fixture["response"].clone(),
            calls: calls.clone(),
        };
        let client = Client::new(
            AuthenticatedPeerId::new("host").unwrap(),
            ClientId::new_v4(),
            host,
        );
        block_on(client.pair()).unwrap();
        let response = block_on(client.execute_command(&command)).unwrap();
        assert_eq!(serde_json::to_value(response).unwrap(), fixture["response"]);
        let sent = calls.lock().unwrap();
        assert_eq!(sent.len(), 2);
        assert_eq!(sent[1]["method"], fixture["method"]);
        assert_eq!(sent[1]["payload"], fixture["payload"]);
    }
}
#[test]
fn execute_command_rejects_malformed_typed_response_before_presentation_value() {
    let fixture = cases()
        .into_iter()
        .find(|f| f["method"] == "readFileChunk")
        .unwrap();
    let command: RemoteCommand =
        serde_json::from_value(json!({"method":fixture["method"],"payload":fixture["payload"]}))
            .unwrap();
    let host = UploadHost {
        response: json!({"ok":true,"value":{"offset":1,"data":"AB==","eof":true}}),
        calls: Arc::new(Mutex::new(Vec::new())),
    };
    let client = Client::new(
        AuthenticatedPeerId::new("host").unwrap(),
        ClientId::new_v4(),
        host,
    );
    block_on(client.pair()).unwrap();
    assert!(matches!(
        block_on(client.execute_command(&command)),
        Err(ClientError::Validation(_))
    ));
}
#[test]
fn old_host_upload_rejection_is_unsupported_without_reconnect_or_retry() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let host = UploadHost {
        response: json!({"ok":false,"error":{"code":"AUTH_FAILED","message":"This operation is not available remotely.","retryable":false}}),
        calls: calls.clone(),
    };
    let client = Client::new(
        AuthenticatedPeerId::new("host").unwrap(),
        ClientId::new_v4(),
        host,
    );
    block_on(client.pair()).unwrap();
    let input: BeginTerminalUpload = serde_json::from_value(
        cases()
            .into_iter()
            .find(|v| v["method"] == "beginTerminalUpload")
            .unwrap()["payload"]
            .clone(),
    )
    .unwrap();
    let Reply::Failure(error) = block_on(client.execute(&input)).unwrap() else {
        panic!()
    };
    assert_eq!(error.code, ErrorCode::Unsupported);
    assert_eq!(client.state().status, ConnectionStatus::Connected);
    assert_eq!(calls.lock().unwrap().len(), 2);
}
