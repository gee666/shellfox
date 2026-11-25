use super::*;
use shellfox_ghostty_vt::{Cell, Cursor, Rgb, Screen};
use shellfox_runtime::{
    CursorShape, CursorVisual, Identity, Lease, PresentationLease, RowMetadata, ScreenFrame,
    ViewFrame,
};
use std::sync::Arc;

pub(super) fn cell(text: &str, width: u8) -> Cell {
    Cell {
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
        foreground_source: Default::default(),
        background_source: Default::default(),
    }
}
pub(super) fn frame(cols: u16, cells: Vec<Cell>) -> ViewFrame {
    let rows = u16::try_from(cells.len() / usize::from(cols)).unwrap();
    let identity = Identity {
        session_id: shellfox_core::SessionId::new(),
        tab_id: shellfox_core::TabId::new(),
        generation: shellfox_core::Generation::new(),
    };
    ViewFrame {
        identity,
        view: PresentationLease::Controller(Lease {
            controller: 1,
            epoch: 1,
        }),
        sequence: 900,
        version: 1,
        screen: Arc::new(ScreenFrame {
            identity,
            sequence: 900,
            mouse_tracking: false,
            screen: Screen {
                cols,
                rows,
                cells,
                cursor: Cursor {
                    x: 0,
                    y: 0,
                    visible: false,
                },
            },
        }),
        row_metadata: vec![
            RowMetadata {
                soft_wrapped: false,
                wrap_continuation: false
            };
            usize::from(rows)
        ]
        .into(),
        cursor_visual: CursorVisual {
            x: 0,
            y: 0,
            visible: false,
            shape: CursorShape::Bar,
            blinking: false,
            color: Rgb {
                r: 200,
                g: 100,
                b: 0,
            },
            width: 1,
            wide_tail: false,
        },
        selection: None,
        top_row: 7,
        total_rows: 30,
    }
}
pub(super) fn document(rows: usize) -> TerminalDocument {
    TerminalDocument::new(
        SemanticId(10),
        (11..11 + rows as u64).map(SemanticId).collect(),
    )
    .unwrap()
}
pub(super) fn geometry() -> TerminalGeometry {
    TerminalGeometry {
        bounds: Rect::new(100., 50., 300., 150.),
        cell_width: 10.,
        line_height: 20.,
    }
}

#[test]
fn true_wraps_join_text_but_preserve_physical_row_bounds_and_history_stamp() {
    let mut view = frame(
        4,
        "abcdefghijkl"
            .chars()
            .map(|c| cell(&c.to_string(), 1))
            .collect(),
    );
    view.row_metadata = vec![
        RowMetadata {
            soft_wrapped: true,
            wrap_continuation: true,
        },
        RowMetadata {
            soft_wrapped: false,
            wrap_continuation: true,
        },
        RowMetadata {
            soft_wrapped: false,
            wrap_continuation: false,
        },
    ]
    .into();
    let mut doc = document(3);
    let snapshot = doc
        .snapshot_view(&view, geometry(), false, Instant::now())
        .unwrap();
    assert_eq!(snapshot.runs[0].1.value(), Some("abcd"));
    assert_eq!(snapshot.runs[1].1.value(), Some("efgh\n"));
    assert_eq!(snapshot.runs[2].1.value(), Some("ijkl\n"));
    assert_eq!(snapshot.runs[1].1.bounds().unwrap().y0, 70.);
    let stamp = snapshot.terminal_stamp.unwrap();
    assert_eq!(stamp.top_row, 7);
    assert_eq!(stamp.total_rows, 30);
}

#[test]
fn caret_uses_view_cursor_visual_and_retained_history_not_screen_or_sequence() {
    let mut view = frame(4, vec![cell("a", 1); 8]);
    view.cursor_visual.visible = true;
    view.cursor_visual.x = 3;
    view.cursor_visual.y = 1;
    let mut doc = document(2);
    let snapshot = doc
        .snapshot_view(&view, geometry(), false, Instant::now())
        .unwrap();
    let selected = snapshot.document.text_selection().unwrap();
    assert_eq!(selected.focus.node, SemanticId(12));
    assert_eq!(selected.focus.character_index, 3);
    assert_eq!(selected.anchor, selected.focus);
    assert_eq!(snapshot.terminal_stamp.unwrap().top_row, 7);
    assert!(!view.screen.screen.cursor.visible);
}

