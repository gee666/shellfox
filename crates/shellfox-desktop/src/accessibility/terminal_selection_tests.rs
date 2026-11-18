use super::terminal_view_tests::{cell, document, frame, geometry};
use super::*;
use shellfox_runtime::{
    HistoryPoint, PresentationBuffer, PresentationSelection, PresentationSelectionKind, ViewFrame,
};

fn unicode_view(clustered: bool) -> ViewFrame {
    let mut cells = vec![
        cell("A", 1),
        cell("界", 2),
        cell("", 0),
        cell("e\u{301}", 1),
    ];
    if clustered {
        cells.extend([cell("👩\u{200d}💻", 2), cell("", 0), cell("Z", 1)]);
    } else {
        cells.extend([
            cell("👩\u{200d}", 2),
            cell("", 0),
            cell("💻", 2),
            cell("", 0),
            cell("Z", 1),
        ]);
    }
    while cells.len() < 10 {
        cells.push(cell(" ", 1));
    }
    frame(10, cells)
}
fn select(view: &mut ViewFrame, anchor: u16, focus: u16) {
    view.selection = Some(PresentationSelection {
        anchor: HistoryPoint {
            x: anchor,
            row: view.top_row,
        },
        focus: HistoryPoint {
            x: focus,
            row: view.top_row,
        },
        rectangle: false,
        buffer: PresentationBuffer::Primary,
        kind: PresentationSelectionKind::Range,
        view_top_row: view.top_row,
        total_rows: view.total_rows,
    });
}
fn sampled_text(snapshot: &TerminalSnapshot, selection: TextSelection) -> &str {
    let run = &snapshot.runs[0].1;
    let lengths = run.character_lengths();
    let start: usize = lengths[..selection
        .anchor
        .character_index
        .min(selection.focus.character_index)]
        .iter()
        .map(|len| usize::from(*len))
        .sum();
    let end: usize = lengths[..selection
        .anchor
        .character_index
        .max(selection.focus.character_index)]
        .iter()
        .map(|len| usize::from(*len))
        .sum();
    &run.value().unwrap()[start..end]
}

#[test]
fn inclusive_wide_tail_endpoints_map_to_one_character_preserving_reverse_roles() {
    let mut view = unicode_view(true);
    for (anchor, focus, a_index, f_index) in [(1, 2, 1, 2), (2, 1, 2, 1)] {
        select(&mut view, anchor, focus);
        let snapshot = document(1)
            .snapshot_view(&view, geometry(), false, Instant::now())
            .unwrap();
        let selection = *snapshot.document.text_selection().unwrap();
        assert_eq!(
            (
                selection.anchor.character_index,
                selection.focus.character_index
            ),
            (a_index, f_index)
        );
        assert_eq!(sampled_text(&snapshot, selection), "界");
    }
}

#[test]
fn same_cell_combining_cluster_is_selected_not_collapsed_to_a_caret() {
    let mut view = unicode_view(true);
    select(&mut view, 3, 3);
    let snapshot = document(1)
        .snapshot_view(&view, geometry(), false, Instant::now())
        .unwrap();
    let selection = *snapshot.document.text_selection().unwrap();
    assert_eq!(
        (
            selection.anchor.character_index,
            selection.focus.character_index
        ),
        (2, 3)
    );
    assert_eq!(sampled_text(&snapshot, selection), "e\u{301}");
}

#[test]
fn mode2027_cell_boundaries_are_not_reclustered_across_separate_zwj_cells() {
    // These fixtures mirror the producer's separately native-tested ON/OFF cell contracts.
    for (clustered, expected, length) in [(true, "👩\u{200d}💻", 11), (false, "👩\u{200d}", 7)]
    {
        let mut view = unicode_view(clustered);
        select(&mut view, 4, 5);
        let snapshot = document(1)
            .snapshot_view(&view, geometry(), false, Instant::now())
            .unwrap();
        let selection = *snapshot.document.text_selection().unwrap();
        assert_eq!(sampled_text(&snapshot, selection), expected);
        assert_eq!(snapshot.runs[0].1.character_lengths()[3], length);
    }
}

#[test]
fn offscreen_and_rectangular_endpoints_never_become_a_fake_visible_range() {
    let mut view = unicode_view(true);
    select(&mut view, 1, 2);
    view.selection.as_mut().unwrap().anchor.row = view.top_row - 1;
    let mut doc = document(1);
    let snapshot = doc
        .snapshot_view(&view, geometry(), false, Instant::now())
        .unwrap();
    assert!(snapshot.document.text_selection().is_none());
    select(&mut view, 1, 2);
    view.selection.as_mut().unwrap().rectangle = true;
    let mut doc = document(1);
    let snapshot = doc
        .snapshot_view(&view, geometry(), false, Instant::now())
        .unwrap();
    assert!(snapshot.document.text_selection().is_none());
}

#[test]
fn unmapped_spacer_has_no_guessed_character_boundary() {
    let mut view = frame(
        4,
        vec![cell("", 0), cell("a", 1), cell("b", 1), cell("c", 1)],
    );
    select(&mut view, 0, 0);
    let mut doc = document(1);
    let snapshot = doc
        .snapshot_view(&view, geometry(), false, Instant::now())
        .unwrap();
    assert!(snapshot.document.text_selection().is_none());
}

