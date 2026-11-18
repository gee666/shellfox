#[path = "support/external.rs"]
mod common;
use common::*;
use shellfox_runtime::*;
use std::time::{Duration, Instant};
#[test]
fn legacy_policy_uses_separate_transport_decoders_while_raw_default_keeps_raw_contract() {
    let (raw, mut raw_source) = source();
    feed(&mut raw_source, 1, ExternalStream::Stdout, vec![0xf0, 0x9f]);
    feed(&mut raw_source, 2, ExternalStream::Stderr, vec![0x91, 0xa9]);
    finish(&mut raw_source, 2);
    wait(&raw, || raw.external_status().unwrap().finished);
    let observer = observe(&raw, 1, 30, None);
    assert!(text(&frame(&raw, 2, observer)).contains("👩"));
    let (legacy, mut legacy_source) = source_with_policy(ExternalDisplayPolicy::LegacyAutomationV1);
    feed(
        &mut legacy_source,
        1,
        ExternalStream::Stdout,
        vec![0xf0, 0x9f],
    );
    wait(&legacy, || {
        legacy.external_status().unwrap().source_sequence == SourceSequence(1)
    });
    let pending = legacy.external_status().unwrap();
    assert_eq!(pending.delivery, None);
    assert_eq!(pending.last_delivery_sequence, DeliverySequence(0));
    feed(
        &mut legacy_source,
        2,
        ExternalStream::Stderr,
        vec![0x91, 0xa9],
    );
    wait(&legacy, || {
        legacy.external_status().unwrap().source_sequence == SourceSequence(2)
    });
    let emitted = legacy.external_status().unwrap();
    assert_eq!(
        emitted.delivery,
        Some(DeliveryRange {
            first: DeliverySequence(1),
            last: DeliverySequence(1)
        })
    );
    finish(&mut legacy_source, 2);
    wait(&legacy, || legacy.external_status().unwrap().finished);
    let closed = legacy.external_status().unwrap();
    assert_eq!(
        closed.source_sequence,
        SourceSequence(2),
        "final flush is not a new raw chunk"
    );
    assert_eq!(
        closed.finish_delivery,
        Some(DeliveryRange {
            first: DeliverySequence(2),
            last: DeliverySequence(2)
        })
    );
    assert_eq!(closed.last_delivery_sequence, DeliverySequence(2));
    assert!(
        !closed.closed,
        "finished source retains its canonical parser for Show"
    );
    let observer = observe(&legacy, 1, 31, None);
    let visible = text(&frame(&legacy, 2, observer));
    assert!(visible.contains("���") && !visible.contains("👩"));
}
#[test]
fn finish_flush_is_observable_before_explicit_parser_retirement_and_late_show_commands_stay_live() {
    let (mut runtime, mut source) = source_with_policy(ExternalDisplayPolicy::LegacyAutomationV1);
    let mut initial = String::from("FIRST\r\nSECOND\r\nTHIRD\r\n");
    for row in 0..40 {
        initial.push_str(&format!("row{row:02}\r\n"));
    }
    initial.push_str("FINAL\x1b[6n");
    feed(&mut source, 1, ExternalStream::Stdout, initial.into_bytes());
    feed(&mut source, 2, ExternalStream::Stdout, vec![0xf0, 0x9f]);
    feed(&mut source, 3, ExternalStream::Stderr, vec![0xe2, 0x82]);
    // Caller has now observed both descriptor EOFs. One Finish flushes stdout
    // then stderr, regardless of which OS EOF notification arrived first.
    finish(&mut source, 3);
    wait(&runtime, || runtime.external_status().unwrap().finished);
    let status = runtime.external_status().unwrap();
    assert!(!status.closed && !status.failed);
    assert_eq!(status.source_sequence, SourceSequence(3));
    assert_eq!(status.delivery, None);
    assert!(status.finish_delivery.is_some());
    let observer = observe(&runtime, 1, 32, None);
    assert!(text(&frame(&runtime, 2, observer)).contains("FINAL��"));
    let recovery = request(
        &runtime,
        3,
        Command::ObserverRecover {
            observer,
            capability: AnsiRecoveryCapability::FormattedActiveBufferV1,
        },
    )
    .unwrap();
    assert!(
        matches!(recovery, Response::Ansi { bytes, .. } if String::from_utf8_lossy(&bytes).contains("FIRST"))
    );
    request(
        &runtime,
        4,
        Command::Presentation {
            view: PresentationLease::Observer(observer),
            action: PresentationAction::ResizeExternal {
                cols: 100,
                rows: 30,
            },
        },
    )
    .unwrap();
    request(
        &runtime,
        5,
        Command::Presentation {
            view: PresentationLease::Observer(observer),
            action: PresentationAction::Scroll { delta: isize::MIN },
        },
    )
    .unwrap();
    assert!(text(&frame(&runtime, 6, observer)).contains("FIRST"));
    assert!(!runtime.external_status().unwrap().closed);
    // There is deliberately no raw process in this test. In Service, independent
    // raw-owner cleanup proof must precede this explicit parser retirement.
    runtime.request_external_close().unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !runtime.poll_external_closed().unwrap() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(runtime.external_status().unwrap().closed);
    assert!(matches!(
        runtime.try_send(Request {
            id: RequestId(7),
            command: Command::Observe {
                viewer: 33,
                after: None
            }
        }),
        Err(SubmitError::Disconnected(_))
    ));
}
