use super::*;
use shellfox_core::{Generation, SessionId, TabId};
use shellfox_runtime::{ExternalConfig, Identity, Runtime, SourceSequence};

fn identity() -> Identity {
    Identity {
        session_id: SessionId::new(),
        tab_id: TabId::new(),
        generation: Generation::new(),
    }
}
#[test]
fn authoritative_runtime_keeps_pre_tail_state_and_finishes_after_every_raw_chunk() {
    let (source, publisher) = AutomationView::new();
    let mut runtime = Runtime::start_external(identity(), ExternalConfig::default()).unwrap();
    let mut relay = AutomationRelay::new(source, &mut runtime).unwrap();
    assert!(runtime.take_source().is_none());
    let mut initial = b"PRIMARY_HISTORY\r\n".to_vec();
    for _ in 0..40 {
        initial.extend_from_slice(b"old primary row\r\n");
    }
    initial.extend_from_slice(b"PRIMARY_ANCHOR\x1b[?1049h");
    publisher
        .try_push(AutomationViewEvent::Data {
            sequence: 1,
            stream: AutomationStream::Stdout,
            bytes: initial,
        })
        .unwrap();
    for sequence in 2..=10 {
        publisher
            .try_push(AutomationViewEvent::Data {
                sequence,
                stream: AutomationStream::Stdout,
                bytes: vec![b'x'; 32768],
            })
            .unwrap();
    }
    publisher
        .try_push(AutomationViewEvent::Data {
            sequence: 11,
            stream: AutomationStream::Stderr,
            bytes: b"\x1b[?1049l".to_vec(),
        })
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        relay.pump().unwrap();
        while runtime.try_recv().is_ok() {}
        if runtime.external_status().unwrap().source_sequence == SourceSequence(11) {
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    use shellfox_runtime::{Command, PresentationAction, PresentationLease, Response};
    let Response::Observed(observer) = request(
        &runtime,
        1,
        Command::Observe {
            viewer: 1,
            after: None,
        },
    ) else {
        panic!("Read-only observer expected");
    };
    let Response::Presentation(frame) = request(
        &runtime,
        2,
        Command::Presentation {
            view: PresentationLease::Observer(observer),
            action: PresentationAction::Frame { focused: false },
        },
    ) else {
        panic!("Authoritative frame expected");
    };
    let text: String = frame
        .screen
        .screen
        .cells
        .iter()
        .map(|cell| cell.text.as_str())
        .collect();
    assert!(
        text.contains("PRIMARY_ANCHOR"),
        "Show must attach the parser from run start, not parse a truncated tail"
    );
    publisher
        .try_push(AutomationViewEvent::Eof {
            after_sequence: 11,
            stream: AutomationStream::Stdout,
        })
        .unwrap();
    publisher
        .try_push(AutomationViewEvent::Eof {
            after_sequence: 11,
            stream: AutomationStream::Stderr,
        })
        .unwrap();
    publisher
        .try_push(AutomationViewEvent::Exit {
            after_sequence: 11,
            code: 7,
        })
        .unwrap();
    while !relay.finished(&runtime) {
        relay.pump().unwrap();
        while runtime.try_recv().is_ok() {}
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(
        runtime.external_status().unwrap().source_sequence,
        SourceSequence(11)
    );
    assert!(!runtime.external_status().unwrap().failed);
    assert!(
        !runtime.external_status().unwrap().closed,
        "Finish must retain the canonical parser for late Show"
    );
    let Response::Observed(late) = request(
        &runtime,
        3,
        Command::Observe {
            viewer: 2,
            after: None,
        },
    ) else {
        panic!("Late Show observer expected");
    };
    let Response::Ansi {
        bytes,
        last_sequence,
        ..
    } = request(
        &runtime,
        4,
        Command::ObserverRecover {
            observer: late,
            capability: shellfox_runtime::AnsiRecoveryCapability::VisibleScreen,
        },
    )
    else {
        panic!("Late Show recovery expected");
    };
    assert!(visible_ansi_text(&bytes).contains("PRIMARY_ANCHOR"));
    let cursor = frame.screen.screen.cursor;
    let cursor_tail = format!(
        "\x1b[0m\x1b[{};{}H\x1b[?25{}",
        cursor.y + 1,
        cursor.x + 1,
        if cursor.visible { 'h' } else { 'l' }
    );
    assert!(bytes.ends_with(cursor_tail.as_bytes()));
    assert!(matches!(
        request(
            &runtime,
            5,
            Command::ObserverAck {
                observer: late,
                sequence: last_sequence
            }
        ),
        Response::Applied
    ));
    // VisibleScreen is only an active-cell/cursor projection. Scrollback comes
    // from this retained canonical parser, not from that limited ANSI recovery.
    assert!(matches!(
        request(
            &runtime,
            6,
            Command::Presentation {
                view: PresentationLease::Observer(late),
                action: PresentationAction::Scroll { delta: isize::MIN },
            }
        ),
        Response::Applied
    ));
    let Response::Presentation(history) = request(
        &runtime,
        7,
        Command::Presentation {
            view: PresentationLease::Observer(late),
            action: PresentationAction::Frame { focused: false },
        },
    ) else {
        panic!("Canonical history expected");
    };
    let history: String = history
        .screen
        .screen
        .cells
        .iter()
        .map(|cell| cell.text.as_str())
        .collect();
    assert!(
        history.contains("PRIMARY_HISTORY"),
        "Late Show lost primary scrollback"
    );
    runtime.request_external_close().unwrap();
    while !runtime.poll_external_closed().unwrap() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(
        !relay.finished(&Runtime::start_external(identity(), ExternalConfig::default()).unwrap())
    );
}
// Strip only CSI formatting emitted by the known VisibleScreen encoder.
// This is not a second terminal parser or an authoritative screen.
fn visible_ansi_text(bytes: &[u8]) -> String {
    let mut input = bytes.iter().copied();
    let mut plain = Vec::new();
    while let Some(byte) = input.next() {
        if byte == 0x1b {
            assert_eq!(input.next(), Some(b'['));
            for byte in input.by_ref() {
                if (0x40..=0x7e).contains(&byte) {
                    break;
                }
            }
        } else {
            plain.push(byte);
        }
    }
    String::from_utf8(plain).unwrap()
}
pub(super) fn request(
    runtime: &Runtime,
    id: u64,
    command: shellfox_runtime::Command,
) -> shellfox_runtime::Response {
    use shellfox_runtime::{Event, Request, RequestId};
    runtime
        .try_send(Request {
            id: RequestId(id),
            command,
        })
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Ok(Event::Reply { id: actual, result }) = runtime.try_recv() {
            if actual == RequestId(id) {
                return result.unwrap();
            }
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
}
#[test]
fn ready_parser_abort_is_actual_channel_failure_not_finish_or_raw_cleanup() {
    use crate::automation::AutomationSourceAbortReason;
    for reason in [
        AutomationSourceAbortReason::StartupCancelled,
        AutomationSourceAbortReason::RawSpawnFailed,
    ] {
        let (source, _publisher) = AutomationView::new();
        let mut runtime = Runtime::start_external(identity(), ExternalConfig::default()).unwrap();
        let mut relay = AutomationRelay::new(source, &mut runtime).unwrap();
        assert!(
            runtime.request_external_close().is_err(),
            "Healthy unfinished source Close guard must stay intact"
        );
        let failure = relay.abort_source(reason).unwrap();
        assert_eq!(failure.identity(), Some(runtime.identity()));
        assert_eq!(failure.last_queued().0, 0);
        assert_eq!(failure.pending_bytes(), 0);
        assert!(relay.failure() == Some(failure));
        let intent = relay.abort_intent().unwrap();
        assert_eq!(intent.identity, runtime.identity());
        assert_eq!(intent.last_queued.0, 0);
        assert_eq!(runtime.external_status().unwrap().abort, Some(intent));
        let deadline = Instant::now() + Duration::from_secs(10);
        while !runtime.external_status().unwrap().failed {
            while runtime.try_recv().is_ok() {}
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        let status = runtime.external_status().unwrap();
        assert!(!status.finished);
        assert!(status.finish_delivery.is_none());
        assert!(!relay.finished(&runtime));
        assert!(relay.pump().is_err());
        runtime.request_external_close().unwrap();
        while !runtime.poll_external_closed().unwrap() {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}
#[test]
fn premature_exit_is_rejected_and_retained_not_view_cleanup_proof() {
    let (source, publisher) = AutomationView::new();
    let mut runtime = Runtime::start_external(identity(), ExternalConfig::default()).unwrap();
    let mut relay = AutomationRelay::new(source, &mut runtime).unwrap();
    publisher
        .try_push(AutomationViewEvent::Exit {
            after_sequence: 0,
            code: 0,
        })
        .unwrap();
    assert!(relay.pump().is_err());
    assert!(relay.pump().is_err());
    assert!(!relay.finished(&runtime));
    assert!(!runtime.external_status().unwrap().finished);
}