#[test]
fn none_endpoints_remain_none_even_when_cell_masks_are_selected() {
    let mut view = unicode_view(true);
    for cell in &mut std::sync::Arc::get_mut(&mut view.screen)
        .unwrap()
        .screen
        .cells
    {
        cell.selected = true;
    }
    let mut doc = document(1);
    let snapshot = doc
        .snapshot_view(&view, geometry(), false, Instant::now())
        .unwrap();
    assert!(snapshot.document.text_selection().is_none());
}

#[test]
fn at_action_converts_character_boundaries_back_to_checked_view_local_inclusive_cells() {
    let mut view = unicode_view(true);
    view.version = (1u64 << 53) + 17;
    select(&mut view, 2, 1);
    let mut doc = document(1);
    let snapshot = doc
        .snapshot_view(&view, geometry(), false, Instant::now())
        .unwrap();
    let selection = *snapshot.document.text_selection().unwrap();
    let Some(shellfox_runtime::PresentationAction::SelectionAtVersion {
        version,
        selection:
            Some(shellfox_ghostty_vt::Selection::Range {
                start,
                end,
                rectangle: false,
            }),
    }) = doc.selection_action_for_view(&selection, snapshot.terminal_stamp, &view)
    else {
        panic!("missing mapped selection action")
    };
    assert_eq!(version, (1u64 << 53) + 17);
    assert_eq!(version, snapshot.terminal_stamp.unwrap().version);
    assert_eq!((start.x, start.y, end.x, end.y), (2, 0, 1, 0));
    view.version += 1;
    assert!(
        doc.selection_action_for_view(&selection, snapshot.terminal_stamp, &view)
            .is_none()
    );
}

#[test]
fn at_hard_newline_only_selection_is_unavailable_not_a_synthetic_terminal_cell() {
    let view = frame(2, vec![cell("a", 1), cell("b", 1)]);
    let mut doc = document(1);
    let snapshot = doc
        .snapshot_view(&view, geometry(), false, Instant::now())
        .unwrap();
    let selection = TextSelection {
        anchor: accesskit::TextPosition {
            node: SemanticId(11),
            character_index: 2,
        },
        focus: accesskit::TextPosition {
            node: SemanticId(11),
            character_index: 3,
        },
    };
    assert!(
        doc.selection_action_for_view(&selection, snapshot.terminal_stamp, &view)
            .is_none()
    );
}

#[test]
fn same_cell_at_selection_stays_a_range_while_collapsed_boundary_clears_only_view_selection() {
    let mut view = unicode_view(true);
    view.version = 23;
    select(&mut view, 3, 3);
    let mut doc = document(1);
    let snapshot = doc
        .snapshot_view(&view, geometry(), false, Instant::now())
        .unwrap();
    let mut selected = *snapshot.document.text_selection().unwrap();
    assert!(
        matches!(doc.selection_action_for_view(&selected, snapshot.terminal_stamp, &view),
        Some(shellfox_runtime::PresentationAction::SelectionAtVersion { version: 23, selection: Some(shellfox_ghostty_vt::Selection::Range { start, end, .. }) })
        if start.x == 3 && end.x == 3)
    );
    selected.focus = selected.anchor;
    assert!(matches!(
        doc.selection_action_for_view(&selected, snapshot.terminal_stamp, &view),
        Some(shellfox_runtime::PresentationAction::SelectionAtVersion {
            version: 23,
            selection: None
        })
    ));
}

#[test]
fn reversed_one_column_character_cannot_fabricate_a_reverse_equal_cell_gesture() {
    let mut view = unicode_view(true);
    select(&mut view, 3, 3);
    let mut doc = document(1);
    let snapshot = doc
        .snapshot_view(&view, geometry(), false, Instant::now())
        .unwrap();
    let selected = snapshot.document.text_selection().unwrap();
    let reversed = TextSelection {
        anchor: selected.focus,
        focus: selected.anchor,
    };
    assert!(
        doc.selection_action_for_view(&reversed, snapshot.terminal_stamp, &view)
            .is_none()
    );
}

#[test]
fn changed_buffer_selection_context_is_not_reused_for_old_at_indices() {
    let mut view = unicode_view(true);
    select(&mut view, 1, 2);
    let mut doc = document(1);
    let snapshot = doc
        .snapshot_view(&view, geometry(), false, Instant::now())
        .unwrap();
    let selected = *snapshot.document.text_selection().unwrap();
    view.selection.as_mut().unwrap().buffer = PresentationBuffer::Alternate;
    view.version += 1;
    assert!(
        doc.selection_action_for_view(&selected, snapshot.terminal_stamp, &view)
            .is_none()
    );
}

#[test]
fn mismatched_selection_history_interval_is_rejected_not_combined_with_new_cells() {
    let mut view = unicode_view(true);
    select(&mut view, 1, 2);
    view.selection.as_mut().unwrap().view_top_row += 1;
    assert!(
        document(1)
            .snapshot_view(&view, geometry(), false, Instant::now())
            .is_err()
    );
}
