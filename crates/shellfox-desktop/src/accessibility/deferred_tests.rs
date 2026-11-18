use super::*;

fn root() -> SemanticNode {
    SemanticNode::new(SemanticId(1), None, Role::Window, "Shellfox")
}
fn panel() -> SemanticNode {
    SemanticNode::new(SemanticId(2), Some(SemanticId(1)), Role::Region, "Settings")
}
fn dialog() -> SemanticNode {
    SemanticNode::new(
        SemanticId(3),
        Some(SemanticId(2)),
        Role::Dialog,
        "Environment",
    )
}
fn finish_root(frame: &SemanticFrame) -> Result<SemanticTree, SemanticError> {
    let generation = frame.begin_frame();
    frame.record(root(), false);
    frame.finish_frame(generation)
}

#[test]
fn render_time_reject_survives_begin_and_recovers_on_a_later_good_paint() {
    let frame = SemanticFrame::new(SemanticId(1));
    frame.reject(SemanticError::InvalidIdKey);
    assert!(matches!(
        finish_root(&frame),
        Err(SemanticError::InvalidIdKey)
    ));
    assert!(finish_root(&frame).is_ok());
}

#[test]
fn reject_after_completion_invalidates_old_snapshot_and_upcoming_publication() {
    let frame = SemanticFrame::new(SemanticId(1));
    assert!(finish_root(&frame).is_ok());
    frame.reject(SemanticError::IdExhausted);
    assert!(matches!(frame.tree(), Err(SemanticError::IdExhausted)));
    assert!(matches!(
        finish_root(&frame),
        Err(SemanticError::IdExhausted)
    ));
    assert!(finish_root(&frame).is_ok());
}

#[test]
fn active_paint_reject_does_not_poison_a_subsequent_good_frame() {
    let frame = SemanticFrame::new(SemanticId(1));
    let generation = frame.begin_frame();
    frame.record(root(), false);
    frame.reject(SemanticError::Limit);
    assert!(matches!(
        frame.finish_frame(generation),
        Err(SemanticError::Limit)
    ));
    assert!(finish_root(&frame).is_ok());
}

#[test]
fn new_render_error_is_not_lost_when_an_older_completion_is_still_queued() {
    let frame = SemanticFrame::new(SemanticId(1));
    let old = frame.begin_frame();
    frame.record(root(), false);
    frame.reject(SemanticError::InvalidIdCount);
    let current = frame.begin_frame();
    frame.record(root(), false);
    assert!(matches!(
        frame.finish_frame(old),
        Err(SemanticError::FrameGeneration)
    ));
    assert!(matches!(
        frame.finish_frame(current),
        Err(SemanticError::InvalidIdCount)
    ));
}

#[test]
fn first_rejection_is_not_hidden_by_a_later_paint_failure() {
    let frame = SemanticFrame::new(SemanticId(1));
    frame.reject(SemanticError::IdKeyConflict);
    let generation = frame.begin_frame();
    frame.record(root(), false);
    frame.reject(SemanticError::FramePostPaint);
    assert!(matches!(
        frame.finish_frame(generation),
        Err(SemanticError::IdKeyConflict)
    ));
}

#[test]
fn late_overlay_children_are_collected_before_final_tree_validation() {
    let frame = SemanticFrame::new(SemanticId(1));
    let generation = frame.begin_frame();
    frame.record(root(), false);
    frame.record(panel(), false);
    // Normal root paint is done, but GPUI deferred/prompt/tooltip painting has not run.
    frame.expect_overlay(SemanticId(3));
    assert!(matches!(frame.tree(), Err(SemanticError::FrameNotComplete)));
    {
        let _overlay = frame.overlay_scope();
        frame.record(dialog(), false);
        frame.record(
            SemanticNode::new(SemanticId(4), Some(SemanticId(3)), Role::Button, "Close"),
            false,
        );
    }
    // Exercise the same completion entry point as the outer GPUI finish callback.
    let tree = frame.complete_after_draw(generation).unwrap().unwrap();
    assert_eq!(tree.node(SemanticId(3)).unwrap().role(), Role::Dialog);
    assert_eq!(
        tree.node(SemanticId(3)).unwrap().children(),
        &[SemanticId(4)]
    );
}

