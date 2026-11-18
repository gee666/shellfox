#[path = "support/remote_v2_recovery_continuation.rs"]
mod continuation_tests;
#[path = "../src/remote_v2_delivery.rs"]
mod delivery;
#[path = "../src/mailbox_custody.rs"]
mod mailbox_custody;
#[path = "support/remote_v2_mailboxes.rs"]
mod mailbox_tests;
#[path = "../src/remote_v2_mailboxes.rs"]
mod mailboxes;
#[path = "support/remote_v2_recovery_limits.rs"]
mod recovery_limit_tests;
#[path = "support/remote_v2_revocation.rs"]
mod revocation_tests;
#[path = "support/remote_v2_delivery_end.rs"]
mod source_end_tests;
#[path = "support/remote_v2_stamped.rs"]
mod stamped_tests;
#[path = "support/remote_v2_stamp.rs"]
mod stamps;
use delivery::{DeliveryFailure, ObserverDelivery};
use shellfox_core::{Generation, SessionId, TabId};
use shellfox_protocol::{observer_v2 as wire, Validate};
use shellfox_runtime::*;
use stamps::recovery;
use std::time::{Duration, Instant};

fn identity() -> Identity {
    Identity {
        session_id: SessionId::new(),
        tab_id: TabId::new(),
        generation: Generation::new(),
    }
}
fn apply_recovery(view: &mut ObserverDelivery) {
    let recovery = view.recover(recovery(view)).unwrap();
    let ack = wire::Ack::Recovery {
        recovery_id: recovery.recovery_id,
        sequence: 0,
    };
    let reference = view.reference().clone();
    view.ack_command(&reference, &ack).unwrap();
    view.commit_ack(&reference, &ack, &Response::Applied)
        .unwrap();
}
#[test]
fn bounded_outbox_checks_offered_ack_and_preserves_unmatched_events_without_control() {
    let id = identity();
    let observer = ObserverLease {
        viewer: 7,
        epoch: 1,
    };
    let mut view = ObserverDelivery::new(id, observer).unwrap();
    assert_eq!(view.identity(), id);
    assert_eq!(view.observer(), observer);
    apply_recovery(&mut view);
    let data = vec![0x80; wire::MAX_DATA_BYTES];
    for sequence in 1..=8 {
        assert!(view
            .record(
                id,
                Event::ObserverOutput {
                    observer,
                    chunk: Chunk {
                        sequence,
                        bytes: data.clone()
                    }
                }
            )
            .is_none());
    }
    let reference = view.reference().clone();
    assert!(view
        .ack_command(&reference, &wire::Ack::Data { sequence: 1 })
        .is_err());
    let events = view.poll(wire::MAX_DATA_BYTES, 1024).unwrap();
    assert_eq!(events.len(), 1);
    events[0].validate().unwrap();
    let wire::Event::Data { sequence, data, .. } = &events[0] else {
        panic!("data")
    };
    assert_eq!(*sequence, 1);
    assert_eq!(
        wire::decode_bytes(data, wire::MAX_DATA_BYTES).unwrap(),
        vec![0x80; wire::MAX_DATA_BYTES]
    );
    let mut forged = reference.clone();
    forged.delivery_id = wire::Id::new();
    assert_eq!(
        view.ack_command(&forged, &wire::Ack::Data { sequence: 1 })
            .unwrap_err(),
        DeliveryFailure::Stale
    );
    assert!(view
        .ack_command(&reference, &wire::Ack::Data { sequence: 2 })
        .is_err());
    let ack = wire::Ack::Data { sequence: 1 };
    assert!(matches!(
        view.ack_command(&reference, &ack).unwrap(),
        Command::ObserverAck { .. }
    ));
    assert!(view
        .commit_ack(
            &reference,
            &ack,
            &Response::Input(InputReceipt {
                accepted_bytes: 0,
                kind: InputKind::Raw
            })
        )
        .is_err());
    view.commit_ack(&reference, &ack, &Response::Applied)
        .unwrap();
    // The ninth chunk fits only because one real successful ACK released credit.
    view.record(
        id,
        Event::ObserverOutput {
            observer,
            chunk: Chunk {
                sequence: 9,
                bytes: vec![0; wire::MAX_DATA_BYTES],
            },
        },
    );
    assert!(!view.poll(0, 0).unwrap().iter().any(|_| true));
    view.record(
        id,
        Event::ObserverOutput {
            observer,
            chunk: Chunk {
                sequence: 10,
                bytes: vec![0],
            },
        },
    );
    assert_eq!(
        view.poll(1, 1).unwrap_err(),
        DeliveryFailure::RecoveryRequired
    );
    assert!(matches!(
        view.record(id, Event::Started(Ok(()))),
        Some(Event::Started(Ok(())))
    ));
}
#[test]
fn recovery_is_explicit_bounded_and_epoch_fenced() {
    let mut view = ObserverDelivery::new(
        identity(),
        ObserverLease {
            viewer: 1,
            epoch: 1,
        },
    )
    .unwrap();
    assert!(view
        .recover(Response::Ansi {
            capability: AnsiRecoveryCapability::VisibleScreen,
            bytes: b"visible".to_vec(),
            last_sequence: 0
        })
        .is_err());
    let restoration = view.recover(recovery(&view)).unwrap();
    let reference = view.reference().clone();
    assert!(view
        .ack_command(
            &reference,
            &wire::Ack::Recovery {
                recovery_id: wire::Id::new(),
                sequence: 0
            }
        )
        .is_err());
    assert!(view
        .ack_command(&reference, &wire::Ack::Data { sequence: 1 })
        .is_err());
    assert!(view.poll(100, 10).unwrap().is_empty());
    let ack = wire::Ack::Recovery {
        recovery_id: restoration.recovery_id,
        sequence: 0,
    };
    // Service may drain a post-ACK chunk before it returns the ACK Reply. It must
    // retain that chunk, but never offer it until the actual Reply is committed.
    let id = view.identity();
    let observer = view.observer();
    view.record(
        id,
        Event::ObserverOutput {
            observer,
            chunk: Chunk {
                sequence: 1,
                bytes: b"x".to_vec(),
            },
        },
    );
    assert!(view.poll(100, 10).unwrap().is_empty());
    view.commit_ack(&reference, &ack, &Response::Applied)
        .unwrap();
    assert_eq!(view.poll(100, 10).unwrap().len(), 1);
}
fn request(
    runtime: &Runtime,
    number: u64,
    command: Command,
    view: &mut Option<ObserverDelivery>,
) -> Result<Response, RuntimeError> {
    runtime
        .try_send(Request {
            id: RequestId(number),
            command,
        })
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        assert!(Instant::now() < deadline);
        match runtime.try_recv() {
            Ok(Event::Reply { id, result }) if id == RequestId(number) => return result,
            Ok(event) => {
                if let Some(view) = view {
                    view.record(runtime.identity(), event);
                }
            }
            Err(_) => std::thread::sleep(Duration::from_millis(1)),
        }
    }
}
#[test]
fn actual_external_runtime_observer_recovery_ack_data_and_source_end_need_no_controller() {
    let mut runtime = Runtime::start_external(identity(), ExternalConfig::default()).unwrap();
    let mut source = runtime.take_source().unwrap();
    let mut view = None;
    let Response::Observed(observer) = request(
        &runtime,
        1,
        Command::Observe {
            viewer: 77,
            after: None,
        },
        &mut view,
    )
    .unwrap() else {
        panic!("observer")
    };
    view = Some(ObserverDelivery::new(runtime.identity(), observer).unwrap());
    assert!(request(
        &runtime,
        2,
        Command::Claim {
            controller: 77,
            takeover: true
        },
        &mut view
    )
    .is_err());
    let response = request(
        &runtime,
        3,
        Command::ObserverRecoverStamped {
            observer,
            capability: AnsiRecoveryCapability::FormattedActiveBufferV1,
        },
        &mut view,
    )
    .unwrap();
    let restoration = view.as_mut().unwrap().recover(response).unwrap();
    restoration.validate().unwrap();
    let reference = view.as_ref().unwrap().reference().clone();
    let ack = wire::Ack::Recovery {
        recovery_id: restoration.recovery_id,
        sequence: restoration.last_sequence,
    };
    let command = view
        .as_ref()
        .unwrap()
        .ack_command(&reference, &ack)
        .unwrap();
    let response = request(&runtime, 4, command, &mut view).unwrap();
    view.as_mut()
        .unwrap()
        .commit_ack(&reference, &ack, &response)
        .unwrap();
    source
        .try_feed(ExternalChunk {
            sequence: SourceSequence(1),
            stream: ExternalStream::Stdout,
            bytes: b"live\x1b[31m!".to_vec(),
        })
        .unwrap();
    source.try_finish(SourceSequence(1)).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let events = loop {
        assert!(Instant::now() < deadline);
        while let Ok(event) = runtime.try_recv() {
            view.as_mut().unwrap().record(runtime.identity(), event);
        }
        let events = view
            .as_mut()
            .unwrap()
            .poll(wire::MAX_POLL_DATA_BYTES, 1024)
            .unwrap();
        if events
            .iter()
            .any(|event| matches!(event, wire::Event::SourceEnded { .. }))
        {
            break events;
        }
        std::thread::sleep(Duration::from_millis(1));
    };
    assert!(matches!(&events[0], wire::Event::Data { .. }));
    assert!(matches!(
        events.last(),
        Some(wire::Event::SourceEnded { .. })
    ));
    assert!(!events
        .iter()
        .any(|event| matches!(event, wire::Event::ProcessExited { .. })));
    assert!(runtime.external_status().unwrap().finished);
    assert!(
        !runtime.external_status().unwrap().closed,
        "source end did not close the retained parser or prove any raw cleanup"
    );
    let response = request(&runtime, 5, Command::Unobserve { observer }, &mut view).unwrap();
    assert!(matches!(response, Response::Applied));
}
