use serde_json::{json, Value};
use shellfox_client::manager_v2::{
    CursorError, EventDisposition, ManagerCursor, UploadCursor, UploadError,
};
use shellfox_protocol::{manager_v2 as m, *};
fn bytes(name: &str) -> Vec<u8> {
    std::fs::read(format!(
        "{}/../shellfox-protocol/tests/fixtures/manager_v2/{name}.json",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}
fn id() -> m::Id {
    m::Id::new()
}
fn setup(name: &str) -> (m::Binding, ManagerCursor) {
    let reply: m::BoundReply<m::Attachment> = decode(&bytes(name)).unwrap();
    let binding = reply.binding.clone();
    let m::Reply::Success(attachment) = reply.outcome else {
        panic!()
    };
    (
        binding.clone(),
        ManagerCursor::new(binding, &attachment).unwrap(),
    )
}
fn ready(binding: &m::Binding, cursor: &mut ManagerCursor) {
    let ack = cursor
        .consumed_recovery(cursor.recovery().recovery_id(), binding)
        .unwrap();
    cursor
        .acknowledged(&ack, &Acknowledged { acknowledged: true }, binding)
        .unwrap();
}
fn hints() -> m::InputHints {
    m::InputHints {
        user_initiated: true,
        activity_hint: true,
        settled_at: Some(None),
    }
}
fn input() -> m::Input {
    m::Input::Bytes {
        encoding: m::Encoding::Base64,
        data: "gAD/DQ==".into(),
        origin: m::BytesOrigin::Key,
    }
}
fn data(cursor: &ManagerCursor, seq: u64) -> m::Event {
    m::Event::Data {
        delivery: cursor.delivery().clone(),
        sequence: seq,
        encoding: m::Encoding::Base64,
        data: "QQBC".into(),
    }
}
#[test]
fn actual_control_requires_full_consumption_and_exact_successful_ack() {
    let (binding, mut cursor) = setup("attachment");
    assert!(matches!(
        cursor.control(&binding),
        Err(CursorError::Delivery(_))
    ));
    let mut ack = cursor
        .consumed_recovery(cursor.recovery().recovery_id(), &binding)
        .unwrap();
    assert!(cursor.control(&binding).is_err());
    if let m::Ack::Recovery {
        layout_revision, ..
    } = &mut ack.ack
    {
        *layout_revision += 1;
    }
    assert!(cursor
        .acknowledged(&ack, &Acknowledged { acknowledged: true }, &binding)
        .is_err());
    assert!(cursor.control(&binding).is_err());
    ready(&binding, &mut cursor);
    assert_eq!(
        cursor.control(&binding).unwrap().control_epoch,
        cursor.delivery().delivery_epoch
    );
    assert_eq!(cursor.recovery_bytes().unwrap(), b"\x18\x1bc");
}
#[test]
fn busy_or_intrinsic_delivery_and_pair_write_never_mint_a_controller() {
    for name in ["attachment-busy", "attachment-intrinsic"] {
        let (binding, mut cursor) = setup(name);
        ready(&binding, &mut cursor);
        assert_eq!(
            cursor.control(&binding).unwrap_err(),
            CursorError::ControlRequired
        );
        assert!(cursor
            .admit_input(id(), input(), hints(), &binding)
            .is_err());
        assert!(cursor.begin_upload("file".into(), 3, &binding).is_err());
        if name == "attachment-intrinsic" {
            assert_eq!(
                cursor.takeover_request(&binding).unwrap_err(),
                CursorError::IntrinsicReadOnly
            );
        } else {
            let request = cursor.takeover_request(&binding).unwrap();
            assert_eq!(request.delivery.delivery_epoch, 2);
            assert!(cursor.control(&binding).is_err());
        }
    }
}
#[test]
fn frozen_hints_and_receipt_survive_control_loss_without_regrant_or_resample() {
    let (binding, mut cursor) = setup("attachment");
    ready(&binding, &mut cursor);
    let input_id = id();
    let mut original = hints();
    let request = cursor
        .admit_input(input_id, input(), original.clone(), &binding)
        .unwrap();
    original.settled_at = Some(Some("2026-11-13T12:00:00.000Z".into()));
    assert_eq!(request.hints.settled_at, Some(None));
    assert_eq!(
        cursor
            .admit_input(input_id, input(), original, &binding)
            .unwrap_err(),
        CursorError::DuplicateInput
    );
    let event: m::Event = decode(&bytes("revoked-control")).unwrap();
    assert_eq!(
        cursor.observe(&event, &binding).unwrap(),
        EventDisposition::Revoked
    );
    let accepted = cursor
        .accepted(
            m::AcceptedInput {
                input_id,
                kind: m::InputKind::Bytes,
                accepted_bytes: 4,
                disposition: m::Disposition::Accepted,
            },
            &binding,
        )
        .unwrap();
    assert_eq!(accepted.hints.settled_at, Some(None));
    assert_eq!(cursor.pending_inputs(), 0);
    assert!(cursor
        .admit_input(id(), input(), hints(), &binding)
        .is_err());
    assert!(matches!(
        cursor.recovery_request(&binding).unwrap().control,
        m::ControlRequest::None
    ));
}
#[test]
fn unknown_input_is_never_canceled_as_safe_or_replayed_under_new_id() {
    let (binding, mut cursor) = setup("attachment");
    ready(&binding, &mut cursor);
    let input_id = id();
    cursor
        .admit_input(input_id, input(), hints(), &binding)
        .unwrap();
    cursor.input_unknown(input_id).unwrap();
    assert_eq!(
        cursor.cancel_before_send(input_id).unwrap_err(),
        CursorError::OutcomeUnknown
    );
    assert_eq!(
        cursor
            .admit_input(input_id, input(), hints(), &binding)
            .unwrap_err(),
        CursorError::DuplicateInput
    );
    let bad = m::AcceptedInput {
        input_id,
        kind: m::InputKind::Bytes,
        accepted_bytes: 3,
        disposition: m::Disposition::Accepted,
    };
    assert!(cursor.accepted(bad, &binding).is_err());
    assert_eq!(cursor.pending_inputs(), 1);
}
#[test]
fn bounded_input_admission_preserves_unavailable_not_legacy_or_null() {
    let (binding, mut cursor) = setup("attachment");
    ready(&binding, &mut cursor);
    for _ in 0..16 {
        cursor
            .admit_input(
                id(),
                input(),
                m::InputHints {
                    user_initiated: true,
                    activity_hint: true,
                    settled_at: None,
                },
                &binding,
            )
            .unwrap();
    }
    assert_eq!(cursor.pending_inputs(), 16);
    assert_eq!(
        cursor
            .admit_input(id(), input(), hints(), &binding)
            .unwrap_err(),
        CursorError::Busy
    );
}
#[test]
fn resize_and_layout_use_actual_stamp_and_block_old_acks_and_side_effects() {
    let (binding, mut cursor) = setup("attachment");
    ready(&binding, &mut cursor);
    let unchanged = cursor.resize(120, 32, &binding).unwrap();
    assert_eq!(unchanged.expected_layout_revision, 3);
    assert!(cursor.control(&binding).is_ok());
    let changed = cursor.resize(132, 32, &binding).unwrap();
    assert_eq!(changed.expected_layout_revision, 3);
    assert_eq!(cursor.recovery().layout_revision(), 3);
    assert!(cursor.control(&binding).is_err());
    assert!(cursor
        .admit_input(id(), input(), hints(), &binding)
        .is_err());
    assert!(cursor.begin_upload("file".into(), 3, &binding).is_err());
    let (binding, mut cursor) = setup("attachment");
    ready(&binding, &mut cursor);
    let ack = cursor
        .consumed_recovery(cursor.recovery().recovery_id(), &binding)
        .unwrap();
    let layout = m::Event::Layout {
        delivery: cursor.delivery().clone(),
        layout_revision: 4,
        cols: 132,
        rows: 32,
        recovery_required: true,
        last_sequence: 12,
    };
    assert_eq!(
        cursor.observe(&layout, &binding).unwrap(),
        EventDisposition::Reattach
    );
    assert!(cursor.validate_ack(&ack, &binding).is_err());
    assert_eq!(
        cursor.observe(&data(&cursor, 13), &binding).unwrap(),
        EventDisposition::Reattach
    );
}
#[test]
fn mouse_has_frozen_actual_recovery_and_geometry_no_encoded_string() {
    let (binding, mut cursor) = setup("attachment");
    ready(&binding, &mut cursor);
    let command = m::decode_request(&bytes("request-write-mouse")).unwrap();
    let m::Command::WriteTerminal(request) = command.command else {
        panic!()
    };
    let m::Input::Mouse { event, .. } = request.input else {
        panic!()
    };
    let request = cursor
        .admit_mouse(id(), event, 8, 16, hints(), &binding)
        .unwrap();
    let m::Input::Mouse { geometry, .. } = request.input else {
        panic!()
    };
    assert_eq!(geometry.recovery_id, cursor.recovery().recovery_id());
    assert_eq!(geometry.layout_revision, 3);
    assert_eq!((geometry.cols, geometry.rows), (120, 32));
}
#[test]
fn controlled_detach_must_release_real_controller_and_never_kills_source() {
    let (binding, mut cursor) = setup("attachment");
    ready(&binding, &mut cursor);
    assert_eq!(
        cursor.detach_request(false, &binding).unwrap_err(),
        CursorError::ControlReleaseRequired
    );
    let request = cursor.detach_request(true, &binding).unwrap();
    assert_eq!(
        request.release_control.unwrap().control_epoch,
        request.delivery.delivery_epoch
    );
    assert!(cursor.control(&binding).is_err());
    let raw = serde_json::to_value(request.delivery).unwrap();
    assert!(raw.get("pid").is_none());
}
#[test]
fn uploads_are_fenced_by_real_control_and_cleanup_keeps_original_ref() {
    let (binding, mut cursor) = setup("attachment");
    ready(&binding, &mut cursor);
    let begin = cursor.begin_upload("file.bin".into(), 3, &binding).unwrap();
    let original = begin.control.clone();
    let mut upload = UploadCursor::new(
        begin,
        m::TerminalUploadStarted {
            upload_id: id(),
            chunk_bytes: FILE_CHUNK_BYTES,
        },
        &cursor,
        &binding,
    )
    .unwrap();
    let request = upload.write(b"abc", &cursor, &binding).unwrap();
    assert_eq!(request.control, original);
    upload
        .written(&TransferOffset { offset: 3 }, &cursor, &binding)
        .unwrap();
    let event: m::Event = decode(&bytes("revoked-control")).unwrap();
    cursor.observe(&event, &binding).unwrap();
    assert_eq!(
        upload.finish(&cursor, &binding).unwrap_err(),
        UploadError::ControlRequired
    );
    assert_eq!(upload.cancel_request().control, original);
}
#[test]
fn manager_and_observer_control_json_are_separate() {
    let raw: Value = serde_json::from_slice(&bytes("attachment")).unwrap();
    assert!(decode::<
        shellfox_protocol::observer_v2::BoundReply<shellfox_protocol::observer_v2::Attachment>,
    >(&serde_json::to_vec(&raw).unwrap())
    .is_err());
    assert!(serde_json::from_value::<m::InputHints>(
        json!({"userInitiated":true,"activityHint":true,"settledAt":null,"permission":"write"})
    )
    .is_err());
}
