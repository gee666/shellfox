#[path = "support/external.rs"]
mod common;
use common::*;
use shellfox_ghostty_vt::{Cell, Terminal};
use shellfox_runtime::*;
use std::time::{Duration, Instant};
fn normalized(mut cells: Vec<Cell>) -> Vec<Cell> {
    for cell in &mut cells {
        if cell.text.is_empty() && cell.width != 0 {
            cell.text.push(' ');
        }
    }
    cells
}
#[test]
fn late_visible_screen_recovery_semantically_matches_active_primary_and_cursor_not_history() {
    let (runtime, mut source) = source();
    let mut initial = b"PRIMARY_HISTORY\r\n".to_vec();
    for _ in 0..40 {
        initial.extend_from_slice(b"old primary row\r\n");
    }
    initial.extend_from_slice(b"PRIMARY_ANCHOR\x1b[?1049h");
    feed(&mut source, 1, ExternalStream::Stdout, initial);
    for sequence in 2..=10 {
        feed(
            &mut source,
            sequence,
            ExternalStream::Stdout,
            vec![b'x'; EXTERNAL_CHUNK_BYTES],
        );
    }
    feed(
        &mut source,
        11,
        ExternalStream::Stderr,
        b"\x1b[?1049l".to_vec(),
    );
    finish(&mut source, 11);
    wait(&runtime, || runtime.external_status().unwrap().finished);
    let observer = observe(&runtime, 1, 41, None);
    let active = frame(&runtime, 2, observer);
    assert!(text(&active).contains("PRIMARY_ANCHOR"));
    assert!(
        !text(&active).contains("PRIMARY_HISTORY"),
        "history is not the active grid"
    );
    let (bytes, barrier) = match request(
        &runtime,
        3,
        Command::ObserverRecover {
            observer,
            capability: AnsiRecoveryCapability::VisibleScreen,
        },
    )
    .unwrap()
    {
        Response::Ansi {
            bytes,
            last_sequence,
            capability: AnsiRecoveryCapability::VisibleScreen,
        } => (bytes, last_sequence),
        _ => panic!("visible projection expected"),
    };
    // The encoder deliberately inserts SGR between cells. A raw ANSI substring
    // is not proof of a semantic screen; decode on a fresh PASSIVE test mirror.
    let mut client = Terminal::new(active.screen.screen.cols, active.screen.screen.rows).unwrap();
    client.feed(&bytes).unwrap();
    assert!(client.drain_replies().unwrap().is_empty());
    let restored = client.render().unwrap();
    assert_eq!(
        normalized(restored.cells),
        normalized(active.screen.screen.cells.clone())
    );
    assert_eq!(restored.cursor, active.screen.screen.cursor);
    request(
        &runtime,
        4,
        Command::ObserverAck {
            observer,
            sequence: barrier,
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
    assert!(text(&frame(&runtime, 6, observer)).contains("PRIMARY_HISTORY"));
}
#[test]
fn observer_admission_and_exact_recovery_ack_gate_live_bytes_without_weakening_credit() {
    let (runtime, mut source) = source();
    let observer = observe(&runtime, 1, 42, None);
    feed(&mut source, 1, ExternalStream::Stdout, b"before".to_vec());
    wait(&runtime, || {
        runtime.external_status().unwrap().source_sequence == SourceSequence(1)
    });
    assert!(
        !std::iter::from_fn(|| runtime.try_recv().ok())
            .any(|event| matches!(event, Event::ObserverOutput { .. })),
        "None is not byte-subscription admission"
    );
    let barrier = match request(
        &runtime,
        2,
        Command::ObserverRecover {
            observer,
            capability: AnsiRecoveryCapability::VisibleScreen,
        },
    )
    .unwrap()
    {
        Response::Ansi { last_sequence, .. } => last_sequence,
        _ => panic!("barrier expected"),
    };
    feed(
        &mut source,
        2,
        ExternalStream::Stderr,
        b"\r\nafter".to_vec(),
    );
    wait(&runtime, || {
        runtime.external_status().unwrap().source_sequence == SourceSequence(2)
    });
    let current = frame(&runtime, 3, observer);
    assert!(current.sequence > barrier);
    assert_eq!(
        request(
            &runtime,
            4,
            Command::ObserverAck {
                observer,
                sequence: current.sequence
            }
        )
        .unwrap_err()
        .code,
        ErrorCode::Validation
    );
    assert!(
        !std::iter::from_fn(|| runtime.try_recv().ok())
            .any(|event| matches!(event, Event::ObserverOutput { .. })),
        "recovery barrier remains pending"
    );
    request(
        &runtime,
        5,
        Command::ObserverAck {
            observer,
            sequence: barrier,
        },
    )
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        assert!(Instant::now() < deadline);
        match runtime.try_recv() {
            Ok(Event::ObserverOutput {
                observer: actual,
                chunk,
            }) => {
                assert_eq!(actual, observer);
                assert_eq!(chunk.sequence, current.sequence);
                assert_eq!(chunk.bytes, b"\r\nafter");
                break;
            }
            _ => std::thread::sleep(Duration::from_millis(1)),
        }
    }
    finish(&mut source, 2);
    wait(&runtime, || runtime.external_status().unwrap().finished);
}