#[test]
fn overlay_and_deferred_wrappers_preserve_existing_element_identity() {
    use gpui::{Element, InteractiveElement, IntoElement};
    let frame = SemanticFrame::new(SemanticId(1));
    let child = gpui::div().id("dialog").into_element();
    let id = Element::id(&child);
    assert_eq!(frame.overlay_subtree(child).into_element().id(), id);
    assert_eq!(
        frame
            .deferred_subtree(SemanticId(3), frame.wrap(dialog(), gpui::div()), 7)
            .into_element()
            .id(),
        None
    );
}

#[test]
fn an_expected_deferred_root_must_actually_be_painted() {
    let frame = SemanticFrame::new(SemanticId(1));
    let generation = frame.begin_frame();
    frame.record(root(), false);
    frame.expect_overlay(SemanticId(3));
    assert!(matches!(
        frame.complete_after_draw(generation).unwrap(),
        Err(SemanticError::UnpaintedOverlay(SemanticId(3)))
    ));
}

#[test]
fn a_normal_paint_placeholder_cannot_satisfy_the_deferred_overlay_marker() {
    let frame = SemanticFrame::new(SemanticId(1));
    let generation = frame.begin_frame();
    frame.record(root(), false);
    frame.record(panel(), false);
    frame.record(dialog(), false);
    frame.expect_overlay(SemanticId(3));
    assert!(matches!(
        frame.complete_after_draw(generation).unwrap(),
        Err(SemanticError::UnpaintedOverlay(_))
    ));
}

#[test]
fn existing_semantic_overlay_subtree_does_not_add_another_root_wrapper() {
    use gpui::{Element, IntoElement};
    let frame = SemanticFrame::new(SemanticId(1));
    let semantic_child = frame.wrap(dialog(), gpui::div());
    let deferred = frame
        .deferred_subtree(SemanticId(3), semantic_child, 7)
        .into_element();
    assert_eq!(deferred.id(), None);
}

#[test]
fn superseded_post_draw_callback_cannot_publish_or_finish_the_current_paint() {
    let frame = SemanticFrame::new(SemanticId(1));
    let old = frame.begin_frame();
    let current = frame.begin_frame();
    frame.record(root(), false);
    frame.expect_overlay(SemanticId(3));
    assert!(frame.complete_after_draw(old).is_none());
    assert!(frame.is_current(&current));
    frame.record(panel(), false);
    {
        let _overlay = frame.overlay_scope();
        frame.record(dialog(), false);
    }
    let tree = frame.complete_after_draw(current).unwrap().unwrap();
    assert!(tree.node(SemanticId(3)).is_some());
}

#[test]
fn cross_context_post_draw_callback_cannot_consume_a_generation() {
    let owner = SemanticFrame::new(SemanticId(1));
    let other = SemanticFrame::new(SemanticId(1));
    let wrong = other.begin_frame();
    let current = owner.begin_frame();
    owner.record(root(), false);
    assert!(owner.complete_after_draw(wrong).is_none());
    assert!(owner.complete_after_draw(current).unwrap().is_ok());
}

#[test]
fn unwinding_an_overlay_rejects_the_frame_then_restores_normal_collection() {
    let frame = SemanticFrame::new(SemanticId(1));
    let generation = frame.begin_frame();
    frame.record(root(), false);
    let unwind = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _overlay = frame.overlay_scope();
        panic!("fixture overlay paint failure");
    }));
    assert!(unwind.is_err());
    assert!(matches!(
        frame.complete_after_draw(generation).unwrap(),
        Err(SemanticError::FramePostPaint)
    ));
    let generation = frame.begin_frame();
    frame.record(root(), false);
    frame.record(panel(), false);
    frame.expect_overlay(SemanticId(3));
    frame.record(dialog(), false);
    // A leaked overlay scope would let this normal-paint placeholder satisfy the marker.
    assert!(matches!(
        frame.complete_after_draw(generation).unwrap(),
        Err(SemanticError::UnpaintedOverlay(SemanticId(3)))
    ));
}

#[test]
fn dropped_overlay_is_not_retained_by_the_next_frame() {
    let frame = SemanticFrame::new(SemanticId(1));
    let first = frame.begin_frame();
    frame.record(root(), false);
    frame.record(panel(), false);
    {
        let _overlay = frame.overlay_scope();
        frame.record(dialog(), false);
    }
    assert!(frame.finish_frame(first).is_ok());
    let next = frame.begin_frame();
    frame.record(root(), false);
    frame.record(panel(), false);
    let tree = frame.complete_after_draw(next).unwrap().unwrap();
    assert!(tree.node(SemanticId(3)).is_none());
    assert!(tree.node(SemanticId(2)).unwrap().children().is_empty());
}
