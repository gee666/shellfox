use super::delivery::ObserverDelivery;
use shellfox_core::{Generation, SessionId, TabId};
use shellfox_protocol::observer_v2 as wire;
use shellfox_runtime::*;

#[test]
fn source_end_never_overtakes_bytes_after_a_lost_poll_response() {
    let identity = Identity {
        session_id: SessionId::new(),
        tab_id: TabId::new(),
        generation: Generation::new(),
    };
    let observer = ObserverLease {
        viewer: 1,
        epoch: 1,
    };
    let mut view = ObserverDelivery::new(identity, observer).unwrap();
    let recovery = view.recover(super::recovery(&view)).unwrap();
    let reference = view.reference().clone();
    view.commit_ack(
        &reference,
        &wire::Ack::Recovery {
            recovery_id: recovery.recovery_id,
            sequence: 0,
        },
        &Response::Applied,
    )
    .unwrap();
    for sequence in 1..=2 {
        view.record(
            identity,
            Event::ObserverOutput {
                observer,
                chunk: Chunk {
                    sequence,
                    bytes: b"x".to_vec(),
                },
            },
        );
    }
    view.record(
        identity,
        Event::ObserverExited {
            observer,
            last_sequence: 2,
        },
    );
    // These bytes were offered, but the HTTP response can be lost before receipt.
    assert_eq!(view.poll(2, 3).unwrap().len(), 3);
    assert!(view.poll(0, 3).unwrap().is_empty());
    assert!(matches!(
        &view.poll(1, 3).unwrap()[..],
        [wire::Event::Data { sequence: 1, .. }]
    ));
    assert!(matches!(
        &view.poll(2, 3).unwrap()[..],
        [
            wire::Event::Data { sequence: 1, .. },
            wire::Event::Data { sequence: 2, .. },
            wire::Event::SourceEnded {
                last_sequence: 2,
                ..
            }
        ]
    ));
}
