use shellfox_client::{protocol::*, *};

fn attachment() -> TerminalAttachment {
    let Reply::Success(a) = decode(include_bytes!(
        "fixtures/terminal-attach-owned-success.json"
    ))
    .unwrap() else {
        panic!()
    };
    a
}
fn owned(a: &TerminalAttachment) -> &OwnedAttachment {
    let TerminalAttachment::Owned(a) = a else {
        panic!()
    };
    a
}
#[test]
fn consumed_output_creates_explicit_ack_with_current_fence() {
    let a = attachment();
    let identity = owned(&a);
    let mut cursor = ViewCursor::from_attachment(&a, 1).unwrap();
    let ack = cursor.consumed(identity.last_sequence, 1).unwrap();
    assert_eq!(ack.control_epoch, identity.control_epoch);
    assert_eq!(ack.sequence, identity.last_sequence);
    let data = TerminalEvent::Data {
        tab_id: identity.tab_id,
        generation: identity.generation,
        control_epoch: identity.control_epoch,
        sequence: identity.last_sequence + 1,
        data: "next".into(),
    };
    assert_eq!(cursor.observe(&data, 1), Ok(EventDisposition::Deliver));
    // Receiving did not advance consumed. Reconnect resumes after the view's consumed sequence.
    assert_eq!(
        cursor.recovery_request(1).unwrap().after_sequence,
        Some(identity.last_sequence)
    );
    assert_eq!(
        cursor.consumed(identity.last_sequence + 2, 1).unwrap_err(),
        CursorError::NotConsumedInOrder
    );
    assert_eq!(
        cursor
            .consumed(identity.last_sequence + 1, 1)
            .unwrap()
            .sequence,
        identity.last_sequence + 1
    );
    assert_eq!(cursor.observe(&data, 1), Ok(EventDisposition::Ignore));
}
#[test]
fn lost_sequence_requires_recovery_and_disables_input() {
    let a = attachment();
    let identity = owned(&a);
    let mut cursor = ViewCursor::from_attachment(&a, 1).unwrap();
    let data = TerminalEvent::Data {
        tab_id: identity.tab_id,
        generation: identity.generation,
        control_epoch: identity.control_epoch,
        sequence: identity.last_sequence + 2,
        data: "gap".into(),
    };
    assert_eq!(cursor.observe(&data, 1), Ok(EventDisposition::Reattach));
    assert_eq!(
        cursor.write("x".into(), None, 1).unwrap_err(),
        CursorError::NotOwned
    );
    assert_eq!(
        cursor.consumed(identity.last_sequence + 2, 1).unwrap_err(),
        CursorError::NotOwned
    );
}
#[test]
fn takeover_fences_writes_ack_and_requires_new_attachment() {
    let a = attachment();
    let identity = owned(&a);
    let mut cursor = ViewCursor::from_attachment(&a, 1).unwrap();
    let blocked = TerminalEvent::Control {
        tab_id: identity.tab_id,
        generation: identity.generation,
        control_epoch: identity.control_epoch + 1,
        control: Control::Blocked,
    };
    assert_eq!(cursor.observe(&blocked, 1), Ok(EventDisposition::Blocked));
    assert_eq!(cursor.resize(80, 24, 1).unwrap_err(), CursorError::NotOwned);
    let old = TerminalEvent::Data {
        tab_id: identity.tab_id,
        generation: identity.generation,
        control_epoch: identity.control_epoch,
        sequence: identity.last_sequence + 1,
        data: "old".into(),
    };
    assert_eq!(cursor.observe(&old, 1), Ok(EventDisposition::Ignore));
    let owned = TerminalEvent::Control {
        tab_id: identity.tab_id,
        generation: identity.generation,
        control_epoch: identity.control_epoch + 2,
        control: Control::Owned,
    };
    assert_eq!(cursor.observe(&owned, 1), Ok(EventDisposition::Reattach));
    assert_eq!(
        cursor.write("x".into(), None, 1).unwrap_err(),
        CursorError::NotOwned
    );
}
#[test]
fn blocked_attachment_contains_no_content_and_cannot_ack_or_write() {
    let Reply::Success(a): Reply<TerminalAttachment> = decode(include_bytes!(
        "fixtures/terminal-attach-blocked-success.json"
    ))
    .unwrap() else {
        panic!()
    };
    let mut cursor = ViewCursor::from_attachment(&a, 3).unwrap();
    assert_eq!(cursor.consumed(0, 3).unwrap_err(), CursorError::NotOwned);
    assert_eq!(
        cursor.write("x".into(), Some(true), 3).unwrap_err(),
        CursorError::NotOwned
    );
}
#[test]
fn cursor_is_invalid_after_successful_repair_even_if_generation_unchanged() {
    let a = attachment();
    let mut cursor = ViewCursor::from_attachment(&a, 1).unwrap();
    assert_eq!(
        cursor.consumed(1, 2).unwrap_err(),
        CursorError::StaleConnection
    );
    assert_eq!(
        cursor.recovery_request(2).unwrap_err(),
        CursorError::StaleConnection
    );
}
#[test]
fn recovery_is_ansi_not_a_ghostty_binary_snapshot() {
    let Reply::Success(a): Reply<TerminalAttachment> = decode(include_bytes!(
        "fixtures/terminal-attach-recovery-success.json"
    ))
    .unwrap() else {
        panic!()
    };
    let identity = owned(&a);
    assert_eq!(identity.snapshot, Some(true));
    assert!(identity.chunks[0].data.starts_with("\u{1b}["));
    let mut cursor = ViewCursor::from_attachment(&a, 1).unwrap();
    assert_eq!(
        cursor.consumed(identity.last_sequence, 1).unwrap().sequence,
        identity.last_sequence
    );
}
