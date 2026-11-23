use super::*;
use crate::accessibility::TerminalSnapshot;
use shellfox_runtime::{CursorShape, CursorVisual};
use std::sync::Arc;

fn view() -> ViewFrame {
    let mut terminal = shellfox_ghostty_vt::Terminal::new(4, 2).unwrap();
    terminal.feed(b"AB").unwrap();
    let mut presentation = terminal.new_presentation_view().unwrap();
    let rendered = terminal.view_render(&mut presentation).unwrap();
    let identity = Identity {
        session_id: shellfox_core::SessionId::new(),
        tab_id: shellfox_core::TabId::new(),
        generation: shellfox_core::Generation::new(),
    };
    ViewFrame {
        identity,
        view: PresentationLease::Controller(shellfox_runtime::Lease {
            controller: 1,
            epoch: 1,
        }),
        sequence: 1,
        version: 1,
        screen: Arc::new(shellfox_runtime::ScreenFrame {
            identity,
            sequence: 1,
            screen: rendered.screen,
            mouse_tracking: false,
        }),
        row_metadata: rendered.row_metadata.into(),
        cursor_visual: CursorVisual {
            x: 2,
            y: 0,
            visible: true,
            shape: CursorShape::Block,
            blinking: false,
            color: [255, 0, 0].into(),
            width: 1,
            wide_tail: false,
        },
        top_row: rendered.top_row,
        total_rows: rendered.total_rows,
        selection: terminal.view_selection(&mut presentation).unwrap(),
    }
}
#[test]
fn document_ids_survive_paints_but_never_reuse_another_generation_or_view() {
    let frame = SemanticFrame::new(SemanticId(1));
    let mut binding =
        TerminalSemantics::new(frame.clone(), frame.root_id(), "terminal/test".into());
    let mut view = view();
    let first = binding.document(&view).unwrap().id;
    view.version += 1;
    assert_eq!(binding.document(&view).unwrap().id, first);
    view.identity.generation = shellfox_core::Generation::new();
    let generation = binding.document(&view).unwrap().id;
    assert_ne!(generation, first);
    view.view = PresentationLease::Controller(shellfox_runtime::Lease {
        controller: 1,
        epoch: 2,
    });
    assert_ne!(binding.document(&view).unwrap().id, generation);
}
#[test]
fn live_snapshot_borrows_presentation_and_preserves_cell_entries() {
    let frame = SemanticFrame::new(SemanticId(1));
    let mut binding =
        TerminalSemantics::new(frame.clone(), frame.root_id(), "terminal/test".into());
    let view = view();
    let cells = view.screen.screen.cells.as_ptr();
    let document = binding.document(&view).unwrap();
    let TerminalSnapshot {
        document: node,
        runs,
        terminal_stamp,
    } = document
        .text
        .snapshot_view(
            &view,
            TerminalGeometry {
                bounds: accesskit::Rect::new(0., 0., 40., 40.),
                cell_width: 10.,
                line_height: 20.,
            },
            false,
            Instant::now(),
        )
        .unwrap();
    assert_eq!(view.screen.screen.cells.as_ptr(), cells);
    assert_eq!(
        terminal_stamp.map(|stamp| stamp.version),
        Some(view.version)
    );
    assert_eq!(node.role(), Role::Terminal);
    assert_eq!(runs.len(), 2);
    assert_eq!(runs[0].1.character_lengths().len(), 5);
}

fn sample(
    binding: &mut TerminalSemantics,
    view: &ViewFrame,
    now: Instant,
) -> (SemanticId, TerminalSnapshot) {
    let document = binding.document(view).unwrap();
    let snapshot = document
        .text
        .snapshot_view(
            view,
            TerminalGeometry {
                bounds: accesskit::Rect::new(0., 0., 40., 40.),
                cell_width: 10.,
                line_height: 20.,
            },
            false,
            now,
        )
        .unwrap();
    (document.id, snapshot)
}
fn selection_event(id: SemanticId, snapshot: &TerminalSnapshot) -> SemanticAction {
    let row = snapshot.runs[0].0;
    SemanticAction {
        target: id,
        kind: SemanticActionKind::SetTextSelection(accesskit::TextSelection {
            anchor: accesskit::TextPosition {
                node: row,
                character_index: 0,
            },
            focus: accesskit::TextPosition {
                node: row,
                character_index: 1,
            },
        }),
        terminal_stamp: snapshot.terminal_stamp,
    }
}

#[test]
fn paint_registration_uses_the_coalesced_snapshot_not_the_newer_getter_version() {
    let frame = SemanticFrame::new(SemanticId(1));
    let _generation = frame.begin_frame();
    let mut binding =
        TerminalSemantics::new(frame.clone(), frame.root_id(), "terminal/test".into());
    let mut view = view();
    let now = Instant::now();
    let (_, initial) = sample(&mut binding, &view, now);
    view.version += 1;
    let (id, coalesced) = sample(
        &mut binding,
        &view,
        now + std::time::Duration::from_millis(1),
    );
    assert_eq!(coalesced.terminal_stamp, initial.terminal_stamp);
    assert!(
        coalesced
            .document
            .supports_action(accesskit::Action::SetTextSelection)
    );
    register_sample(&frame, id, &coalesced).unwrap();
    let mut wrong = coalesced.terminal_stamp.unwrap();
    wrong.version = view.version;
    assert_eq!(
        frame.set_terminal_view(id, wrong),
        Err(SemanticError::TerminalStamp(id))
    );
}

#[test]
fn dispatch_preserves_captured_version_and_never_adopts_a_newer_identical_document() {
    let frame = SemanticFrame::new(SemanticId(1));
    let mut binding =
        TerminalSemantics::new(frame.clone(), frame.root_id(), "terminal/test".into());
    let mut view = view();
    let now = Instant::now();
    let (id, initial) = sample(&mut binding, &view, now);
    let event = selection_event(id, &initial);
    let (lease, action) = binding.selection_action(&event, &view).unwrap();
    assert_eq!(lease, view.view);
    assert!(matches!(
        action,
        PresentationAction::SelectionAtVersion {
            version: 1,
            selection: Some(_)
        }
    ));
    let mut missing = event.clone();
    missing.terminal_stamp = None;
    assert!(binding.selection_action(&missing, &view).is_none());
    view.version += 1;
    assert!(binding.selection_action(&event, &view).is_none());
    let (_, newer) = sample(
        &mut binding,
        &view,
        now + std::time::Duration::from_millis(100),
    );
    assert_eq!(initial.document, newer.document);
    assert_eq!(initial.runs, newer.runs);
    assert!(binding.selection_action(&event, &view).is_none());
    let refreshed = selection_event(id, &newer);
    assert!(matches!(
        binding.selection_action(&refreshed, &view),
        Some((_, PresentationAction::SelectionAtVersion { version: 2, .. }))
    ));
}

#[test]
fn missing_sample_stamp_and_wrong_target_are_not_silently_promoted() {
    let frame = SemanticFrame::new(SemanticId(1));
    let _generation = frame.begin_frame();
    let mut binding =
        TerminalSemantics::new(frame.clone(), frame.root_id(), "terminal/test".into());
    let view = view();
    let (id, mut snapshot) = sample(&mut binding, &view, Instant::now());
    let mut event = selection_event(id, &snapshot);
    event.target = frame.root_id();
    assert!(binding.selection_action(&event, &view).is_none());
    snapshot.terminal_stamp = None;
    assert_eq!(
        register_sample(&frame, id, &snapshot),
        Err(SemanticError::TerminalStamp(id))
    );
}
