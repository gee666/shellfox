use shellfox_client::observer_v2::{CursorError, DeliveryCursor, EventDisposition};
use shellfox_protocol::{
    decode,
    observer_v2::{self, *},
};
fn bytes(name: &str) -> Vec<u8> {
    std::fs::read(format!(
        "{}/../shellfox-protocol/tests/fixtures/observer_v2/{name}.json",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}
fn setup() -> (Binding, DeliveryCursor) {
    let bound: BoundReply<Attachment> = decode(&bytes("attachment")).unwrap();
    let binding = bound.binding.clone();
    let observer_v2::Reply::Success(attachment) = bound.outcome else {
        panic!()
    };
    (
        binding.clone(),
        DeliveryCursor::new(binding, &attachment).unwrap(),
    )
}
fn data(cursor: &DeliveryCursor, sequence: u64) -> Event {
    Event::Data {
        delivery: cursor.delivery().clone(),
        sequence,
        encoding: Encoding::Base64,
        data: "QQBC".into(),
    }
}
#[test]
fn recovery_and_data_ack_require_explicit_consumption_not_receipt_or_queue() {
    let (binding, mut cursor) = setup();
    let event = data(&cursor, 5);
    assert_eq!(cursor.recovery_bytes().unwrap(), b"\x18\x1bc");
    assert_eq!(
        cursor.observe(&event, &binding),
        Err(CursorError::RecoveryNotConsumed)
    );
    assert!(cursor.recovery_request(&binding).unwrap().resume.is_none());
    assert_eq!(
        cursor.consumed_recovery(Id::new(), &binding).unwrap_err(),
        CursorError::InvalidMessage
    );
    let recovery_id = cursor.recovery().recovery_id;
    let ack = cursor.consumed_recovery(recovery_id, &binding).unwrap();
    cursor.validate_ack(&ack, &binding).unwrap();
    assert!(matches!(ack.ack, Ack::Recovery { sequence: 4, .. }));
    assert_eq!(
        cursor.observe(&event, &binding).unwrap(),
        EventDisposition::Deliver
    );
    let resume = cursor.recovery_request(&binding).unwrap().resume.unwrap();
    assert_eq!(resume.after_sequence, 4);
    assert_eq!(
        cursor.consumed_data(6, &binding).unwrap_err(),
        CursorError::InvalidSequence
    );
    let ack = cursor.consumed_data(5, &binding).unwrap();
    cursor.validate_ack(&ack, &binding).unwrap();
    cursor.consumed_recovery(recovery_id, &binding).unwrap();
    assert_eq!(
        cursor
            .recovery_request(&binding)
            .unwrap()
            .resume
            .unwrap()
            .after_sequence,
        5
    );
    assert_eq!(
        cursor.observe(&event, &binding).unwrap(),
        EventDisposition::Ignore
    );
}
#[test]
fn forged_acks_stale_delivery_and_connection_are_rejected() {
    let (binding, mut cursor) = setup();
    let recovery_id = cursor.recovery().recovery_id;
    cursor.consumed_recovery(recovery_id, &binding).unwrap();
    let forged = AcknowledgeTerminal {
        delivery: cursor.delivery().clone(),
        ack: Ack::Data { sequence: 5 },
    };
    assert_eq!(
        cursor.validate_ack(&forged, &binding),
        Err(CursorError::InvalidSequence)
    );
    let mut stale = binding.clone();
    stale.connection_id = Id::new();
    assert_eq!(
        cursor.consumed_data(5, &stale).unwrap_err(),
        CursorError::StaleConnection
    );
    let mut event = data(&cursor, 5);
    if let Event::Data { delivery, .. } = &mut event {
        delivery.delivery_epoch += 1;
    }
    assert_eq!(
        cursor.observe(&event, &binding),
        Err(CursorError::StaleDelivery)
    );
}
#[test]
fn gap_or_canonical_geometry_change_requires_recovery_without_layout_control() {
    let (binding, mut cursor) = setup();
    cursor
        .consumed_recovery(cursor.recovery().recovery_id, &binding)
        .unwrap();
    let event = data(&cursor, 6);
    assert_eq!(
        cursor.observe(&event, &binding).unwrap(),
        EventDisposition::Reattach
    );
    assert_eq!(
        cursor.consumed_data(6, &binding).unwrap_err(),
        CursorError::RecoveryRequired
    );
    let (binding, mut cursor) = setup();
    let event: Event = decode(&bytes("layout")).unwrap();
    assert_eq!(
        cursor.observe(&event, &binding).unwrap(),
        EventDisposition::Reattach
    );
    assert_eq!(
        cursor
            .consumed_recovery(cursor.recovery().recovery_id, &binding)
            .unwrap_err(),
        CursorError::RecoveryRequired
    );
    assert!(cursor.recovery_request(&binding).unwrap().resume.is_none());
}
#[test]
fn old_acks_cannot_accept_a_new_layout_stamp_or_bypass_full_recovery() {
    let (binding, mut cursor) = setup();
    let old_recovery = cursor
        .consumed_recovery(cursor.recovery().recovery_id, &binding)
        .unwrap();
    cursor.observe(&data(&cursor, 5), &binding).unwrap();
    let old_data = cursor.consumed_data(5, &binding).unwrap();
    let layout: Event = decode(&bytes("layout")).unwrap();
    assert_eq!(
        cursor.observe(&layout, &binding).unwrap(),
        EventDisposition::Reattach
    );
    assert_eq!(
        cursor.validate_ack(&old_recovery, &binding),
        Err(CursorError::RecoveryRequired)
    );
    assert_eq!(
        cursor.validate_ack(&old_data, &binding),
        Err(CursorError::RecoveryRequired)
    );
    assert_eq!(
        cursor.observe(&data(&cursor, 6), &binding).unwrap(),
        EventDisposition::Reattach
    );
    assert!(serde_json::from_value::<Ack>(
        serde_json::json!({"kind":"layout","sequence":5,"layoutRevision":2})
    )
    .is_err());
    let mut recovery = cursor.recovery().clone();
    recovery.recovery_id = Id::new();
    recovery.last_sequence = 5;
    recovery.cols = 100;
    recovery.rows = 30;
    recovery.layout_revision = 2;
    let mut delivery = cursor.delivery().clone();
    delivery.delivery_id = Id::new();
    delivery.delivery_epoch += 1;
    let attachment = Attachment::Attached {
        delivery,
        input: (),
        layout: Layout::FixedCanonicalGrid,
        recovery,
    };
    let mut next = DeliveryCursor::new(binding.clone(), &attachment).unwrap();
    let ack = next
        .consumed_recovery(next.recovery().recovery_id, &binding)
        .unwrap();
    next.validate_ack(&ack, &binding).unwrap();
    assert_eq!(next.recovery().layout_revision, 2);
    assert_eq!((next.recovery().cols, next.recovery().rows), (100, 30));
    assert!(matches!(ack.ack, Ack::Recovery { sequence: 5, .. }));
    assert_eq!(
        next.validate_ack(&old_recovery, &binding),
        Err(CursorError::StaleDelivery)
    );
}
#[test]
fn source_end_is_not_process_exit_or_cleanup_and_revocation_stops_delivery() {
    let (binding, mut cursor) = setup();
    cursor
        .consumed_recovery(cursor.recovery().recovery_id, &binding)
        .unwrap();
    let event = data(&cursor, 5);
    cursor.observe(&event, &binding).unwrap();
    cursor.consumed_data(5, &binding).unwrap();
    let ended: Event = decode(&bytes("source-ended")).unwrap();
    assert!(matches!(ended, Event::SourceEnded { .. }));
    assert_eq!(
        cursor.observe(&ended, &binding).unwrap(),
        EventDisposition::Deliver
    );
    assert_eq!(
        cursor.observe(&data(&cursor, 6), &binding),
        Err(CursorError::InvalidSequence)
    );
    let exited: Event = decode(&bytes("process-exited")).unwrap();
    assert!(matches!(
        exited,
        Event::ProcessExited {
            exit_code: Some(0),
            ..
        }
    ));
    let revoked = Event::Revoked {
        delivery: cursor.delivery().clone(),
        reason: RevokedReason::ApprovalRevoked,
    };
    assert_eq!(
        cursor.observe(&revoked, &binding).unwrap(),
        EventDisposition::Revoked
    );
    assert_eq!(
        cursor.consumed_data(5, &binding).unwrap_err(),
        CursorError::Revoked
    );
    assert_eq!(
        cursor.detach_request(&binding).unwrap().delivery,
        *cursor.delivery()
    );
}
