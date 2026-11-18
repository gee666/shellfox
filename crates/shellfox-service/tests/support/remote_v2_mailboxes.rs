use super::{delivery::ObserverDelivery, identity, mailboxes::ObserverMailboxes, recovery};
use shellfox_protocol::observer_v2 as wire;
use shellfox_runtime::*;

#[test]
fn one_drain_moves_only_the_exact_remote_observer_event_and_retains_uncertain_ack() {
    let id = identity();
    let observer = ObserverLease {
        viewer: 3,
        epoch: 1,
    };
    let mut entry = ObserverDelivery::new(id, observer).unwrap();
    let restoration = entry.recover(recovery(&entry)).unwrap();
    let reference = entry.reference().clone();
    entry
        .commit_ack(
            &reference,
            &wire::Ack::Recovery {
                recovery_id: restoration.recovery_id,
                sequence: 0,
            },
            &Response::Applied,
        )
        .unwrap();
    let mut mailboxes = ObserverMailboxes::default();
    mailboxes.insert_offer(entry).unwrap();
    // Codec-only ownership state, not Service/Native grant acceptance.
    mailboxes.prepare_adoption(id, observer).unwrap().commit();
    let native = ObserverLease {
        viewer: 4,
        epoch: 1,
    };
    assert!(matches!(
        mailboxes.record(
            id,
            Event::ObserverOutput {
                observer: native,
                chunk: Chunk {
                    sequence: 1,
                    bytes: b"native".to_vec()
                }
            }
        ),
        Some(Event::ObserverOutput { .. })
    ));
    assert!(mailboxes
        .record(
            id,
            Event::ObserverOutput {
                observer,
                chunk: Chunk {
                    sequence: 1,
                    bytes: b"remote".to_vec()
                }
            }
        )
        .is_none());
    let current = mailboxes.get_mut(id, observer).unwrap();
    let canceled_response = current.poll(1024, 1).unwrap();
    drop(canceled_response);
    assert!(matches!(
        &current.poll(1024, 1).unwrap()[..],
        [wire::Event::Data { sequence: 1, .. }]
    ));
    let ack = wire::Ack::Data { sequence: 1 };
    current.ack_command(&reference, &ack).unwrap();
    // The Runtime may apply this ACK while its Reply is lost. Until a matching
    // Reply is confirmed, owner retention remains; no fabricated ACK releases it.
    assert!(matches!(
        &current.poll(1024, 1).unwrap()[..],
        [wire::Event::Data { sequence: 1, .. }]
    ));
    current
        .commit_ack(&reference, &ack, &Response::Applied)
        .unwrap();
    assert!(current.poll(1024, 1).unwrap().is_empty());
    assert!(mailboxes.discard_unadopted(id, native).is_err());
    assert!(
        mailboxes.discard_unadopted(id, observer).is_err(),
        "adopted lease custody cannot be generically discarded"
    );
    mailboxes.source_destroyed(id);
    assert!(mailboxes.get_mut(id, observer).is_none());
}
