//! Injected-state ledger mechanics; no runtime or controller.
use super::*;
mod pending_claim;
use shellfox_core::{Generation, SessionId, TabId};
use shellfox_protocol::CancellationToken;
use shellfox_runtime::*;
fn identity() -> Identity {
    Identity {
        session_id: SessionId::new(),
        tab_id: TabId::new(),
        generation: Generation::new(),
    }
}
fn claim(
    ledger: &mut GrantCustody,
    identity: Identity,
    id: u64,
    controller: u64,
) -> (GrantTicket, GrantGuard) {
    let reservation = ledger
        .reserve(
            identity,
            RequestId(id),
            GrantKind::Controller(controller),
            CancellationToken::new(),
        )
        .unwrap();
    ledger.sent(reservation.0).unwrap();
    reservation
}
fn granted(ledger: &mut GrantCustody, identity: Identity, id: u64, lease: Lease) {
    assert!(ledger
        .record_reply(identity, RequestId(id), &Ok(Response::Claimed(lease)))
        .unwrap());
}
#[test]
fn dropped_wait_late_reply_conflicting_retry_and_full_cleanup_keep_original_grant() {
    let identity = identity();
    let mut ledger = GrantCustody::default();
    let (ticket, guard) = claim(&mut ledger, identity, 1, 10);
    drop(guard);
    assert!(matches!(ledger.poll(ticket).unwrap(), GrantPoll::Pending));
    assert!(matches!(
        ledger.reserve(
            identity,
            RequestId(2),
            GrantKind::Controller(10),
            CancellationToken::new()
        ),
        Err(CustodyError::Conflict)
    ));
    let lease = Lease {
        controller: 10,
        epoch: 7,
    };
    granted(&mut ledger, identity, 1, lease);
    assert_eq!(ledger.retirements().len(), 1);
    ledger.prepare_retirement(ticket, RequestId(3)).unwrap();
    // Inject queue Full: do not call retirement_sent. The same command remains owned.
    assert_eq!(ledger.retirements()[0].2, Some((RequestId(3), false)));
    ledger.reap_closed();
    assert!(matches!(ledger.poll(ticket).unwrap(), GrantPoll::Retiring));
    // A still-valid server holder may atomically take an unsent cleanup offer.
    ledger
        .adopt(ticket, ActualGrant::Controller(lease))
        .unwrap();
    assert_eq!(
        ledger.retirement_sent(ticket, RequestId(3)),
        Err(CustodyError::Stale)
    );
}
#[test]
fn recipient_cancellation_before_adoption_is_retained_without_queue_notification() {
    let identity = identity();
    let cancel = CancellationToken::new();
    let mut ledger = GrantCustody::default();
    let (ticket, _guard) = ledger
        .reserve(
            identity,
            RequestId(1),
            GrantKind::Controller(8),
            cancel.clone(),
        )
        .unwrap();
    ledger.sent(ticket).unwrap();
    let lease = Lease {
        controller: 8,
        epoch: 1,
    };
    granted(&mut ledger, identity, 1, lease);
    cancel.cancel();
    assert!(ledger.recipient_cancelled(ticket).unwrap());
    assert_eq!(
        ledger.adopt(ticket, ActualGrant::Controller(lease)),
        Err(CustodyError::Cancelled)
    );
    assert_eq!(ledger.retirements().len(), 1);
}
#[test]
fn release_sent_blocks_adoption_and_only_exact_completion_retires() {
    let identity = identity();
    let mut ledger = GrantCustody::default();
    let (ticket, guard) = claim(&mut ledger, identity, 1, 9);
    drop(guard);
    let lease = Lease {
        controller: 9,
        epoch: 4,
    };
    granted(&mut ledger, identity, 1, lease);
    ledger.prepare_retirement(ticket, RequestId(2)).unwrap();
    ledger.retirement_sent(ticket, RequestId(2)).unwrap();
    assert_eq!(
        ledger.adopt(ticket, ActualGrant::Controller(lease)),
        Err(CustodyError::ReleaseSent)
    );
    assert!(!ledger
        .record_reply(identity, RequestId(3), &Ok(Response::Applied))
        .unwrap());
    assert!(matches!(ledger.poll(ticket).unwrap(), GrantPoll::Retiring));
    ledger
        .record_reply(identity, RequestId(2), &Ok(Response::Applied))
        .unwrap();
    assert!(matches!(ledger.poll(ticket).unwrap(), GrantPoll::Retired));
}
#[test]
fn matching_no_grant_rejection_preserves_other_owner_and_observer() {
    let identity = identity();
    let mut ledger = GrantCustody::default();
    let (owner, _guard) = claim(&mut ledger, identity, 1, 11);
    let lease = Lease {
        controller: 11,
        epoch: 5,
    };
    granted(&mut ledger, identity, 1, lease);
    let (rejected, _rejected_guard) = claim(&mut ledger, identity, 2, 12);
    ledger
        .record_reply(
            identity,
            RequestId(2),
            &Err(RuntimeError {
                code: ErrorCode::ControlRequired,
                message: "injected no-grant result".into(),
            }),
        )
        .unwrap();
    assert!(matches!(
        ledger.poll(rejected).unwrap(),
        GrantPoll::Rejected(error) if error.code==ErrorCode::ControlRequired
    ));
    assert!(
        matches!(ledger.poll(owner).unwrap(),GrantPoll::Granted(ActualGrant::Controller(actual)) if actual==lease)
    );
    let (observer, _observer_guard) = ledger
        .reserve(
            identity,
            RequestId(3),
            GrantKind::Observer(20),
            CancellationToken::new(),
        )
        .unwrap();
    ledger.sent(observer).unwrap();
    let actual = ObserverLease {
        viewer: 20,
        epoch: 6,
    };
    ledger
        .record_reply(identity, RequestId(3), &Ok(Response::Observed(actual)))
        .unwrap();
    let (failed, _failed_guard) = ledger
        .reserve(
            identity,
            RequestId(4),
            GrantKind::Observer(21),
            CancellationToken::new(),
        )
        .unwrap();
    ledger.sent(failed).unwrap();
    ledger
        .record_reply(
            identity,
            RequestId(4),
            &Err(RuntimeError {
                code: ErrorCode::ResourceLimit,
                message: "injected no-grant result".into(),
            }),
        )
        .unwrap();
    assert!(
        matches!(ledger.poll(observer).unwrap(),GrantPoll::Granted(ActualGrant::Observer(value)) if value==actual)
    );
    assert!(matches!(
        ledger.poll(failed).unwrap(),
        GrantPoll::Rejected(error) if error.code==ErrorCode::ResourceLimit
    ));
    ledger.acknowledge_rejection(failed).unwrap();
    assert!(ledger.poll(failed).is_err());
}
#[test]
fn stale_generation_revocation_and_malformed_reply_do_not_replace_exact_receipt() {
    let identity = identity();
    let mut ledger = GrantCustody::default();
    let (ticket, _guard) = claim(&mut ledger, identity, 1, 1);
    let lease = Lease {
        controller: 1,
        epoch: 2,
    };
    granted(&mut ledger, identity, 1, lease);
    assert_eq!(
        ledger.record_reply(
            identity,
            RequestId(1),
            &Ok(Response::Claimed(Lease {
                controller: 1,
                epoch: 99
            }))
        ),
        Err(CustodyError::WrongReply)
    );
    let other = Identity {
        generation: Generation::new(),
        ..identity
    };
    ledger.record_revocation(
        other,
        &Event::ControllerRevoked {
            lease,
            reason: ControllerRevocation::Replaced,
        },
    );
    ledger.destroy_source(other);
    assert!(
        matches!(ledger.poll(ticket).unwrap(),GrantPoll::Granted(ActualGrant::Controller(value)) if value==lease)
    );
    ledger.record_revocation(
        identity,
        &Event::ControllerRevoked {
            lease,
            reason: ControllerRevocation::Released,
        },
    );
    assert!(matches!(ledger.poll(ticket).unwrap(), GrantPoll::Retired));
}
#[test]
fn bounded_reservation_and_not_sent_cannot_erase_a_sent_request() {
    let identity = identity();
    let mut ledger = GrantCustody::default();
    let mut guards = Vec::new();
    let mut sources = Vec::new();
    for index in 1..=128 {
        let source = Identity {
            generation: Generation::new(),
            ..identity
        };
        sources.push(source);
        let (ticket, guard) = claim(&mut ledger, source, index, index);
        guards.push(guard);
        assert_eq!(ledger.not_sent(ticket), Err(CustodyError::Conflict));
    }
    assert!(matches!(
        ledger.reserve(
            identity,
            RequestId(129),
            GrantKind::Observer(900),
            CancellationToken::new()
        ),
        Err(CustodyError::Full)
    ));
    drop(guards);
    ledger.reap_closed();
    assert!(matches!(
        ledger.reserve(
            identity,
            RequestId(129),
            GrantKind::Observer(900),
            CancellationToken::new()
        ),
        Err(CustodyError::Full)
    ));
    for source in sources {
        ledger.destroy_source(source);
    }
    let (ticket, mut guard) = ledger
        .reserve(
            identity,
            RequestId(130),
            GrantKind::Observer(900),
            CancellationToken::new(),
        )
        .unwrap();
    ledger.not_sent(ticket).unwrap();
    guard.disarm();
    assert!(ledger.poll(ticket).is_err());
}
#[test]
fn adopted_receipt_cannot_be_retired_by_later_cancel_or_stale_drop() {
    let identity = identity();
    let mut ledger = GrantCustody::default();
    let cancel = CancellationToken::new();
    let (ticket, guard) = ledger
        .reserve(
            identity,
            RequestId(1),
            GrantKind::Observer(4),
            cancel.clone(),
        )
        .unwrap();
    ledger.sent(ticket).unwrap();
    let actual = ObserverLease {
        viewer: 4,
        epoch: 5,
    };
    ledger
        .record_reply(identity, RequestId(1), &Ok(Response::Observed(actual)))
        .unwrap();
    ledger.adopt(ticket, ActualGrant::Observer(actual)).unwrap();
    cancel.cancel();
    drop(guard);
    assert!(ledger.retirements().is_empty());
    assert!(ledger.poll(ticket).is_err());
    ledger
        .retire_owned(identity, RequestId(2), ActualGrant::Observer(actual))
        .unwrap();
    assert_eq!(ledger.retirements().len(), 1);
    assert_eq!(ticket.request(), RequestId(1));
    assert_eq!(ticket.identity(), identity);
}
#[test]
fn cancel_watch_follows_the_recipient_and_ends_when_the_entry_leaves() {
    let mut ledger = GrantCustody::default();
    let recipient = CancellationToken::new();
    let (ticket, mut guard) = ledger
        .reserve(
            identity(),
            RequestId(1),
            GrantKind::Controller(7),
            recipient.clone(),
        )
        .unwrap();
    let (cancel, removed) = ledger.cancel_watch(ticket).unwrap();
    recipient.cancel();
    assert!(cancel.is_cancelled());
    assert!(!removed.is_cancelled());
    ledger.not_sent(ticket).unwrap();
    guard.disarm();
    assert!(removed.is_cancelled());
    assert!(ledger.cancel_watch(ticket).is_none());
}
