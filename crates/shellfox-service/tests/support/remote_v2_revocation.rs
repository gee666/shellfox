//! Codec-only transition checks; injected replies are not live cleanup proof.
use super::{
    delivery::{DeliveryFailure, ObserverDelivery},
    identity,
    mailboxes::ObserverMailboxes,
    recovery,
};
use shellfox_protocol::observer_v2 as wire;
use shellfox_runtime::*;
#[test]
fn revocation_purges_queued_bytes_and_recovery_tokens_before_unobserve_admission_or_reply() {
    let id = identity();
    let observer = ObserverLease {
        viewer: 1,
        epoch: 1,
    };
    let mut entry = ObserverDelivery::new(id, observer).unwrap();
    let snapshot = entry.recover(recovery(&entry)).unwrap();
    let reference = entry.reference().clone();
    let ack = wire::Ack::Recovery {
        recovery_id: snapshot.recovery_id,
        sequence: snapshot.last_sequence,
    };
    entry
        .commit_ack(&reference, &ack, &Response::Applied)
        .unwrap();
    entry.record(
        id,
        Event::ObserverOutput {
            observer,
            chunk: Chunk {
                sequence: 1,
                bytes: b"queued before revoke".to_vec(),
            },
        },
    );
    let mut books = ObserverMailboxes::default();
    books.insert_offer(entry).unwrap();
    books.prepare_adoption(id, observer).unwrap().commit();
    books.revoke_delivery(id, observer);
    books.revoke_source(id);
    assert_eq!(
        books
            .get_mut(id, observer)
            .unwrap()
            .poll(1024, 10)
            .unwrap_err(),
        DeliveryFailure::Stale
    );
    assert!(books
        .get_mut(id, observer)
        .unwrap()
        .ack_command(&reference, &ack)
        .is_err());
    assert_eq!(books.retirement_candidates(), vec![(id, observer)]);
    books.retirement_sent(id, observer, RequestId(99));
    assert!(books.retirement_candidates().is_empty());
    assert!(books
        .record(
            id,
            Event::ObserverOutput {
                observer,
                chunk: Chunk {
                    sequence: 2,
                    bytes: b"late already queued output".to_vec()
                }
            }
        )
        .is_none());
    books.record(
        id,
        Event::Reply {
            id: RequestId(99),
            result: Err(RuntimeError {
                code: ErrorCode::Unavailable,
                message: "retained failure".into(),
            }),
        },
    );
    assert_eq!(books.retirement_candidates(), vec![(id, observer)]);
    assert!(books.get_mut(id, observer).unwrap().is_revoked());
    books.retirement_sent(id, observer, RequestId(100));
    books.record(
        id,
        Event::Reply {
            id: RequestId(100),
            result: Ok(Response::Applied),
        },
    );
    assert!(books.get_mut(id, observer).is_none());
    assert!(books.retirement_candidates().is_empty());
    books
        .insert_offer(ObserverDelivery::new(id, observer).unwrap())
        .unwrap();
    books.revoke_delivery(id, observer);
    assert!(
        books.retirement_candidates().is_empty(),
        "unadopted offer remains central custody, not local retirement"
    );
    assert!(books.prepare_adoption(id, observer).is_none());
    books.discard_unadopted(id, observer).unwrap();
    assert!(books.get_mut(id, observer).is_none());
}
