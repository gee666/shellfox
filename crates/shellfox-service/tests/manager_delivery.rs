//! Approved standalone actual-source harness: production mailbox/delivery files
//! plus one real in-memory ExternalBytes Runtime receiver per observer fixture.
//! Controller tests below are REFERENCE CODEC ONLY, not Native grant acceptance.
#[path = "support/manager_delivery_codec.rs"]
mod codec;
#[path = "support/manager_delivery_fixture.rs"]
mod fixture;
#[path = "../src/mailbox_custody.rs"]
mod mailbox_custody;
#[path = "../src/manager_delivery.rs"]
pub mod manager_delivery;
#[path = "../src/manager_mailboxes.rs"]
mod manager_mailboxes;
use fixture::Fixture;
use manager_delivery::{Failure, ManagerDelivery, Role};
use manager_mailboxes::ManagerMailboxes;
use shellfox_core::{Generation, SessionId, TabId};
use shellfox_protocol::{manager_v2 as wire, Validate};
use shellfox_runtime::*;
use std::time::{Duration, Instant};
fn identity() -> Identity {
    Identity {
        session_id: SessionId::new(),
        tab_id: TabId::new(),
        generation: Generation::new(),
    }
}
#[test]
fn actual_observer_credit_retains_lost_poll_bytes_and_source_end_is_not_process_exit() {
    let mut fixture = Fixture::new();
    assert!(fixture.book().control().is_none());
    assert!(fixture.book().layout().is_none());
    assert_eq!(
        fixture
            .request(Command::Claim {
                controller: 77,
                takeover: true
            })
            .unwrap_err()
            .code,
        ErrorCode::Unsupported
    );
    fixture.ready();
    fixture.feed(b"live\x1b[31m!");
    let offered = fixture.events_until(|events| {
        events
            .iter()
            .any(|event| matches!(event, wire::Event::Data { .. }))
    });
    let repeated = fixture
        .book()
        .poll(wire::MAX_POLL_DATA_BYTES, 1024)
        .unwrap();
    assert_eq!(
        serde_json::to_value(&offered).unwrap(),
        serde_json::to_value(repeated).unwrap()
    );
    let mut through = 0;
    for event in &offered {
        event.validate().unwrap();
        if let wire::Event::Data { sequence, .. } = event {
            through = *sequence;
        }
    }
    let reference = fixture.reference.clone();
    assert_eq!(
        fixture
            .book()
            .ack_command(
                &reference,
                &wire::Ack::Data {
                    sequence: through + 1
                }
            )
            .unwrap_err(),
        Failure::Invalid
    );
    let mut wrong = reference.clone();
    wrong.delivery_id = wire::Id::new();
    assert_eq!(
        fixture
            .book()
            .ack_command(&wrong, &wire::Ack::Data { sequence: through })
            .unwrap_err(),
        Failure::Stale
    );
    fixture.ack(wire::Ack::Data { sequence: through });
    assert!(fixture
        .book()
        .poll(wire::MAX_POLL_DATA_BYTES, 1024)
        .unwrap()
        .is_empty());
    fixture.finish();
    let ended = fixture.events_until(|events| {
        events
            .iter()
            .any(|event| matches!(event, wire::Event::SourceEnded { .. }))
    });
    assert!(!ended
        .iter()
        .any(|event| matches!(event, wire::Event::ProcessExited { .. })));
    assert!(fixture.runtime.external_status().unwrap().finished);
    assert!(!fixture.runtime.external_status().unwrap().closed);
}
#[test]
fn actual_opaque_replacement_rejects_old_wire_and_native_recovery_ack() {
    let mut fixture = Fixture::new();
    let first = fixture.recover();
    let reference = fixture.reference.clone();
    let ack = wire::Ack::Recovery {
        recovery_id: first.recovery_id(),
        sequence: first.last_sequence(),
        layout_revision: first.layout_revision(),
    };
    let old_native = fixture.book().ack_command(&reference, &ack).unwrap();
    let second = fixture.recover();
    assert_eq!(first.last_sequence(), second.last_sequence());
    assert_eq!(first.layout_revision(), second.layout_revision());
    assert_ne!(first.recovery_id(), second.recovery_id());
    assert_eq!(
        fixture.book().ack_command(&reference, &ack).unwrap_err(),
        Failure::Invalid
    );
    assert!(fixture.request(old_native).is_err());
    fixture.ack(wire::Ack::Recovery {
        recovery_id: second.recovery_id(),
        sequence: second.last_sequence(),
        layout_revision: second.layout_revision(),
    });
}
#[test]
fn actual_layout_barrier_requires_new_full_recovery_not_geometry_authority() {
    let mut fixture = Fixture::new();
    let old = fixture.ready();
    let observer = fixture.observer;
    fixture
        .request(Command::Presentation {
            view: PresentationLease::Observer(observer),
            action: PresentationAction::ResizeExternal {
                cols: 100,
                rows: 30,
            },
        })
        .unwrap();
    let layouts = fixture.events_until(|events| {
        events.iter().any(|event| {
            matches!(
                event,
                wire::Event::Layout {
                    cols: 100,
                    rows: 30,
                    ..
                }
            )
        })
    });
    assert!(matches!(
        layouts[0],
        wire::Event::Layout {
            recovery_required: true,
            ..
        }
    ));
    let reference = fixture.reference.clone();
    assert!(fixture
        .book()
        .ack_command(
            &reference,
            &wire::Ack::Recovery {
                recovery_id: old.recovery_id(),
                sequence: old.last_sequence(),
                layout_revision: old.layout_revision()
            }
        )
        .is_err());
    let fresh = fixture.recover();
    assert_eq!((fresh.cols(), fresh.rows()), (100, 30));
    assert!(fresh.layout_revision() > old.layout_revision());
    fixture.ack(wire::Ack::Recovery {
        recovery_id: fresh.recovery_id(),
        sequence: fresh.last_sequence(),
        layout_revision: fresh.layout_revision(),
    });
    assert_eq!(
        fixture.book().current_layout().unwrap().layout_revision,
        fresh.layout_revision()
    );
    assert_eq!(
        fixture.book().layout_revision(),
        Some(fresh.layout_revision())
    );
}
#[test]
fn actual_observer_revocation_purges_before_retirement_and_late_output_cannot_refill() {
    let mut fixture = Fixture::new();
    fixture.ready();
    fixture.feed(b"held");
    fixture.events_until(|events| {
        events
            .iter()
            .any(|event| matches!(event, wire::Event::Data { .. }))
    });
    let identity = fixture.identity();
    let role = fixture.role();
    let reference = fixture.reference.clone();
    fixture.mailboxes.revoke_source(identity);
    assert!(matches!(
        fixture.mailboxes.revocation(&reference),
        Some(wire::RevokedReason::PolicyChanged)
    ));
    assert!(fixture.book().revoked());
    assert!(matches!(
        fixture.book().revoked_reason(),
        Some(wire::RevokedReason::PolicyChanged)
    ));
    assert!(fixture.mailboxes.retirements().contains(&(identity, role)));
    fixture.feed(b"late");
    let deadline = Instant::now() + Duration::from_secs(10);
    while fixture.runtime.external_status().unwrap().source_sequence != SourceSequence(2) {
        fixture.drain();
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    fixture.drain();
    assert!(fixture
        .book()
        .poll(1024, 1024)
        .unwrap()
        .iter()
        .all(|event| matches!(event, wire::Event::Revoked { .. })));
    fixture.retire();
    assert!(matches!(
        fixture.mailboxes.revocation(&reference),
        Some(wire::RevokedReason::PolicyChanged)
    ));
    assert!(!fixture.runtime.external_status().unwrap().closed);
    fixture.mailboxes.forget_notice(&reference);
    assert!(fixture.mailboxes.revocation(&reference).is_none());
    assert!(fixture.mailboxes.can_insert());
}
#[test]
fn actual_provisional_observer_discard_never_issues_duplicate_unobserve() {
    let mut fixture = Fixture::new_provisional();
    let identity = fixture.identity();
    let role = fixture.role();
    let reference = fixture.reference.clone();
    let event = Event::Started(Ok(()));
    assert!(matches!(
        fixture.mailboxes.record(identity, event),
        Some(Event::Started(Ok(())))
    ));
    assert!(fixture
        .book()
        .recover(Response::Ansi {
            capability: AnsiRecoveryCapability::VisibleScreen,
            bytes: Vec::new(),
            last_sequence: 0
        })
        .is_err());
    fixture.mailboxes.discard_unadopted(identity, role).unwrap();
    let observer = fixture.observer;
    assert!(
        matches!(
            fixture
                .request(Command::ObserverRecoverStamped {
                    observer,
                    capability: AnsiRecoveryCapability::FormattedActiveBufferV1
                })
                .unwrap(),
            Response::Recovery(_)
        ),
        "discard removed only bookkeeping, not the actual grant"
    );
    fixture.mailboxes.forget_notice(&reference);
    assert!(
        fixture.mailboxes.retirements().is_empty(),
        "provisional discard did not acquire cleanup ownership"
    );
    // This public-Runtime fixture still owns its original actual observer.
    // Confirm its explicit cleanup through the same sole receiver.
    assert!(matches!(
        fixture.request(Command::Unobserve { observer }).unwrap(),
        Response::Applied
    ));
    assert!(fixture
        .request(Command::ObserverAck {
            observer,
            sequence: 0
        })
        .is_err());
    assert!(fixture.mailboxes.retirements().is_empty());
    assert!(!fixture.runtime.external_status().unwrap().closed);
}
