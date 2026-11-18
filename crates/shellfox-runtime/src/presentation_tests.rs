use super::*;
use crate::{Lease, ObserverLease};
use shellfox_core::{Generation, SessionId, TabId};
use shellfox_ghostty_vt::{CursorShape, Key, KeyEvent, Selection, ViewportPoint};
fn identity() -> Identity {
    static IDENTITY: std::sync::OnceLock<Identity> = std::sync::OnceLock::new();
    *IDENTITY.get_or_init(|| Identity {
        session_id: SessionId::new(),
        tab_id: TabId::new(),
        generation: Generation::new(),
    })
}
fn frame(
    bank: &mut Presentations,
    terminal: &mut Terminal,
    view: PresentationLease,
    focused: bool,
) -> Arc<ViewFrame> {
    match bank
        .action(
            terminal,
            identity(),
            42,
            GridLimits::default(),
            view,
            PresentationAction::Frame { focused },
        )
        .unwrap()
    {
        PresentationResult::Frame(frame) => frame,
        _ => panic!("expected frame"),
    }
}
fn apply(
    bank: &mut Presentations,
    terminal: &mut Terminal,
    view: PresentationLease,
    action: PresentationAction,
) {
    assert!(matches!(
        bank.action(
            terminal,
            identity(),
            42,
            GridLimits::default(),
            view,
            action
        )
        .unwrap(),
        PresentationResult::Applied
    ));
}
fn copy(
    bank: &mut Presentations,
    terminal: &mut Terminal,
    view: PresentationLease,
) -> Option<String> {
    match bank
        .action(
            terminal,
            identity(),
            42,
            GridLimits::default(),
            view,
            PresentationAction::Copy,
        )
        .unwrap()
    {
        PresentationResult::Text(text) => text,
        _ => panic!("expected copied text"),
    }
}
fn rows(frame: &ViewFrame) -> Vec<String> {
    frame
        .screen
        .screen
        .cells
        .chunks(usize::from(frame.screen.screen.cols))
        .map(|row| {
            row.iter()
                .map(|cell| cell.text.as_str())
                .collect::<String>()
                .trim_end()
                .to_owned()
        })
        .collect()
}
#[test]
fn independent_observers_never_change_canonical_viewport_selection_modes_replies_or_continuation() {
    let mut terminal = Terminal::new(12, 3).unwrap();
    terminal
        .feed(b"first\r\nsecond\r\nthird\r\nfourth\r\nlast\x1b[?1h\x1b[?2004h\x1b[6n\x1b[38;")
        .unwrap();
    terminal.scroll(-1).unwrap();
    terminal
        .set_selection(Some(Selection::Word(ViewportPoint { x: 1, y: 0 })))
        .unwrap();
    let before = terminal.render().unwrap();
    let host_copy = terminal.copy_selection().unwrap();
    let continuation = terminal.continuation().unwrap();
    let formatted = terminal.format_vt(0xfff, 1024 * 1024).unwrap();
    let key = terminal
        .encode_key(&KeyEvent {
            key: Key::ArrowUp,
            ..Default::default()
        })
        .unwrap();
    let mut bank = Presentations::new();
    let one = PresentationLease::Observer(ObserverLease {
        viewer: 1,
        epoch: 1,
    });
    let two = PresentationLease::Observer(ObserverLease {
        viewer: 2,
        epoch: 2,
    });
    apply(
        &mut bank,
        &mut terminal,
        one,
        PresentationAction::Scroll { delta: isize::MIN },
    );
    apply(
        &mut bank,
        &mut terminal,
        one,
        PresentationAction::Selection {
            selection: Some(Selection::Word(ViewportPoint { x: 1, y: 0 })),
        },
    );
    apply(
        &mut bank,
        &mut terminal,
        two,
        PresentationAction::Selection {
            selection: Some(Selection::Line(ViewportPoint { x: 1, y: 1 })),
        },
    );
    assert_eq!(copy(&mut bank, &mut terminal, one), Some("first".into()));
    assert_eq!(copy(&mut bank, &mut terminal, two), Some("fourth".into()));
    let first = frame(&mut bank, &mut terminal, one, true);
    let second = frame(&mut bank, &mut terminal, two, true);
    assert_eq!(rows(&first)[0], "first");
    assert_eq!(rows(&second)[0], "third");
    assert!(!first.cursor_visual.visible);
    assert!(second.cursor_visual.visible);
    assert_eq!(terminal.render().unwrap().cells, before.cells);
    assert_eq!(terminal.render().unwrap().cursor, before.cursor);
    assert_eq!(terminal.copy_selection().unwrap(), host_copy);
    assert_eq!(terminal.continuation().unwrap(), continuation);
    assert_eq!(terminal.format_vt(0xfff, 1024 * 1024).unwrap(), formatted);
    assert_eq!(
        terminal
            .encode_key(&KeyEvent {
                key: Key::ArrowUp,
                ..Default::default()
            })
            .unwrap(),
        key
    );
    assert_eq!(terminal.drain_replies().unwrap(), b"\x1b[3;5R");
}
#[test]
fn repaint_versions_change_without_output_and_unchanged_frames_share_owned_cells() {
    let mut terminal = Terminal::new(8, 3).unwrap();
    terminal.feed(b"abcdefghij\r\nhard").unwrap();
    let mut bank = Presentations::new();
    let view = PresentationLease::Controller(Lease {
        controller: 1,
        epoch: 1,
    });
    let first = frame(&mut bank, &mut terminal, view, true);
    let same = frame(&mut bank, &mut terminal, view, true);
    assert!(Arc::ptr_eq(&first, &same));
    let inactive = frame(&mut bank, &mut terminal, view, false);
    assert_eq!(inactive.sequence, first.sequence);
    assert!(inactive.version > first.version);
    assert!(Arc::ptr_eq(&first.screen, &inactive.screen));
    assert_eq!(inactive.cursor_visual.shape, CursorShape::HollowBlock);
    assert!(!inactive.cursor_visual.blinking);
    let active = frame(&mut bank, &mut terminal, view, true);
    assert_ne!(active.cursor_visual.shape, CursorShape::HollowBlock);
    assert!(active.row_metadata[0].soft_wrapped);
    assert!(active.row_metadata[1].wrap_continuation);
    apply(
        &mut bank,
        &mut terminal,
        view,
        PresentationAction::Selection {
            selection: Some(Selection::All),
        },
    );
    let selected = frame(&mut bank, &mut terminal, view, true);
    assert_eq!(selected.sequence, first.sequence);
    assert!(selected.version > inactive.version);
    assert!(selected
        .screen
        .screen
        .cells
        .iter()
        .any(|cell| cell.selected));
}
#[test]
fn leases_prune_native_handles_before_replacement_and_headless_input_creates_no_view() {
    let mut terminal = Terminal::new(8, 3).unwrap();
    let mut bank = Presentations::new();
    let controller = Lease {
        controller: 1,
        epoch: 1,
    };
    bank.follow_input(&mut terminal, controller).unwrap();
    assert!(bank.entries.is_empty());
    frame(
        &mut bank,
        &mut terminal,
        PresentationLease::Controller(controller),
        true,
    );
    for viewer in 0..16 {
        frame(
            &mut bank,
            &mut terminal,
            PresentationLease::Observer(ObserverLease { viewer, epoch: 1 }),
            true,
        );
    }
    assert_eq!(bank.entries.len(), 17);
    for epoch in 2..25 {
        let next = ObserverLease { viewer: 0, epoch };
        bank.observer_changed(next);
        frame(
            &mut bank,
            &mut terminal,
            PresentationLease::Observer(next),
            true,
        );
        bank.controller_changed(Lease {
            controller: 1,
            epoch,
        });
        frame(
            &mut bank,
            &mut terminal,
            PresentationLease::Controller(Lease {
                controller: 1,
                epoch,
            }),
            true,
        );
        assert_eq!(bank.entries.len(), 17);
    }
}