#[test]
fn same_sequence_new_view_version_updates_cursor_without_cloning_cells() {
    let mut view = frame(4, vec![cell("a", 1); 4]);
    view.cursor_visual.visible = true;
    let start = Instant::now();
    let cells = view.screen.screen.cells.as_ptr();
    let refs = Arc::strong_count(&view.screen);
    let mut doc = document(1);
    doc.snapshot_view(&view, geometry(), false, start).unwrap();
    view.version += 1;
    view.cursor_visual.x = 2;
    let snapshot = doc
        .snapshot_view(&view, geometry(), false, start + Duration::from_millis(50))
        .unwrap();
    assert_eq!(
        snapshot
            .document
            .text_selection()
            .unwrap()
            .focus
            .character_index,
        2
    );
    assert_eq!(snapshot.terminal_stamp.unwrap().version, 2);
    assert_eq!(snapshot.terminal_stamp.unwrap().sequence, 900);
    assert_eq!(view.screen.screen.cells.as_ptr(), cells);
    assert_eq!(Arc::strong_count(&view.screen), refs);
}

#[test]
fn wide_tail_caret_maps_through_the_published_character_and_preserves_the_view_stamp() {
    let mut view = frame(
        4,
        vec![cell("a", 1), cell("界", 2), cell("", 0), cell("b", 1)],
    );
    view.cursor_visual.visible = true;
    view.cursor_visual.x = 2;
    view.cursor_visual.wide_tail = true;
    view.cursor_visual.width = 2;
    let mut doc = document(1);
    let snapshot = doc
        .snapshot_view(&view, geometry(), false, Instant::now())
        .unwrap();
    assert_eq!(
        snapshot
            .document
            .text_selection()
            .unwrap()
            .focus
            .character_index,
        1
    );
    assert_eq!(snapshot.terminal_stamp.unwrap().top_row, 7);
    assert_eq!(view.cursor_visual.x, 2);
}

#[test]
fn unchanged_view_does_not_create_a_permanent_idle_repaint_loop() {
    let view = frame(4, vec![cell("a", 1); 4]);
    let mut doc = document(1);
    let start = Instant::now();
    doc.snapshot_view(&view, geometry(), false, start).unwrap();
    for n in 1..50 {
        doc.snapshot_view(&view, geometry(), false, start + Duration::from_millis(n))
            .unwrap();
    }
    assert!(
        !doc.refresh_delay(start + Duration::from_millis(100))
            .is_some_and(|delay| delay.is_zero())
    );
}

#[test]
fn coalescing_keeps_a_coherent_old_stamp_and_rejects_at_mapping_until_new_sample() {
    let mut view = frame(4, vec![cell("a", 1); 4]);
    view.cursor_visual.visible = true;
    let mut doc = document(1);
    let start = Instant::now();
    let first = doc.snapshot_view(&view, geometry(), false, start).unwrap();
    let range = *first.document.text_selection().unwrap();
    view.version += 1;
    view.top_row += 1;
    let next = doc
        .snapshot_view(&view, geometry(), false, start + Duration::from_millis(1))
        .unwrap();
    assert_eq!(
        next.document.text_selection(),
        first.document.text_selection()
    );
    assert_eq!(next.terminal_stamp, first.terminal_stamp);
    assert_eq!(next.terminal_stamp.unwrap().top_row, 7);
    assert!(
        doc.selection_action_for_view(&range, next.terminal_stamp, &view)
            .is_none()
    );
    assert!(
        doc.refresh_delay(start + Duration::from_millis(50))
            .is_some_and(|delay| delay.is_zero())
    );
    let fresh = doc
        .snapshot_view(&view, geometry(), false, start + Duration::from_millis(50))
        .unwrap();
    assert_eq!(fresh.terminal_stamp.unwrap().top_row, 8);
    // Refreshing the document must not authorize an action captured from the old publication.
    assert!(
        doc.selection_action_for_view(&range, first.terminal_stamp, &view)
            .is_none()
    );
    assert!(matches!(
        doc.selection_action_for_view(&range, fresh.terminal_stamp, &view),
        Some(shellfox_runtime::PresentationAction::SelectionAtVersion {
            version: 2,
            selection: None,
        })
    ));
}

#[test]
fn observer_frame_is_read_only_even_if_caller_passes_interactive_label() {
    let mut view = frame(4, vec![cell("a", 1); 4]);
    view.view = PresentationLease::Observer(shellfox_runtime::ObserverLease {
        viewer: 2,
        epoch: 1,
    });
    let snapshot = document(1)
        .snapshot_view(&view, geometry(), false, Instant::now())
        .unwrap();
    assert_eq!(snapshot.document.label(), Some("Read-only terminal"));
    assert!(!snapshot.document.supports_action(Action::SetValue));
}

#[test]
fn invalid_history_or_metadata_is_an_error_not_a_clamped_visible_frame() {
    let mut view = frame(4, vec![cell("a", 1); 4]);
    view.top_row = 30;
    assert!(
        document(1)
            .snapshot_view(&view, geometry(), false, Instant::now())
            .is_err()
    );
    view.top_row = 0;
    view.row_metadata = Vec::new().into();
    assert!(
        document(1)
            .snapshot_view(&view, geometry(), false, Instant::now())
            .is_err()
    );
}
