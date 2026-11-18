use super::*;
const TRANSFER: &str = "50000000-0000-4000-8000-000000000001";
const GENERATION: &str = "30000000-0000-4000-8000-000000000001";

#[test]
fn shared_command_dispatch_covers_remaining_sixteen_typed_remote_methods() {
    let mut h = Harness::new();
    h.connect();
    let _view = NativeView::attach(&mut h);
    let revision = "0".repeat(64);
    let transfer = json!({"transferId":TRANSFER});
    let upload = json!({"tabId":TAB,"generation":GENERATION,"controlEpoch":1,"uploadId":TRANSFER});
    let started = json!({"transferId":TRANSFER,"chunkBytes":196608});
    let file = json!({"path":"b.txt","size":3,"revision":revision});
    let cases = [
        (
            "resolveDirectory",
            json!({"path":"~"}),
            json!({"cwd":"/fake-home"}),
        ),
        (
            "createDirectory",
            json!({"path":"uploads"}),
            json!({"cwd":"/fake-home/uploads"}),
        ),
        (
            "completeDirectory",
            json!({"path":"/fake"}),
            json!({"matches":["/fake-home"]}),
        ),
        (
            "getSessionStats",
            json!({}),
            json!({"sessions":[],"tabs":[]}),
        ),
        (
            "beginFileRead",
            json!({"path":"a.txt"}),
            json!({"transferId":TRANSFER,"path":"a.txt","size":3,
            "revision":revision,"kind":"text","mimeType":"text/plain","chunkBytes":196608}),
        ),
        (
            "readFileChunk",
            json!({"transferId":TRANSFER,"offset":0}),
            json!({"offset":3,"data":"YWJj","eof":true}),
        ),
        (
            "beginFileWrite",
            json!({"path":"b.txt","size":3,"mode":"create","content":"file"}),
            started,
        ),
        (
            "writeFileChunk",
            json!({"transferId":TRANSFER,"offset":0,"data":"YWJj"}),
            json!({"offset":3}),
        ),
        ("finishFileWrite", transfer.clone(), file.clone()),
        ("cancelFileTransfer", transfer, json!({"cancelled":true})),
        (
            "writeImageFile",
            json!({"path":"a.png","revision":revision,"dataUrl":"data:image/png;base64,AA=="}),
            json!({"path":"a.png","revision":revision,"size":1}),
        ),
        (
            "readFileAsset",
            json!({"path":"a.html","assetPath":"a.css"}),
            json!({"mimeType":"text/css","dataUrl":"data:text/css;base64,YWJj","size":3}),
        ),
        (
            "beginTerminalUpload",
            json!({"tabId":TAB,"generation":GENERATION,"controlEpoch":1,"fileName":"a.txt","size":3}),
            json!({"uploadId":TRANSFER,"chunkBytes":196608}),
        ),
        (
            "writeTerminalUpload",
            json!({"tabId":TAB,"generation":GENERATION,"controlEpoch":1,
            "uploadId":TRANSFER,"offset":0,"data":"YWJj"}),
            json!({"offset":3}),
        ),
        (
            "finishTerminalUpload",
            upload.clone(),
            json!({"path":"/fake-home/a.txt","fileName":"a.txt","size":3}),
        ),
        ("cancelTerminalUpload", upload, json!({"cancelled":true})),
    ];
    assert_eq!(cases.len(), 16);
    for (method, payload, value) in cases {
        if method == "cancelTerminalUpload" {
            assert_eq!(
                h.operation(json!({"method":"beginTerminalUpload","payload":{
                "tabId":TAB,"generation":GENERATION,"controlEpoch":1,"fileName":"a.txt","size":3}}))
                    .0,
                0
            );
            let begin = h.request();
            let response = serde_json::to_vec(
                &json!({"ok":true,"value":{"uploadId":TRANSFER,"chunkBytes":196608}}),
            )
            .unwrap();
            assert_eq!(h.complete(&begin, &response), 0);
            assert_eq!(h.event()["status"], 0);
        }
        let (status, operation) = h.operation(json!({"method":method,"payload":payload}));
        assert_eq!(status, 0, "{method}");
        let request = h.request();
        assert_eq!(request.path, "/v1/request");
        assert_eq!(request.body["method"], method);
        assert_eq!(request.body["payload"], payload);
        let response = serde_json::to_vec(&json!({"ok":true,"value":value})).unwrap();
        assert_eq!(h.complete(&request, &response), 0, "{method}");
        let event = h.event();
        assert_eq!(event["status"], 0, "{method}");
        assert_eq!(event["internal"], false, "{method}");
        assert_eq!(event["operationId"], operation.to_string());
        assert_eq!(event["result"], json!({"ok":true,"value":value}));
    }
}
#[test]
fn new_file_request_schema_rejects_bad_chunks_and_unknown_fields_before_go() {
    let mut h = Harness::new();
    h.connect();
    for command in [
        json!({"method":"beginFileRead","payload":{"path":"a.txt","authKey":"not-allowed"}}),
        json!({"method":"writeFileChunk","payload":{"transferId":TRANSFER,"offset":0,"data":"YR=="}}),
        json!({"method":"readFileAsset","payload":{"path":"a.html","assetPath":"https://outside.invalid/a.css"}}),
    ] {
        assert_eq!(h.operation(command), (INVALID, 0));
    }
    let oversized = "A".repeat(262148);
    assert_eq!(
        h.operation(json!({"method":"writeFileChunk","payload":{
        "transferId":TRANSFER,"offset":0,"data":oversized}})),
        (INVALID, 0)
    );
    h.dispatch();
    assert!(h.mock.requests.is_empty());
}
#[test]
fn invalid_file_chunk_response_does_not_cross_to_file_presentation_as_success() {
    let mut h = Harness::new();
    h.connect();
    assert_eq!(
        h.operation(json!({"method":"readFileChunk","payload":{"transferId":TRANSFER,"offset":0}}))
            .0,
        0
    );
    let request = h.request();
    let invalid = br#"{"ok":true,"value":{"offset":1,"data":"YR==","eof":true}}"#;
    assert_eq!(h.complete(&request, invalid), 0);
    let event = h.event();
    assert_eq!(event["status"], PROTOCOL);
    assert_eq!(event["internal"], false);
    assert!(event["result"].is_null());
}
#[test]
fn legacy_terminal_upload_rejection_stays_shared_unsupported_not_auth_failure() {
    let mut h = Harness::new();
    h.connect();
    let _view = NativeView::attach(&mut h);
    assert_eq!(
        h.operation(json!({"method":"beginTerminalUpload","payload":{
        "tabId":TAB,"generation":GENERATION,"controlEpoch":1,"fileName":"a.txt","size":3}}))
            .0,
        0
    );
    let request = h.request();
    let response = br#"{"ok":false,"error":{"code":"AUTH_FAILED","message":"This operation is not available remotely.","retryable":false}}"#;
    assert_eq!(h.complete(&request, response), 0);
    let event = h.event();
    assert_eq!(event["result"]["error"]["code"], "UNSUPPORTED");
    assert_eq!(event["connection"]["status"], "connected");
}
