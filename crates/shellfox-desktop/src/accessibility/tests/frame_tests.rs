use super::*;

fn root() -> SemanticNode {
    SemanticNode::new(SemanticId(1), None, Role::Window, "Shellfox")
}
fn control(id: u64) -> SemanticNode {
    let mut node = SemanticNode::new(
        SemanticId(id),
        Some(SemanticId(1)),
        Role::Button,
        "New session",
    );
    node.data
        .set_bounds(accesskit::Rect::new(20., 30., 100., 54.));
    node
}

#[test]
fn retained_entity_clone_collects_into_the_same_window_after_every_reset() {
    let context = WindowSemanticContext::new(SemanticId(1));
    let root_frame = context.frame();
    // This is the handle an unchanged Settings/Field entity retained from an earlier render.
    let retained_child = context.frame();
    assert!(root_frame.same_context(&retained_child));
    for _ in 0..3 {
        let generation = root_frame.begin_frame();
        root_frame.record(root(), false);
        retained_child.record(control(2), false);
        let tree = root_frame.finish_frame(generation).unwrap();
        assert_eq!(
            tree.node(SemanticId(1)).unwrap().children(),
            &[SemanticId(2)]
        );
        assert_eq!(
            tree.node(SemanticId(2)).unwrap().bounds(),
            Some(accesskit::Rect::new(20., 30., 100., 54.))
        );
    }
}

#[test]
fn reset_removes_vanished_controls_and_duplicate_ids_from_previous_paints() {
    let frame = SemanticFrame::new(SemanticId(1));
    let first = frame.begin_frame();
    frame.record(root(), false);
    frame.record(control(2), false);
    frame.record(control(2), false);
    assert!(matches!(
        frame.finish_frame(first),
        Err(SemanticError::DuplicateId(_))
    ));
    let second = frame.begin_frame();
    frame.record(root(), false);
    frame.record(control(3), false);
    let tree = frame.finish_frame(second).unwrap();
    assert!(tree.node(SemanticId(2)).is_none());
    assert_eq!(
        tree.node(SemanticId(1)).unwrap().children(),
        &[SemanticId(3)]
    );
}

#[test]
fn partial_and_outside_paint_collection_is_not_publishable() {
    let frame = SemanticFrame::new(SemanticId(1));
    assert!(matches!(frame.tree(), Err(SemanticError::FrameNotComplete)));
    frame.record(control(2), false);
    let generation = frame.begin_frame();
    frame.record(root(), false);
    assert!(matches!(frame.tree(), Err(SemanticError::FrameNotComplete)));
    let complete = frame.finish_frame(generation).unwrap();
    assert!(complete.node(SemanticId(2)).is_none());
    frame.record(control(3), false);
    assert!(matches!(
        frame.tree(),
        Err(SemanticError::FrameNotCollecting)
    ));
}

#[test]
fn stale_generation_cannot_finish_a_newer_root_paint() {
    let frame = SemanticFrame::new(SemanticId(1));
    let stale = frame.begin_frame();
    frame.record(root(), false);
    let current = frame.begin_frame();
    frame.record(root(), false);
    frame.record(control(2), false);
    assert!(matches!(
        frame.finish_frame(stale),
        Err(SemanticError::FrameGeneration)
    ));
    assert!(
        frame
            .finish_frame(current)
            .unwrap()
            .node(SemanticId(2))
            .is_some()
    );
}

#[test]
fn equal_generation_numbers_do_not_allow_cross_window_collection() {
    let first = WindowSemanticContext::new(SemanticId(1)).frame();
    let second = WindowSemanticContext::new(SemanticId(1)).frame();
    assert!(!first.same_context(&second));
    let first_generation = first.begin_frame();
    let second_generation = second.begin_frame();
    first.record(root(), false);
    second.record(root(), false);
    assert!(matches!(
        second.finish_frame(first_generation),
        Err(SemanticError::FrameGeneration)
    ));
    assert!(second.finish_frame(second_generation).is_ok());
}

#[test]
fn key_reservations_survive_reset_and_are_shared_by_retained_entity_clones() {
    let context = WindowSemanticContext::new(SemanticId(1));
    let frame = context.frame();
    let retained = context.frame();
    let id = frame.id_for("settings/close").unwrap();
    let rows = frame
        .reserve_ids("terminal/session/generation/rows", 250)
        .unwrap();
    let generation = frame.begin_frame();
    frame.record(root(), false);
    assert!(frame.finish_frame(generation).is_ok());
    let generation = frame.begin_frame();
    frame.record(root(), false);
    assert_eq!(retained.id_for("settings/close").unwrap(), id);
    assert_eq!(
        retained
            .reserve_ids("terminal/session/generation/rows", 250)
            .unwrap(),
        rows
    );
    let another = retained.id_for("settings/name").unwrap();
    assert_ne!(another, id);
    assert!(!rows.contains(&another));
    assert!(frame.finish_frame(generation).is_ok());
}

#[test]
fn later_paint_bounds_replace_old_geometry_without_accumulating_nodes() {
    let frame = WindowSemanticContext::new(SemanticId(1)).frame();
    let first = frame.begin_frame();
    frame.record(root(), false);
    frame.record(control(2), false);
    let old = frame.finish_frame(first).unwrap();
    let second = frame.begin_frame();
    frame.record(root(), false);
    let mut moved = control(2);
    moved
        .data
        .set_bounds(accesskit::Rect::new(70., 90., 150., 114.));
    frame.record(moved, false);
    let new = frame.finish_frame(second).unwrap();
    assert_eq!(
        new.node(SemanticId(2)).unwrap().bounds(),
        Some(accesskit::Rect::new(70., 90., 150., 114.))
    );
    assert_eq!(new.delta(&old).nodes.len(), 1);
}
