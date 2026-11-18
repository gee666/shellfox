#[path = "support/external.rs"]
mod common;
use common::*;
use shellfox_runtime::*;
use std::time::{Duration, Instant};
fn snapshot(runtime: &Runtime, id: u64, observer: ObserverLease) -> StampedRecovery {
    match request(
        runtime,
        id,
        Command::ObserverRecoverStamped {
            observer,
            capability: AnsiRecoveryCapability::VisibleScreen,
        },
    )
    .unwrap()
    {
        Response::Recovery(snapshot) => snapshot,
        _ => panic!("stamped recovery expected"),
    }
}
fn ack(
    runtime: &Runtime,
    id: u64,
    observer: ObserverLease,
    snapshot: &StampedRecovery,
) -> Result<Response, RuntimeError> {
    request(
        runtime,
        id,
        Command::ObserverLayoutAck {
            observer,
            recovery_id: snapshot.recovery_id,
            last_delivery_sequence: snapshot.last_delivery_sequence,
            layout_revision: snapshot.layout_revision,
        },
    )
}
fn output(runtime: &Runtime, observer: ObserverLease, sequence: u64) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        assert!(Instant::now() < deadline);
        match runtime.try_recv() {
            Ok(Event::ObserverOutput {
                observer: actual,
                chunk,
            }) => {
                assert_eq!(actual, observer);
                assert_eq!(chunk.sequence, sequence);
                return;
            }
            _ => std::thread::sleep(Duration::from_millis(1)),
        }
    }
}
#[test]
fn stamped_recovery_replacement_ids_and_real_geometry_barrier_gate_all_new_bytes() {
    let (runtime, mut source) = source();
    feed(&mut source, 1, ExternalStream::Stdout, b"before".to_vec());
    wait(&runtime, || {
        runtime.external_status().unwrap().source_sequence == SourceSequence(1)
    });
    let observer = observe(&runtime, 1, 61, None);
    let old = snapshot(&runtime, 2, observer);
    let latest = snapshot(&runtime, 3, observer);
    assert_eq!(
        (
            latest.cols,
            latest.rows,
            latest.layout_revision,
            latest.last_delivery_sequence
        ),
        (80, 24, 1, 1)
    );
    assert_ne!(old.recovery_id, latest.recovery_id);
    assert_eq!(old.last_delivery_sequence, latest.last_delivery_sequence);
    feed(
        &mut source,
        2,
        ExternalStream::Stderr,
        b"\r\nafter".to_vec(),
    );
    wait(&runtime, || {
        runtime.external_status().unwrap().source_sequence == SourceSequence(2)
    });
    assert_eq!(
        ack(&runtime, 4, observer, &old).unwrap_err().code,
        ErrorCode::Validation
    );
    assert_eq!(
        request(
            &runtime,
            5,
            Command::ObserverAck {
                observer,
                sequence: 1
            }
        )
        .unwrap_err()
        .code,
        ErrorCode::Validation
    );
    assert!(!std::iter::from_fn(|| runtime.try_recv().ok())
        .any(|event| matches!(event, Event::ObserverOutput { .. })));
    ack(&runtime, 6, observer, &latest).unwrap();
    output(&runtime, observer, 2);
    request(
        &runtime,
        7,
        Command::ObserverAck {
            observer,
            sequence: 2,
        },
    )
    .unwrap();
    request(
        &runtime,
        8,
        Command::Presentation {
            view: PresentationLease::Observer(observer),
            action: PresentationAction::ResizeExternal {
                cols: 100,
                rows: 30,
            },
        },
    )
    .unwrap();
    feed(
        &mut source,
        3,
        ExternalStream::Stdout,
        b"\r\ngeometry two".to_vec(),
    );
    wait(&runtime, || {
        runtime.external_status().unwrap().source_sequence == SourceSequence(3)
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    let notification = loop {
        assert!(Instant::now() < deadline);
        match runtime.try_recv() {
            Ok(Event::ObserverLayout {
                observer: actual,
                layout,
            }) => {
                assert_eq!(actual, observer);
                break layout;
            }
            Ok(Event::ObserverOutput { .. }) => {
                panic!("post-layout bytes escaped before full recovery")
            }
            _ => std::thread::sleep(Duration::from_millis(1)),
        }
    };
    assert_eq!(
        notification,
        LayoutStamp {
            cols: 100,
            rows: 30,
            layout_revision: 2,
            last_delivery_sequence: 2
        }
    );
    assert_eq!(
        request(
            &runtime,
            9,
            Command::ObserverLayoutAck {
                observer,
                recovery_id: latest.recovery_id,
                last_delivery_sequence: notification.last_delivery_sequence,
                layout_revision: notification.layout_revision
            }
        )
        .unwrap_err()
        .code,
        ErrorCode::Validation
    );
    let recovered = snapshot(&runtime, 10, observer);
    assert_eq!(
        (
            recovered.cols,
            recovered.rows,
            recovered.layout_revision,
            recovered.last_delivery_sequence
        ),
        (100, 30, 2, 3)
    );
    ack(&runtime, 11, observer, &recovered).unwrap();
    feed(&mut source, 4, ExternalStream::Stdout, b"\r\nlast".to_vec());
    wait(&runtime, || {
        runtime.external_status().unwrap().source_sequence == SourceSequence(4)
    });
    output(&runtime, observer, 4); // sequence 3 was consumed by full recovery, not skipped raw bytes
    request(
        &runtime,
        12,
        Command::Presentation {
            view: PresentationLease::Observer(observer),
            action: PresentationAction::ResizeExternal {
                cols: 100,
                rows: 30,
            },
        },
    )
    .unwrap();
    let duplicate = snapshot(&runtime, 13, observer);
    assert_eq!(
        duplicate.layout_revision, 2,
        "same geometry is not a new layout revision"
    );
    ack(&runtime, 14, observer, &duplicate).unwrap();
    finish(&mut source, 4);
    wait(&runtime, || runtime.external_status().unwrap().finished);
    assert!(text(&frame(&runtime, 15, observer)).contains("geometry two"));
}
#[test]
fn layout_journal_gap_is_explicit_recovery_not_standalone_resize_acceptance() {
    let (runtime, mut source) = source();
    let observer = observe(&runtime, 1, 62, None);
    let initial = snapshot(&runtime, 2, observer);
    ack(&runtime, 3, observer, &initial).unwrap();
    // The observer sees at most one notification until it recovers. More than64
    // source geometry changes cannot create an unbounded per-view queue.
    for index in 0..70u16 {
        request(
            &runtime,
            10 + u64::from(index),
            Command::Presentation {
                view: PresentationLease::Observer(observer),
                action: PresentationAction::ResizeExternal {
                    cols: 81 + index,
                    rows: 24,
                },
            },
        )
        .unwrap();
    }
    feed(
        &mut source,
        1,
        ExternalStream::Stdout,
        b"latest dimensions".to_vec(),
    );
    wait(&runtime, || {
        runtime.external_status().unwrap().source_sequence == SourceSequence(1)
    });
    let latest = snapshot(&runtime, 100, observer);
    assert_eq!(
        (latest.cols, latest.rows, latest.layout_revision),
        (150, 24, 71)
    );
    assert!(ack(&runtime, 101, observer, &initial).is_err());
    ack(&runtime, 102, observer, &latest).unwrap();
    finish(&mut source, 1);
    wait(&runtime, || runtime.external_status().unwrap().finished);
}
