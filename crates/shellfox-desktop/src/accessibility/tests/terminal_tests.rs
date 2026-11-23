use super::*;
use shellfox_ghostty_vt::{Cell, Cursor, Rgb, Screen};

fn screen() -> Screen {
    let cell = |text: &str, width| Cell {
        text: text.into(),
        foreground: Rgb {
            r: 255,
            g: 255,
            b: 255,
        },
        background: Rgb { r: 0, g: 0, b: 0 },
        bold: false,
        italic: false,
        underline: false,
        inverse: false,
        faint: false,
        invisible: false,
        strikethrough: false,
        width,
        selected: false,
        has_hyperlink: false,
    };
    Screen {
        cols: 4,
        rows: 1,
        cells: vec![
            cell("雪", 2),
            cell("", 0),
            cell("e\u{301}", 1),
            cell(" ", 1),
        ],
        cursor: Cursor {
            x: 2,
            y: 0,
            visible: true,
        },
    }
}

fn geometry() -> TerminalGeometry {
    TerminalGeometry {
        bounds: Rect::new(100., 50., 140., 70.),
        cell_width: 10.,
        line_height: 20.,
    }
}

fn document() -> TerminalDocument {
    TerminalDocument::new(SemanticId(10), vec![SemanticId(11)]).unwrap()
}

#[test]
fn native_terminal_has_text_runs_caret_and_grapheme_geometry() {
    let snapshot = document()
        .snapshot(&screen(), geometry(), None, false, Instant::now())
        .unwrap();
    assert_eq!(snapshot.document.role(), Role::Terminal);
    assert_eq!(snapshot.document.label(), Some("Interactive terminal"));
    assert!(snapshot.document.is_read_only());
    assert!(!snapshot.document.supports_action(Action::SetValue));
    let run = &snapshot.runs[0].1;
    assert_eq!(run.value(), Some("雪e\u{301} \n"));
    assert_eq!(run.character_lengths(), &[3, 3, 1, 1]);
    assert_eq!(run.character_positions().unwrap(), &[0., 20., 30., 40.]);
    assert_eq!(run.character_widths().unwrap(), &[20., 10., 10., 0.]);
    assert_eq!(
        snapshot
            .document
            .text_selection()
            .unwrap()
            .focus
            .character_index,
        1
    );
    let mut root = snapshot.document;
    root.set_children([SemanticId(11)]);
    assert!(
        SemanticTree::new(
            SemanticId(10),
            SemanticId(10),
            std::iter::once((SemanticId(10), root)).chain(snapshot.runs)
        )
        .is_ok()
    );
}

#[test]
fn samples_at_most_twenty_times_per_second_without_throttling_layout() {
    let mut doc = document();
    let mut screen = screen();
    let start = Instant::now();
    let first = doc
        .snapshot(&screen, geometry(), None, false, start)
        .unwrap();
    screen.cells[0].text = "語".into();
    let mut moved = geometry();
    moved.bounds = Rect::new(200., 50., 240., 70.);
    for millisecond in 1..50 {
        let next = doc
            .snapshot(
                &screen,
                moved,
                None,
                false,
                start + Duration::from_millis(millisecond),
            )
            .unwrap();
        assert_eq!(next.runs[0].1.value(), first.runs[0].1.value());
        assert_eq!(next.runs[0].1.bounds().unwrap().x0, 200.);
    }
    assert!(!doc.refresh_delay(start + Duration::from_millis(49)).is_some_and(|delay| delay.is_zero()));
    assert!(doc.refresh_delay(start + Duration::from_millis(50)).is_some_and(|delay| delay.is_zero()));
    let final_idle_frame = doc
        .snapshot(
            &screen,
            moved,
            None,
            false,
            start + Duration::from_millis(50),
        )
        .unwrap();
    assert!(!doc.refresh_delay(start + Duration::from_millis(100)).is_some_and(|delay| delay.is_zero()));
    assert!(
        final_idle_frame.runs[0]
            .1
            .value()
            .unwrap()
            .starts_with('語')
    );
}

#[test]
fn selection_preserves_direction_and_wide_cell_mapping() {
    let mut doc = document();
    let selection = TerminalSelection {
        anchor: TerminalPoint { column: 3, row: 0 },
        focus: TerminalPoint { column: 1, row: 0 },
    };
    let snapshot = doc
        .snapshot(
            &screen(),
            geometry(),
            Some(selection),
            false,
            Instant::now(),
        )
        .unwrap();
    let selection = snapshot.document.text_selection().unwrap();
    assert_eq!(selection.anchor.character_index, 2);
    assert_eq!(selection.focus.character_index, 0);
    assert_eq!(selection.focus.node, snapshot.runs[0].0);
    assert_eq!(snapshot.runs[0].1.character_positions().unwrap()[0], 0.);
}

#[test]
fn invisible_text_is_not_disclosed_and_control_sequences_are_not_replayed() {
    let mut screen = screen();
    screen.cells[0].invisible = true;
    screen.cells[0].text = "secret".into();
    let snapshot = document()
        .snapshot(&screen, geometry(), None, true, Instant::now())
        .unwrap();
    assert!(!snapshot.runs[0].1.value().unwrap().contains("secret"));
    screen.cells[0].invisible = false;
    screen.cells[0].text = "\x1b]52;c;secret\x07".into();
    assert!(
        document()
            .snapshot(&screen, geometry(), None, false, Instant::now())
            .is_err()
    );
}

#[test]
fn supports_the_full_desktop_renderer_grid_instead_of_dropping_large_windows() {
    let mut screen = screen();
    let mut cell = screen.cells[3].clone();
    cell.text = "x".into();
    screen.cols = 500;
    screen.rows = 250;
    screen.cells = vec![cell; 125_000];
    let ids = (11..261).map(SemanticId).collect();
    let mut doc = TerminalDocument::new(SemanticId(10), ids).unwrap();
    let snapshot = doc
        .snapshot(&screen, geometry(), None, false, Instant::now())
        .unwrap();
    assert_eq!(snapshot.runs.len(), 250);
    assert_eq!(snapshot.runs[249].1.character_lengths().len(), 501);
}

#[test]
fn rejects_oversized_or_malformed_screens_and_identifier_collisions() {
    assert!(TerminalDocument::new(SemanticId(1), vec![SemanticId(1)]).is_err());
    assert!(TerminalDocument::new(SemanticId(1), vec![SemanticId(2), SemanticId(2)]).is_err());
    let mut screen = screen();
    screen.cells.pop();
    assert!(
        document()
            .snapshot(&screen, geometry(), None, false, Instant::now())
            .is_err()
    );
    screen.cells.push(screen.cells[0].clone());
    screen.cells[0].text = "x".repeat(256);
    assert!(
        document()
            .snapshot(&screen, geometry(), None, false, Instant::now())
            .is_err()
    );
}
