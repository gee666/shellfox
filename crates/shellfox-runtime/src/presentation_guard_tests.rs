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
fn run(
    bank: &mut Presentations,
    terminal: &mut Terminal,
    view: PresentationLease,
    action: PresentationAction,
) -> Result<PresentationResult, RuntimeError> {
    bank.action(terminal, id(), 7, GridLimits::default(), view, action)
}
fn frame(
    bank: &mut Presentations,
    terminal: &mut Terminal,
    view: PresentationLease,
    focused: bool,
) -> Arc<ViewFrame> {
    match run(bank, terminal, view, PresentationAction::Frame { focused }).unwrap() {
        PresentationResult::Frame(frame) => frame,
        _ => panic!("frame expected"),
    }
}
fn range() -> Option<Selection> {
    Some(Selection::Range {
        start: ViewportPoint { x: 6, y: 1 },
        end: ViewportPoint { x: 1, y: 0 },
        rectangle: true,
    })
}
fn stale(result: Result<PresentationResult, RuntimeError>) {
    assert!(matches!(
        result,
        Err(RuntimeError {
            code: crate::ErrorCode::StalePresentation,
            ..
        })
    ));
}
#[test]
fn fresh_guard_applies_once_and_repeated_or_focus_stale_samples_never_mutate() {
    let mut terminal = Terminal::new(12, 3).unwrap();
    terminal.feed(b"abcdefgh\r\nijklmnop\r\nqrstuvwx").unwrap();
    let mut bank = Presentations::new();
    let view = PresentationLease::Observer(ObserverLease {
        viewer: 1,
        epoch: 1,
    });
    let initial = frame(&mut bank, &mut terminal, view, true);
    assert!(matches!(
        run(
            &mut bank,
            &mut terminal,
            view,
            PresentationAction::SelectionAtVersion {
                version: initial.version,
                selection: range()
            }
        )
        .unwrap(),
        PresentationResult::Applied
    ));
    stale(run(
        &mut bank,
        &mut terminal,
        view,
        PresentationAction::SelectionAtVersion {
            version: initial.version,
            selection: None,
        },
    ));
    let selected = frame(&mut bank, &mut terminal, view, true);
    assert!(selected.selection.unwrap().rectangle);
    let old = selected.selection;
    let blurred = frame(&mut bank, &mut terminal, view, false);
    assert!(blurred.version > selected.version);
    stale(run(
        &mut bank,
        &mut terminal,
        view,
        PresentationAction::SelectionAtVersion {
            version: selected.version,
            selection: None,
        },
    ));
    assert_eq!(frame(&mut bank, &mut terminal, view, false).selection, old);
    // Ordinary gestures retain their existing unguarded behavior.
    run(
        &mut bank,
        &mut terminal,
        view,
        PresentationAction::Selection { selection: None },
    )
    .unwrap();
    assert!(frame(&mut bank, &mut terminal, view, true)
        .selection
        .is_none());
}
#[test]
fn source_output_and_resize_invalidate_lazily_cached_versions_before_any_frame_poll() {
    let mut terminal = Terminal::new(12, 3).unwrap();
    terminal.feed(b"abcdefghijk\r\nrow two\r\nlast").unwrap();
    let mut bank = Presentations::new();
    let view = PresentationLease::Controller(Lease {
        controller: 1,
        epoch: 1,
    });
    let sampled = frame(&mut bank, &mut terminal, view, true);
    terminal.feed(b" changed").unwrap();
    bank.source_changed().unwrap();
    // The old cache still holds exactly the sampled version: source comparison,
    // not version equality alone, is what must reject this cross-queue race.
    assert_eq!(bank.entries[0].version, sampled.version);
    stale(run(
        &mut bank,
        &mut terminal,
        view,
        PresentationAction::SelectionAtVersion {
            version: sampled.version,
            selection: range(),
        },
    ));
    assert_eq!(bank.entries[0].version, sampled.version);
    let fresh = frame(&mut bank, &mut terminal, view, true);
    assert!(fresh.selection.is_none());
    terminal.resize(6, 4).unwrap();
    bank.source_changed().unwrap();
    stale(run(
        &mut bank,
        &mut terminal,
        view,
        PresentationAction::SelectionAtVersion {
            version: fresh.version,
            selection: None,
        },
    ));
    assert!(frame(&mut bank, &mut terminal, view, true)
        .selection
        .is_none());
}
#[test]
fn same_lease_scroll_invalidates_the_coordinate_frame_without_retargeting() {
    let mut terminal = Terminal::new(12, 3).unwrap();
    terminal
        .feed(b"first\r\nsecond\r\nthird\r\nfourth\r\nlast")
        .unwrap();
    let mut bank = Presentations::new();
    let view = PresentationLease::Observer(ObserverLease {
        viewer: 1,
        epoch: 1,
    });
    run(
        &mut bank,
        &mut terminal,
        view,
        PresentationAction::Selection {
            selection: Some(Selection::Word(ViewportPoint { x: 1, y: 0 })),
        },
    )
    .unwrap();
    let sampled = frame(&mut bank, &mut terminal, view, true);
    run(
        &mut bank,
        &mut terminal,
        view,
        PresentationAction::Scroll { delta: -1 },
    )
    .unwrap();
    stale(run(
        &mut bank,
        &mut terminal,
        view,
        PresentationAction::SelectionAtVersion {
            version: sampled.version,
            selection: range(),
        },
    ));
    let current = frame(&mut bank, &mut terminal, view, true);
    assert_ne!(current.top_row, sampled.top_row);
    assert_eq!(
        current.selection.unwrap().anchor,
        sampled.selection.unwrap().anchor
    );
    assert_eq!(
        current.selection.unwrap().kind,
        PresentationSelectionKind::Word
    );
}
#[test]
fn missing_samples_and_invalid_coordinates_never_allocate_or_clamp_a_selection() {
    let mut terminal = Terminal::new(12, 3).unwrap();
    let mut bank = Presentations::new();
    let view = PresentationLease::Observer(ObserverLease {
        viewer: 1,
        epoch: 1,
    });
    stale(run(
        &mut bank,
        &mut terminal,
        view,
        PresentationAction::SelectionAtVersion {
            version: 1,
            selection: range(),
        },
    ));
    assert!(bank.entries.is_empty());
    let sampled = frame(&mut bank, &mut terminal, view, true);
    let result = run(
        &mut bank,
        &mut terminal,
        view,
        PresentationAction::SelectionAtVersion {
            version: sampled.version,
            selection: Some(Selection::Word(ViewportPoint { x: 12, y: 0 })),
        },
    );
    assert!(matches!(
        result,
        Err(RuntimeError {
            code: crate::ErrorCode::Validation,
            ..
        })
    ));
    assert!(Arc::ptr_eq(
        &sampled,
        &frame(&mut bank, &mut terminal, view, true)
    ));
    assert!(sampled.selection.is_none());
}
