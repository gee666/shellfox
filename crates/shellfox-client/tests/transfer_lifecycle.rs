use serde_json::{json, Value};
use shellfox_client::{protocol::*, *};
fn fixture(method: &str) -> Value {
    serde_json::from_str::<Vec<Value>>(include_str!("fixtures/remote-parity.json"))
        .unwrap()
        .into_iter()
        .find(|v| v["method"] == method)
        .unwrap()
}
fn read_header(size: u64) -> FileReadTransfer {
    let mut v = fixture("beginFileRead")["response"]["value"].clone();
    v["size"] = size.into();
    serde_json::from_value(v).unwrap()
}
fn writer(size: u64) -> FileWriteCursor {
    let f = fixture("beginFileWrite");
    let mut input = f["payload"].clone();
    input["size"] = size.into();
    FileWriteCursor::new(
        serde_json::from_value(input).unwrap(),
        serde_json::from_value(f["response"]["value"].clone()).unwrap(),
    )
    .unwrap()
}
#[test]
fn download_advances_only_after_sink_consumption_and_validates_eof_offset() {
    let mut cursor = FileReadCursor::new(read_header(3)).unwrap();
    assert_eq!(cursor.request().unwrap().offset, 0);
    assert!(matches!(cursor.request(), Err(TransferError::Busy)));
    let bytes = cursor
        .receive(&FileChunk {
            offset: 3,
            data: "YWJj".into(),
            eof: true,
        })
        .unwrap();
    assert_eq!(bytes, b"abc");
    assert_eq!(cursor.offset(), 0);
    assert!(!cursor.is_complete());
    assert!(matches!(cursor.request(), Err(TransferError::Busy)));
    assert_eq!(cursor.consumed().unwrap(), 3);
    assert!(cursor.is_complete());
    assert!(matches!(cursor.request(), Err(TransferError::Complete)));
    assert_eq!(
        cursor.cancel_request().transfer_id,
        cursor.header().transfer_id
    );
}
#[test]
fn empty_download_requires_valid_empty_final_chunk_and_never_spins() {
    let mut cursor = FileReadCursor::new(read_header(0)).unwrap();
    cursor.request().unwrap();
    assert!(cursor
        .receive(&FileChunk {
            offset: 0,
            data: "".into(),
            eof: true
        })
        .unwrap()
        .is_empty());
    cursor.consumed().unwrap();
    assert!(cursor.is_complete());
    for chunk in [
        FileChunk {
            offset: 0,
            data: "".into(),
            eof: false,
        },
        FileChunk {
            offset: 2,
            data: "YWJj".into(),
            eof: true,
        },
        FileChunk {
            offset: 3,
            data: "YWJj".into(),
            eof: false,
        },
    ] {
        let mut cursor = FileReadCursor::new(read_header(3)).unwrap();
        cursor.request().unwrap();
        assert_eq!(cursor.receive(&chunk).unwrap_err(), TransferError::Invalid);
        assert!(matches!(cursor.request(), Err(TransferError::Uncertain)));
    }
}
#[test]
fn write_cursor_serializes_mutations_and_never_replays_uncertain_chunk() {
    let mut cursor = writer(3);
    let request = cursor.write(b"ab").unwrap();
    assert_eq!(request.offset, 0);
    assert!(matches!(cursor.write(b"c"), Err(TransferError::Busy)));
    assert!(matches!(cursor.finish(), Err(TransferError::Busy)));
    cursor.written(&TransferOffset { offset: 2 }).unwrap();
    assert_eq!(cursor.offset(), 2);
    let request = cursor.write(b"c").unwrap();
    assert_eq!(request.offset, 2);
    assert_eq!(
        cursor.written(&TransferOffset { offset: 2 }).unwrap_err(),
        TransferError::Invalid
    );
    assert!(matches!(cursor.write(b"c"), Err(TransferError::Uncertain)));
    assert!(matches!(cursor.finish(), Err(TransferError::Uncertain)));
    assert_eq!(cursor.cancel_request().transfer_id, request.transfer_id);
}
#[test]
fn write_empty_and_positive_commit_ack_survive_cleanup_request_race() {
    let mut cursor = writer(0);
    assert!(matches!(cursor.write(b"x"), Err(TransferError::Invalid)));
    let finish = cursor.finish().unwrap();
    assert_eq!(cursor.cancel_request().transfer_id, finish.transfer_id);
    cursor
        .finished(&FileTransferResult {
            path: "/host/notes".into(),
            size: 0,
            revision: "a".repeat(64),
        })
        .unwrap();
    assert!(cursor.is_complete());
    assert!(matches!(cursor.finish(), Err(TransferError::Complete)));
    let mut cursor = writer(3);
    assert!(matches!(cursor.finish(), Err(TransferError::Invalid)));
    cursor.write(b"abc").unwrap();
    cursor.written(&TransferOffset { offset: 3 }).unwrap();
    cursor.finish().unwrap();
    assert!(cursor
        .finished(&FileTransferResult {
            path: "/host/notes".into(),
            size: 2,
            revision: "a".repeat(64)
        })
        .is_err());
}
fn upload(size: u64) -> (ViewCursor, UploadCursor) {
    let Reply::Success(a): Reply<TerminalAttachment> = decode(include_bytes!(
        "fixtures/terminal-attach-owned-success.json"
    ))
    .unwrap() else {
        panic!()
    };
    let view = ViewCursor::from_attachment(&a, 1).unwrap();
    let begin = view.begin_upload("image.png".into(), size, 1).unwrap();
    let started: TerminalUploadStarted =
        serde_json::from_value(fixture("beginTerminalUpload")["response"]["value"].clone())
            .unwrap();
    let cursor = UploadCursor::new(begin, started, &view, 1).unwrap();
    (view, cursor)
}
#[test]
fn upload_consumes_exact_chunk_acks_and_commit_without_automatic_terminal_input() {
    let (view, mut cursor) = upload(3);
    let request = cursor.write(b"abc", &view, 1).unwrap();
    assert_eq!(request.data, "YWJj");
    assert!(matches!(
        cursor.write(b"x", &view, 1),
        Err(UploadError::Busy)
    ));
    cursor
        .written(&TransferOffset { offset: 3 }, &view, 1)
        .unwrap();
    assert_eq!(cursor.offset(), 3);
    cursor.finish(&view, 1).unwrap();
    cursor.cancel_request(); // Positive finish still acknowledges an already-issued commit.
    cursor
        .finished(&TerminalUploadResult {
            path: "/host/image.png".into(),
            file_name: "image.png".into(),
            size: 3,
        })
        .unwrap();
    assert!(cursor.is_complete());
}
#[test]
fn upload_fences_takeover_repair_and_keeps_old_identity_only_for_cleanup() {
    let (mut view, mut cursor) = upload(3);
    let begin = view.begin_upload("image.png".into(), 3, 1).unwrap();
    let event = TerminalEvent::Control {
        tab_id: begin.tab_id,
        generation: begin.generation,
        control_epoch: begin.control_epoch + 1,
        control: Control::Blocked,
    };
    assert_eq!(view.observe(&event, 1).unwrap(), EventDisposition::Blocked);
    assert!(matches!(
        cursor.write(b"abc", &view, 1),
        Err(UploadError::Cursor(CursorError::NotOwned))
    ));
    let cleanup = cursor.cancel_request();
    assert_eq!(cleanup.control_epoch, begin.control_epoch);
    let (view, mut cursor) = upload(0);
    assert!(matches!(
        cursor.finish(&view, 2),
        Err(UploadError::Cursor(CursorError::StaleConnection))
    ));
    cursor.finish(&view, 1).unwrap();
    cursor.failed();
    assert!(matches!(
        cursor.finish(&view, 1),
        Err(UploadError::Uncertain)
    ));
}
#[test]
fn native_upload_lease_rejects_forged_command_identity_before_state_change() {
    let (view, _) = upload(3);
    let mut begin = view.begin_upload("image.png".into(), 3, 1).unwrap();
    begin.control_epoch += 1;
    assert_eq!(
        view.validate_upload_begin(&begin, 1).unwrap_err(),
        CursorError::NotOwned
    );
    let valid: WriteTerminalUpload =
        serde_json::from_value(fixture("writeTerminalUpload")["payload"].clone()).unwrap();
    for field in ["tabId", "generation", "controlEpoch", "uploadId", "offset"] {
        let (view, mut lease) = upload(3);
        let mut bad = serde_json::to_value(&valid).unwrap();
        bad[field] = if field == "offset" {
            json!(1)
        } else if field == "controlEpoch" {
            json!(2)
        } else {
            json!("60000000-0000-4000-8000-000000000001")
        };
        let bad: WriteTerminalUpload = serde_json::from_value(bad).unwrap();
        assert_eq!(
            lease.accept_write(&bad, &view, 1).unwrap_err(),
            UploadError::Invalid
        );
        lease.accept_write(&valid, &view, 1).unwrap();
        lease
            .written(&TransferOffset { offset: 3 }, &view, 1)
            .unwrap();
    }
}
#[test]
fn native_upload_lease_checks_finish_and_cancel_ids_without_borrowing_js_authority() {
    let (view, mut lease) = upload(0);
    let valid: FinishTerminalUpload =
        serde_json::from_value(fixture("finishTerminalUpload")["payload"].clone()).unwrap();
    let mut forged = valid.clone();
    forged.control_epoch += 1;
    assert_eq!(
        lease.accept_finish(&forged, &view, 1).unwrap_err(),
        UploadError::Invalid
    );
    lease.accept_finish(&valid, &view, 1).unwrap();
    let mut cleanup: CancelTerminalUpload =
        serde_json::from_value(fixture("cancelTerminalUpload")["payload"].clone()).unwrap();
    cleanup.upload_id = ClientId::new_v4();
    assert_eq!(
        lease.accept_cancel(&cleanup).unwrap_err(),
        UploadError::Invalid
    );
    cleanup.upload_id = valid.upload_id;
    lease.accept_cancel(&cleanup).unwrap();
    lease
        .finished(&TerminalUploadResult {
            path: "/host/image.png".into(),
            file_name: "image.png".into(),
            size: 0,
        })
        .unwrap();
}
#[test]
fn invalid_upload_names_chunks_and_identity_are_rejected_before_transport() {
    let (view, mut cursor) = upload(3);
    assert!(matches!(
        cursor.write(b"", &view, 1),
        Err(UploadError::Invalid)
    ));
    assert!(view.begin_upload("".into(), 3, 1).is_err());
    assert!(view
        .begin_upload("name".into(), FILE_TRANSFER_MAX_BYTES + 1, 1)
        .is_err());
    let mut begin = serde_json::to_value(view.begin_upload("file".into(), 0, 1).unwrap()).unwrap();
    begin["controlEpoch"] = json!(0);
    assert!(serde_json::from_value::<BeginTerminalUpload>(begin)
        .unwrap()
        .validate()
        .is_err());
}
