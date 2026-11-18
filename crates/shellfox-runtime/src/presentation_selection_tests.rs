use super::*;
use crate::{Lease, ObserverLease};
use shellfox_core::{Generation, SessionId, TabId};
use shellfox_ghostty_vt::{Selection, ViewportPoint};
fn id() -> Identity {
    Identity {
        session_id: SessionId::new(),
        tab_id: TabId::new(),
        generation: Generation::new(),
    }
}
fn point(x: u16, y: u16) -> ViewportPoint {
    ViewportPoint { x, y }
}
fn history(x: u16, row: u64) -> HistoryPoint {
    HistoryPoint { x, row }
}
fn action(
    bank: &mut Presentations,
    terminal: &mut Terminal,
    view: PresentationLease,
    action: PresentationAction,
) -> PresentationResult {
    bank.action(terminal, id(), 42, GridLimits::default(), view, action)
        .unwrap()
}
fn select(
    bank: &mut Presentations,
    terminal: &mut Terminal,
    view: PresentationLease,
    selection: Selection,
) {
    assert!(matches!(
        action(
            bank,
            terminal,
            view,
            PresentationAction::Selection {
                selection: Some(selection)
            }
        ),
        PresentationResult::Applied
    ));
}
fn frame(
    bank: &mut Presentations,
    terminal: &mut Terminal,
    view: PresentationLease,
    focused: bool,
) -> Arc<ViewFrame> {
    match action(bank, terminal, view, PresentationAction::Frame { focused }) {
        PresentationResult::Frame(frame) => frame,
        _ => panic!("frame expected"),
    }
}
#[test]
fn reverse_and_rectangle_frames_carry_real_anchor_focus_not_selected_mask_inference() {
    let mut terminal = Terminal::new(8, 3).unwrap();
    terminal.feed(b"abcdefgh\r\nijklmnop\r\nqrstuvwx").unwrap();
    let mut bank = Presentations::new();
    let reverse = PresentationLease::Observer(ObserverLease {
        viewer: 1,
        epoch: 1,
    });
    let forward = PresentationLease::Observer(ObserverLease {
        viewer: 2,
        epoch: 2,
    });
    select(
        &mut bank,
        &mut terminal,
        reverse,
        Selection::Range {
            start: point(6, 1),
            end: point(1, 0),
            rectangle: false,
        },
    );
    select(
        &mut bank,
        &mut terminal,
        forward,
        Selection::Range {
            start: point(1, 0),
            end: point(6, 1),
            rectangle: false,
        },
    );
    let one = frame(&mut bank, &mut terminal, reverse, true);
    let two = frame(&mut bank, &mut terminal, forward, true);
    let endpoints = one.selection.unwrap();
    assert_eq!(endpoints.anchor, history(6, 1));
    assert_eq!(endpoints.focus, history(1, 0));
    assert_eq!(endpoints.kind, PresentationSelectionKind::Range);
    assert_eq!(endpoints.buffer, PresentationBuffer::Primary);
    assert_eq!(two.selection.unwrap().anchor, endpoints.focus);
    assert_eq!(two.selection.unwrap().focus, endpoints.anchor);
    assert_eq!(
        one.screen
            .screen
            .cells
            .iter()
            .map(|cell| cell.selected)
            .collect::<Vec<_>>(),
        two.screen
            .screen
            .cells
            .iter()
            .map(|cell| cell.selected)
            .collect::<Vec<_>>()
    );
    for (start, end) in [
        (point(6, 0), point(1, 2)),
        (point(1, 2), point(6, 0)),
        (point(6, 2), point(1, 0)),
    ] {
        select(
            &mut bank,
            &mut terminal,
            reverse,
            Selection::Range {
                start,
                end,
                rectangle: true,
            },
        );
        let selected = frame(&mut bank, &mut terminal, reverse, true)
            .selection
            .unwrap();
        assert_eq!(selected.anchor, history(start.x, u64::from(start.y)));
        assert_eq!(selected.focus, history(end.x, u64::from(end.y)));
        assert!(selected.rectangle);
    }
}
#[test]
fn offscreen_endpoints_remain_absolute_and_focus_cache_preserves_the_query() {
    let mut terminal = Terminal::new(8, 3).unwrap();
    terminal
        .feed(b"zero\r\none\r\ntwo\r\nthree\r\nfour")
        .unwrap();
    let mut bank = Presentations::new();
    let view = PresentationLease::Controller(Lease {
        controller: 1,
        epoch: 1,
    });
    action(
        &mut bank,
        &mut terminal,
        view,
        PresentationAction::Scroll { delta: isize::MIN },
    );
    select(
        &mut bank,
        &mut terminal,
        view,
        Selection::Range {
            start: point(1, 0),
            end: point(2, 1),
            rectangle: false,
        },
    );
    let selected = frame(&mut bank, &mut terminal, view, true);
    action(
        &mut bank,
        &mut terminal,
        view,
        PresentationAction::FollowTail,
    );
    let tail = frame(&mut bank, &mut terminal, view, true);
    let endpoints = tail.selection.unwrap();
    assert_eq!(endpoints.anchor, selected.selection.unwrap().anchor);
    assert_eq!(endpoints.focus, selected.selection.unwrap().focus);
    assert_eq!(endpoints.view_top_row, tail.top_row);
    assert_eq!(endpoints.total_rows, tail.total_rows);
    assert!(endpoints.focus.row < tail.top_row);
    assert!(tail.screen.screen.cells.iter().all(|cell| !cell.selected));
    let blurred = frame(&mut bank, &mut terminal, view, false);
    assert_eq!(blurred.selection, tail.selection);
    assert!(Arc::ptr_eq(&blurred.screen, &tail.screen));
    assert_ne!(tail.sequence, endpoints.anchor.row);
}
#[test]
fn reverse_endpoints_follow_resize_and_buffers_report_their_own_context() {
    let mut terminal = Terminal::new(12, 3).unwrap();
    terminal.feed(b"abcdefghijkl\r\nnext\r\nlast").unwrap();
    let mut bank = Presentations::new();
    let view = PresentationLease::Observer(ObserverLease {
        viewer: 1,
        epoch: 1,
    });
    select(
        &mut bank,
        &mut terminal,
        view,
        Selection::Range {
            start: point(10, 0),
            end: point(2, 0),
            rectangle: false,
        },
    );
    terminal.resize(6, 4).unwrap();
    bank.source_changed().unwrap();
    let resized = frame(&mut bank, &mut terminal, view, true)
        .selection
        .unwrap();
    assert_eq!(resized.anchor, history(4, 1));
    assert_eq!(resized.focus, history(2, 0));
    terminal.feed(b"\x1b[?1049h\x1b[Halt").unwrap();
    bank.source_changed().unwrap();
    assert!(frame(&mut bank, &mut terminal, view, true)
        .selection
        .is_none());
    select(
        &mut bank,
        &mut terminal,
        view,
        Selection::Range {
            start: point(2, 0),
            end: point(0, 0),
            rectangle: true,
        },
    );
    let alternate = frame(&mut bank, &mut terminal, view, true)
        .selection
        .unwrap();
    assert_eq!(alternate.buffer, PresentationBuffer::Alternate);
    assert!(alternate.rectangle);
    terminal.feed(b"\x1b[?1049l").unwrap();
    bank.source_changed().unwrap();
    assert_eq!(
        frame(&mut bank, &mut terminal, view, true).selection,
        Some(resized)
    );
}
#[test]
fn word_line_all_use_documented_forward_normalization_and_pruned_endpoints_are_absent() {
    let mut terminal = Terminal::new(8, 4).unwrap();
    terminal
        .feed(b"one two\r\nwrappedline012345\r\nlast")
        .unwrap();
    let mut bank = Presentations::new();
    let view = PresentationLease::Observer(ObserverLease {
        viewer: 1,
        epoch: 1,
    });
    action(
        &mut bank,
        &mut terminal,
        view,
        PresentationAction::Scroll { delta: isize::MIN },
    );
    for (selection, kind) in [
        (
            Selection::Word(point(5, 0)),
            PresentationSelectionKind::Word,
        ),
        (
            Selection::Line(point(1, 1)),
            PresentationSelectionKind::Line,
        ),
        (Selection::All, PresentationSelectionKind::All),
    ] {
        select(&mut bank, &mut terminal, view, selection);
        let selected = frame(&mut bank, &mut terminal, view, true)
            .selection
            .unwrap();
        assert_eq!(selected.kind, kind);
        assert!(!selected.rectangle);
        assert!((selected.anchor.row, selected.anchor.x) <= (selected.focus.row, selected.focus.x));
    }
    select(
        &mut bank,
        &mut terminal,
        view,
        Selection::Range {
            start: point(5, 0),
            end: point(1, 1),
            rectangle: false,
        },
    );
    assert!(frame(&mut bank, &mut terminal, view, true)
        .selection
        .is_some());
    terminal.feed(&b"x\r\n".repeat(300_000)).unwrap();
    bank.source_changed().unwrap();
    let pruned = frame(&mut bank, &mut terminal, view, true);
    assert!(pruned.selection.is_none());
    assert!(pruned.screen.screen.cells.iter().all(|cell| !cell.selected));
    assert_eq!(frame(&mut bank, &mut terminal, view, false).selection, None);
}
