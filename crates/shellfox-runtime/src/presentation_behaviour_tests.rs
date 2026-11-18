use super::*;
use crate::{Lease, ObserverLease};
use shellfox_core::{Generation, SessionId, TabId};
use shellfox_ghostty_vt::{Selection, ViewportPoint};
fn identity() -> Identity {
    Identity {
        session_id: SessionId::new(),
        tab_id: TabId::new(),
        generation: Generation::new(),
    }
}
fn read(
    bank: &mut Presentations,
    terminal: &mut Terminal,
    view: PresentationLease,
) -> Arc<ViewFrame> {
    match bank
        .action(
            terminal,
            identity(),
            7,
            GridLimits::default(),
            view,
            PresentationAction::Frame { focused: true },
        )
        .unwrap()
    {
        PresentationResult::Frame(frame) => frame,
        _ => panic!("frame expected"),
    }
}
#[test]
fn input_follow_tail_is_controller_local_and_links_resolve_in_each_history_view() {
    let mut terminal = Terminal::new(12, 3).unwrap();
    terminal.feed(b"\x1b]8;;https://example.invalid/history\x1b\\first\x1b]8;;\x1b\\\r\nsecond\r\nthird\r\nfourth\r\nlast").unwrap();
    let mut bank = Presentations::new();
    let controller = Lease {
        controller: 1,
        epoch: 1,
    };
    let controlled = PresentationLease::Controller(controller);
    let observed = PresentationLease::Observer(ObserverLease {
        viewer: 2,
        epoch: 2,
    });
    for view in [controlled, observed] {
        bank.action(
            &mut terminal,
            identity(),
            7,
            GridLimits::default(),
            view,
            PresentationAction::Scroll { delta: isize::MIN },
        )
        .unwrap();
    }
    let before = read(&mut bank, &mut terminal, observed);
    assert_eq!(before.top_row, 0);
    let link = bank
        .action(
            &mut terminal,
            identity(),
            7,
            GridLimits::default(),
            observed,
            PresentationAction::Hyperlink {
                point: ViewportPoint { x: 1, y: 0 },
            },
        )
        .unwrap();
    assert!(
        matches!(link, PresentationResult::Text(Some(uri)) if uri == "https://example.invalid/history")
    );
    bank.follow_input(&mut terminal, controller).unwrap();
    let tail = read(&mut bank, &mut terminal, controlled);
    assert_eq!(
        tail.top_row + u64::from(tail.screen.screen.rows),
        tail.total_rows
    );
    let observer_after = read(&mut bank, &mut terminal, observed);
    assert!(Arc::ptr_eq(&before, &observer_after));
    assert_eq!(observer_after.top_row, 0);
}
#[test]
fn frames_preserve_vt_cell_clusters_instead_of_reclustering_joined_unicode_text() {
    for (mode, expected) in [
        (b"\x1b[?2027l".as_slice(), vec!["👩\u{200d}", "💻"]),
        (b"\x1b[?2027h".as_slice(), vec!["👩\u{200d}💻"]),
    ] {
        let mut terminal = Terminal::new(12, 3).unwrap();
        terminal.feed(mode).unwrap();
        terminal.feed("👩\u{200d}💻".as_bytes()).unwrap();
        let mut bank = Presentations::new();
        let view = PresentationLease::Observer(ObserverLease {
            viewer: 1,
            epoch: 1,
        });
        let frame = read(&mut bank, &mut terminal, view);
        let cells: Vec<_> = frame
            .screen
            .screen
            .cells
            .iter()
            .filter(|cell| !cell.text.is_empty())
            .collect();
        assert_eq!(
            cells
                .iter()
                .map(|cell| cell.text.as_str())
                .collect::<Vec<_>>(),
            expected
        );
        assert!(cells.iter().all(|cell| cell.width == 2));
        assert!(frame.screen.screen.cells[1].width == 0);
        assert_eq!(
            cells
                .iter()
                .map(|cell| cell.text.encode_utf16().count())
                .sum::<usize>(),
            5
        );
    }
}
#[test]
fn a_frame_budget_refusal_is_optional_and_does_not_poison_the_source() {
    let mut terminal = Terminal::new(80, 24).unwrap();
    terminal.feed(b"source alive").unwrap();
    let mut bank = Presentations::new();
    let view = PresentationLease::Observer(ObserverLease {
        viewer: 1,
        epoch: 1,
    });
    let refused = bank.action(
        &mut terminal,
        identity(),
        7,
        GridLimits::new(1).unwrap(),
        view,
        PresentationAction::Frame { focused: true },
    );
    assert!(matches!(
        refused,
        Err(RuntimeError {
            code: crate::ErrorCode::Unsupported,
            ..
        })
    ));
    terminal.feed(b"\x1b[6n").unwrap();
    assert!(!terminal.drain_replies().unwrap().is_empty());
    assert_eq!(read(&mut bank, &mut terminal, view).screen.screen.cols, 80);
}
#[test]
fn presentation_reads_do_not_drain_live_metadata_and_source_changes_invalidate_same_seq_cache() {
    let mut terminal = Terminal::new(12, 3).unwrap();
    terminal.feed(b"hello\x07\x1b]2;volatile\x07").unwrap();
    let mut bank = Presentations::new();
    let view = PresentationLease::Observer(ObserverLease {
        viewer: 1,
        epoch: 1,
    });
    let before = read(&mut bank, &mut terminal, view);
    bank.action(
        &mut terminal,
        identity(),
        7,
        GridLimits::default(),
        view,
        PresentationAction::Selection {
            selection: Some(Selection::All),
        },
    )
    .unwrap();
    bank.action(
        &mut terminal,
        identity(),
        7,
        GridLimits::default(),
        view,
        PresentationAction::Copy,
    )
    .unwrap();
    let metadata = terminal.take_metadata().unwrap().unwrap();
    assert_eq!(metadata.title.as_deref(), Some("volatile"));
    assert_eq!(metadata.bell_count, 1);
    terminal.feed(b"\r\nchanged").unwrap();
    bank.source_changed().unwrap();
    let after = read(&mut bank, &mut terminal, view);
    assert_eq!(after.sequence, before.sequence);
    assert!(after.version > before.version);
    assert!(!Arc::ptr_eq(&before.screen, &after.screen));
    terminal.resize(10, 4).unwrap();
    bank.source_changed().unwrap();
    let resized = read(&mut bank, &mut terminal, view);
    assert_eq!(
        (resized.screen.screen.cols, resized.screen.screen.rows),
        (10, 4)
    );
}
