#[path = "support/external.rs"]
mod common;
use common::*;
use shellfox_runtime::*;
use std::time::{Duration, Instant};

#[test]
fn external_source_has_no_control_input_or_inspection_and_only_local_layout_resize() {
    let (runtime, mut source) = source();
    assert_eq!(
        request(
            &runtime,
            1,
            Command::Claim {
                controller: 1,
                takeover: true
            }
        )
        .unwrap_err()
        .code,
        ErrorCode::Unsupported
    );
    let forged = Lease {
        controller: 1,
        epoch: 1,
    };
    for (id, command) in [
        (
            2,
            Command::RawInput {
                lease: forged,
                bytes: b"must not write".to_vec(),
            },
        ),
        (
            3,
            Command::Resize {
                lease: forged,
                cols: 100,
                rows: 30,
            },
        ),
        (
            4,
            Command::Paste {
                lease: forged,
                text: "must not paste".into(),
            },
        ),
    ] {
        assert!(request(&runtime, id, command).is_err());
    }
    assert_eq!(
        runtime.request_inspection(),
        Err(InspectionFailure::Unsupported)
    );
    let observer = observe(&runtime, 5, 10, None);
    feed(
        &mut source,
        1,
        ExternalStream::Stdout,
        b"view only\x1b[6n\x1b]52;c;Zm9yYmlkZGVu\x07\x1b]2;not-live\x07\x07".to_vec(),
    );
    wait(&runtime, || {
        runtime.external_status().unwrap().source_sequence == SourceSequence(1)
    });
    assert!(
        runtime.metadata().is_none(),
        "passive source effects must not publish live bells/title/CWD"
    );
    assert_eq!(runtime.live_output_status(), LiveOutputStatus::Disabled);
    assert!(text(&frame(&runtime, 6, observer)).contains("view only"));
    request(
        &runtime,
        7,
        Command::Presentation {
            view: PresentationLease::Observer(observer),
            action: PresentationAction::ResizeExternal {
                cols: 100,
                rows: 30,
            },
        },
    )
    .unwrap();
    let resized = frame(&runtime, 8, observer);
    assert_eq!(
        (resized.screen.screen.cols, resized.screen.screen.rows),
        (100, 30)
    );
    assert_eq!(runtime.phase(), Phase::Active);
    assert_eq!(
        request(&runtime, 9, Command::RetryClose).unwrap_err().code,
        ErrorCode::Unsupported
    );
    assert_eq!(
        runtime.phase(),
        Phase::Active,
        "generic close cannot retire a run's parser before its source finishes"
    );
    finish(&mut source, 1);
    wait(&runtime, || runtime.phase() == Phase::Closed);
    assert!(runtime.external_status().unwrap().finished);
}
#[test]
fn source_sequences_remain_distinct_from_segments_and_final_bytes_precede_view_exit() {
    let (runtime, mut source) = source();
    let observer = observe(&runtime, 1, 11, Some(0));
    let mut full = vec![b'a'; EXTERNAL_CHUNK_BYTES];
    full[..4].copy_from_slice(b"HEAD");
    feed(&mut source, 1, ExternalStream::Stdout, full);
    feed(&mut source, 2, ExternalStream::Stderr, vec![0xf0, 0x9f]);
    feed(
        &mut source,
        3,
        ExternalStream::Stdout,
        vec![0x91, 0xa9, b'!'],
    );
    finish(&mut source, 3);
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut delivered = 0;
    loop {
        assert!(Instant::now() < deadline);
        match runtime.try_recv() {
            Ok(Event::ObserverOutput {
                observer: got,
                chunk,
            }) => {
                assert_eq!(got, observer);
                assert_eq!(chunk.sequence, delivered + 1);
                delivered = chunk.sequence;
            }
            Ok(Event::ObserverExited {
                observer: got,
                last_sequence,
            }) => {
                assert_eq!(got, observer);
                assert_eq!(last_sequence, delivered);
                break;
            }
            Ok(Event::Phase {
                error: Some(error), ..
            }) => panic!("external view failed: {error}"),
            _ => std::thread::sleep(Duration::from_millis(1)),
        }
    }
    let status = runtime.external_status().unwrap();
    assert_eq!(status.source_sequence, SourceSequence(3));
    assert_eq!(
        status.delivery,
        Some(DeliveryRange {
            first: DeliverySequence(10),
            last: DeliverySequence(10)
        })
    );
    assert_eq!(status.last_delivery_sequence, DeliverySequence(10));
    assert_eq!(delivered, 10);
    assert!(status.finished && !status.closed && !status.failed);
    assert!(text(&frame(&runtime, 2, observer)).contains("👩!"));
}
#[test]
fn late_show_and_view_detach_never_reprime_a_parser_from_the_lost_raw_tail() {
    let (runtime, mut source) = source();
    feed(
        &mut source,
        1,
        ExternalStream::Stdout,
        b"\x1b[2J\x1b[HPERSISTENT_HEADER\x1b[2;1Hstateful".to_vec(),
    );
    for sequence in 2..=42 {
        feed(
            &mut source,
            sequence,
            ExternalStream::Stderr,
            b"\x1b[0m".repeat(EXTERNAL_CHUNK_BYTES / 4),
        );
    }
    wait(&runtime, || {
        runtime.external_status().unwrap().source_sequence == SourceSequence(42)
    });
    // Over 1 MiB was parsed before the first Show/observer. The bounded raw ring
    // no longer contains HEADER, but the one parser still owns its full state.
    let first = observe(&runtime, 1, 12, None);
    assert!(text(&frame(&runtime, 2, first)).contains("PERSISTENT_HEADER"));
    request(&runtime, 3, Command::Unobserve { observer: first }).unwrap();
    feed(
        &mut source,
        43,
        ExternalStream::Stdout,
        b"\x1b[3;1HAFTER_DETACH".to_vec(),
    );
    wait(&runtime, || {
        runtime.external_status().unwrap().source_sequence == SourceSequence(43)
    });
    assert_eq!(runtime.phase(), Phase::Active);
    let second = observe(&runtime, 4, 12, None);
    let shown = text(&frame(&runtime, 5, second));
    assert!(shown.contains("PERSISTENT_HEADER") && shown.contains("AFTER_DETACH"));
    finish(&mut source, 43);
    wait(&runtime, || runtime.phase() == Phase::Closed);
}
#[test]
fn dropped_source_drains_accepted_bytes_but_never_reports_successful_finish() {
    let (runtime, mut source) = source();
    feed(
        &mut source,
        1,
        ExternalStream::Stdout,
        b"ACCEPTED_BEFORE_DISCONNECT".to_vec(),
    );
    drop(source);
    let deadline = Instant::now() + Duration::from_secs(10);
    while runtime.phase() != Phase::Closed {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    let status = runtime.external_status().unwrap();
    assert!(status.failed && !status.closed && !status.finished);
    assert_eq!(status.source_sequence, SourceSequence(1));
    let observer = observe(&runtime, 1, 20, None);
    assert!(text(&frame(&runtime, 2, observer)).contains("ACCEPTED_BEFORE_DISCONNECT"));
}
#[test]
fn failed_source_validation_never_advances_or_silently_remaps_sequence() {
    let (runtime, mut source) = source();
    let chunk = ExternalChunk {
        sequence: SourceSequence(2),
        stream: ExternalStream::Stderr,
        bytes: b"exact".to_vec(),
    };
    let pointer = chunk.bytes.as_ptr();
    let returned = match source.try_feed(chunk) {
        Err(ExternalFeedError::Invalid { chunk, .. }) => chunk,
        _ => panic!("gap must fail"),
    };
    assert_eq!(returned.bytes.as_ptr(), pointer);
    assert_eq!(returned.sequence, SourceSequence(2));
    feed(&mut source, 1, ExternalStream::Stdout, b"valid".to_vec());
    assert!(matches!(
        source.try_finish(SourceSequence(2)),
        Err(ExternalFinishError::Invalid(_))
    ));
    finish(&mut source, 1);
    wait(&runtime, || runtime.phase() == Phase::Closed);
    assert_eq!(
        runtime.external_status().unwrap().source_sequence,
        SourceSequence(1)
    );
}
