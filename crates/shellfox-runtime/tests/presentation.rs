#![cfg(windows)]
mod support;
use shellfox_ghostty_vt::{Key, KeyAction, KeyEvent, Selection, ViewportPoint};
use shellfox_runtime::*;
use std::sync::Arc;
use support::Harness;
fn observed(host: &mut Harness, viewer: u64) -> ObserverLease {
    match host
        .request(Command::Observe {
            viewer,
            after: None,
        })
        .unwrap()
    {
        Response::Observed(observer) => observer,
        _ => panic!("observer expected"),
    }
}
fn present(host: &mut Harness, view: PresentationLease, action: PresentationAction) -> Response {
    host.request(Command::Presentation { view, action })
        .unwrap()
}
fn frame(host: &mut Harness, view: PresentationLease, focused: bool) -> Arc<ViewFrame> {
    match present(host, view, PresentationAction::Frame { focused }) {
        Response::Presentation(frame) => frame,
        _ => panic!("view frame expected"),
    }
}
fn text(frame: &ViewFrame, row: usize) -> String {
    frame
        .screen
        .screen
        .cells
        .chunks(usize::from(frame.screen.screen.cols))
        .nth(row)
        .unwrap()
        .iter()
        .map(|cell| cell.text.as_str())
        .collect::<String>()
        .trim_end()
        .to_owned()
}
#[test]
fn two_readonly_presentations_have_independent_history_selection_copy_and_repaint_versions() {
    let mut host = Harness::start("--child-presentations");
    host.until(|h| String::from_utf8_lossy(&h.output).contains("PRESENTATION_READY"));
    let controller = PresentationLease::Controller(host.lease);
    let first_lease = observed(&mut host, 10);
    let second_lease = observed(&mut host, 20);
    let first = PresentationLease::Observer(first_lease);
    let second = PresentationLease::Observer(second_lease);
    present(
        &mut host,
        first,
        PresentationAction::Scroll { delta: isize::MIN },
    );
    let at_start = frame(&mut host, first, true);
    let at_tail = frame(&mut host, second, true);
    assert_eq!(text(&at_start, 0), "row00");
    assert!(at_start.top_row < at_tail.top_row);
    // A newer screen is not delivered raw-byte credit for this frame-only view.
    assert_eq!(
        host.request(Command::ObserverAck {
            observer: first_lease,
            sequence: at_start.sequence
        })
        .unwrap_err()
        .code,
        ErrorCode::Validation
    );
    let surface_error = host
        .request(Command::Presentation {
            view: first,
            action: PresentationAction::Hyperlink {
                point: ViewportPoint {
                    x: u16::MAX,
                    y: u16::MAX,
                },
            },
        })
        .unwrap_err();
    assert_eq!(surface_error.code, ErrorCode::Validation);
    assert_eq!(host.runtime.phase(), Phase::Active);
    assert!(!at_start.cursor_visual.visible && at_tail.cursor_visual.visible);
    present(
        &mut host,
        first,
        PresentationAction::Selection {
            selection: Some(Selection::Word(ViewportPoint { x: 1, y: 0 })),
        },
    );
    assert!(
        matches!(present(&mut host, first, PresentationAction::Copy), Response::Text(Some(text)) if text == "row00")
    );
    let selected = frame(&mut host, first, true);
    assert_eq!(selected.sequence, at_start.sequence);
    assert!(selected.version > at_start.version);
    let endpoints = selected.selection.expect("tracked word endpoints");
    assert_eq!(endpoints.kind, PresentationSelectionKind::Word);
    assert_eq!(endpoints.buffer, PresentationBuffer::Primary);
    assert_eq!(endpoints.anchor, HistoryPoint { x: 0, row: 0 });
    assert_eq!(endpoints.focus, HistoryPoint { x: 4, row: 0 });
    assert_eq!(endpoints.view_top_row, selected.top_row);
    assert_eq!(endpoints.total_rows, selected.total_rows);
    assert!(selected
        .screen
        .screen
        .cells
        .iter()
        .any(|cell| cell.selected));
    let untouched = frame(&mut host, second, true);
    assert!(Arc::ptr_eq(&at_tail, &untouched));
    let blurred = frame(&mut host, second, false);
    assert_eq!(blurred.cursor_visual.shape, CursorShape::HollowBlock);
    assert!(!blurred.cursor_visual.blinking);
    assert!(Arc::ptr_eq(&blurred.screen, &at_tail.screen));
    // Existing controller commands now share this controller's private view.
    host.request(Command::Scroll {
        lease: host.lease,
        delta: isize::MIN,
    })
    .unwrap();
    let controlled_start = frame(&mut host, controller, true);
    assert_eq!(text(&controlled_start, 0), "row00");
    host.request(Command::Key {
        lease: host.lease,
        event: KeyEvent {
            key: Key::ArrowUp,
            action: KeyAction::Release,
            ..Default::default()
        },
    })
    .unwrap();
    assert_eq!(
        frame(&mut host, controller, true).top_row,
        controlled_start.top_row
    );
    host.request(Command::Paste {
        lease: host.lease,
        text: "typed".into(),
    })
    .unwrap();
    let controlled_tail = frame(&mut host, controller, true);
    assert_eq!(
        controlled_tail.top_row + u64::from(controlled_tail.screen.screen.rows),
        controlled_tail.total_rows
    );
    assert_eq!(frame(&mut host, first, true).top_row, at_start.top_row);
    assert!(
        matches!(present(&mut host, first, PresentationAction::Copy), Response::Text(Some(text)) if text == "row00")
    );
    host.request(Command::Key {
        lease: host.lease,
        event: KeyEvent {
            key: Key::Enter,
            ..Default::default()
        },
    })
    .unwrap();
    host.until(|h| h.exited);
    assert!(String::from_utf8_lossy(&host.output).contains("PRESENTATION_INPUT=typed"));
    host.request(Command::Unobserve {
        observer: first_lease,
    })
    .unwrap();
    let error = host
        .request(Command::Presentation {
            view: first,
            action: PresentationAction::Frame { focused: true },
        })
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::ControlRequired);
}
#[test]
fn volatile_metadata_is_out_of_band_and_presentation_reads_never_drain_bells() {
    let mut host = Harness::start("--child-presentations");
    host.until(|h| String::from_utf8_lossy(&h.output).contains("PRESENTATION_READY"));
    let before = host.runtime.metadata().expect("live metadata");
    assert_eq!(before.identity, host.runtime.identity());
    assert_eq!(before.title.as_deref(), Some("presentation-fixture"));
    assert_eq!(
        before.cwd_uri.as_deref(),
        Some("file://foreign.example/project")
    );
    assert!(before.bell_count >= 1);
    let observer = PresentationLease::Observer(observed(&mut host, 30));
    present(
        &mut host,
        observer,
        PresentationAction::Scroll { delta: isize::MIN },
    );
    frame(&mut host, observer, false);
    present(
        &mut host,
        observer,
        PresentationAction::Selection {
            selection: Some(Selection::All),
        },
    );
    present(&mut host, observer, PresentationAction::Copy);
    let after = host.runtime.metadata().unwrap();
    assert!(Arc::ptr_eq(&before, &after));
    host.request(Command::Close { lease: host.lease }).unwrap();
    host.until(|h| h.exited);
}
