use super::*;
fn key(text: &str) -> OwnedKeyEventV2 {
    OwnedKeyEventV2 {
        physical: PhysicalKey::KeyA,
        action: KeyActionV2::Press,
        modifiers: ModifiersV2::NONE,
        consumed_modifiers: ModifiersV2::NONE,
        text: text.into(),
        unshifted_codepoint: Some('a'),
        composing: false,
    }
}
#[test]
fn key_v2_uses_actual_controller_and_pending_stamp_before_any_encoding_or_write() {
    let mut owner = test_support::owner();
    let lease = owner.delivery.claim(7, false).unwrap();
    let snapshot = match owner
        .command(
            RequestId(1),
            Command::RecoverStamped {
                lease,
                capability: AnsiRecoveryCapability::FormattedActiveBufferV1,
            },
        )
        .unwrap()
    {
        Response::Recovery(snapshot) => snapshot,
        _ => panic!(),
    };
    assert_eq!(
        owner
            .command(
                RequestId(2),
                Command::KeyV2 {
                    lease,
                    event: key("a")
                }
            )
            .unwrap_err()
            .code,
        ErrorCode::RecoveryRequired
    );
    assert_eq!(
        owner
            .command(
                RequestId(3),
                Command::KeyV2 {
                    lease: Lease {
                        epoch: lease.epoch + 1,
                        ..lease
                    },
                    event: key("a")
                }
            )
            .unwrap_err()
            .code,
        ErrorCode::ControlRequired
    );
    owner
        .command(
            RequestId(4),
            Command::ControllerLayoutAck {
                lease,
                recovery_id: snapshot.recovery_id,
                last_delivery_sequence: snapshot.last_delivery_sequence,
                layout_revision: snapshot.layout_revision,
            },
        )
        .unwrap();
    let mut preedit = key("á");
    preedit.composing = true;
    assert!(matches!(
        owner
            .command(
                RequestId(5),
                Command::KeyV2 {
                    lease,
                    event: preedit
                }
            )
            .unwrap(),
        Response::Input(InputReceipt {
            accepted_bytes: 0,
            kind: InputKind::Key {
                has_committed_text: false,
                ..
            }
        })
    ));
    let mut invalid = key("a");
    invalid.unshifted_codepoint = Some('\0');
    assert_eq!(
        owner
            .command(
                RequestId(6),
                Command::KeyV2 {
                    lease,
                    event: invalid
                }
            )
            .unwrap_err()
            .code,
        ErrorCode::Validation
    );
    owner.canonical_resize(100, 30).unwrap();
    assert_eq!(
        owner
            .command(
                RequestId(7),
                Command::KeyV2 {
                    lease,
                    event: key("a")
                }
            )
            .unwrap_err()
            .code,
        ErrorCode::RecoveryRequired
    );
}
#[test]
fn owned_key_v2_fifo_payload_bound_is_not_the_wrapper_allocation_bound() {
    fn owned_fifo<T: Send + 'static>() {}
    owned_fifo::<OwnedKeyEventV2>();
    owned_fifo::<Command>();
    let lease = Lease {
        controller: 1,
        epoch: 1,
    };
    assert!(Command::KeyV2 {
        lease,
        event: key(&"x".repeat(MAX_INPUT_PAYLOAD_BYTES))
    }
    .validate(GridLimits::default())
    .is_ok());
    assert_eq!(
        Command::KeyV2 {
            lease,
            event: key(&"x".repeat(MAX_INPUT_PAYLOAD_BYTES + 1))
        }
        .validate(GridLimits::default())
        .unwrap_err()
        .code,
        ErrorCode::Validation
    );
}
