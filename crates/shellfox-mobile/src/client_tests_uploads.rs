use super::*;
const UPLOAD: &str = "50000000-0000-4000-8000-000000000001";
const GEN: &str = "30000000-0000-4000-8000-000000000001";
fn begin(h: &mut Harness) {
    assert_eq!(
        h.operation(json!({"method":"beginTerminalUpload","payload":{
        "tabId":TAB,"generation":GEN,"controlEpoch":1,"fileName":"a.txt","size":3}}))
            .0,
        0
    );
    let request = h.request();
    assert_eq!(h.complete(&request, br#"{"ok":true,"value":{"uploadId":"50000000-0000-4000-8000-000000000001","chunkBytes":196608}}"#), 0);
    assert_eq!(h.event()["status"], 0);
}
fn upload(h: &Harness, method: &str, epoch: u64) -> (i32, u64) {
    h.operation(json!({"method":method,"payload":{
        "tabId":TAB,"generation":GEN,"controlEpoch":epoch,"uploadId":UPLOAD}}))
}
fn chunk(h: &Harness, epoch: u64, offset: u64) -> (i32, u64) {
    h.operation(json!({"method":"writeTerminalUpload","payload":{
        "tabId":TAB,"generation":GEN,"controlEpoch":epoch,"uploadId":UPLOAD,"offset":offset,"data":"YWJj"}}))
}
#[test]
fn upload_lease_and_sequence_come_from_shared_cursors_not_caller_identity() {
    let mut h = Harness::new();
    h.connect();
    let _view = NativeView::attach(&mut h);
    assert_eq!(
        h.operation(json!({"method":"beginTerminalUpload","payload":{
        "tabId":TAB,"generation":GEN,"controlEpoch":2,"fileName":"a.txt","size":3}})),
        (STALE, 0)
    );
    begin(&mut h);
    assert_eq!(chunk(&h, 2, 0), (STALE, 0));
    assert_eq!(chunk(&h, 1, 1), (INVALID, 0));
    assert_eq!(chunk(&h, 1, 0).0, 0);
    let request = h.request();
    assert_eq!(request.body["payload"]["controlEpoch"], 1);
    assert_eq!(chunk(&h, 1, 0), (BUSY, 0));
    assert_eq!(upload(&h, "finishTerminalUpload", 1), (BUSY, 0));
    assert_eq!(
        h.complete(&request, br#"{"ok":true,"value":{"offset":3}}"#),
        0
    );
    assert_eq!(h.event()["status"], 0);
    assert_eq!(chunk(&h, 1, 0), (INVALID, 0));
    assert_eq!(upload(&h, "finishTerminalUpload", 2), (STALE, 0));
    assert_eq!(upload(&h, "finishTerminalUpload", 1).0, 0);
    let finish = h.request();
    assert_eq!(
        h.complete(
            &finish,
            br#"{"ok":true,"value":{"path":"/fake-home/a.txt","fileName":"a.txt","size":3}}"#
        ),
        0
    );
    assert_eq!(h.event()["status"], 0);
    assert_eq!(upload(&h, "cancelTerminalUpload", 1), (STALE, 0));
}
#[test]
fn mismatched_upload_ack_poisoning_never_replays_an_uncertain_chunk() {
    let mut h = Harness::new();
    h.connect();
    let _view = NativeView::attach(&mut h);
    begin(&mut h);
    assert_eq!(chunk(&h, 1, 0).0, 0);
    let request = h.request();
    assert_eq!(
        h.complete(&request, br#"{"ok":true,"value":{"offset":2}}"#),
        0
    );
    let event = h.event();
    assert_eq!(event["status"], PROTOCOL);
    assert_eq!(event["internal"], false);
    assert_eq!(chunk(&h, 1, 0), (STALE, 0));
    assert_eq!(upload(&h, "cancelTerminalUpload", 1).0, 0);
    let cancel = h.request();
    assert_eq!(
        h.complete(&cancel, br#"{"ok":true,"value":{"cancelled":true}}"#),
        0
    );
    h.event();
    h.dispatch();
    assert!(h.mock.requests.is_empty());
}
#[test]
fn takeover_cancels_upload_chunk_but_cleanup_retains_original_fenced_identity() {
    let mut h = Harness::new();
    h.connect();
    let _view = NativeView::attach(&mut h);
    begin(&mut h);
    assert_eq!(chunk(&h, 1, 0).0, 0);
    let write = h.request();
    let mut op = 0;
    assert_eq!(unsafe { sfm_client_poll(h.handle, &mut op) }, 0);
    let poll = h.request();
    let blocked =
        include_bytes!("../../shellfox-client/tests/fixtures/terminal-control-event-success.json");
    assert_eq!(h.complete(&poll, blocked), 0);
    h.event();
    h.event();
    assert_eq!(
        h.complete(&write, br#"{"ok":true,"value":{"offset":3}}"#),
        STALE
    );
    assert_eq!(chunk(&h, 1, 0), (STALE, 0));
    assert_eq!(upload(&h, "cancelTerminalUpload", 2), (STALE, 0));
    assert_eq!(upload(&h, "cancelTerminalUpload", 1).0, 0);
    let cancel = h.request();
    assert_eq!(cancel.body["payload"]["controlEpoch"], 1);
    assert_eq!(
        h.complete(&cancel, br#"{"ok":true,"value":{"cancelled":true}}"#),
        0
    );
    h.event();
}
