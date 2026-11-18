use super::*;
use accesskit::{ActionData, ActionHandler, ActionRequest, ActivationHandler, Rect, TreeId};
use std::time::{Duration, Instant};

#[path = "deferred_tests.rs"]
mod deferred_tests;
#[path = "frame_tests.rs"]
mod frame_tests;
#[path = "terminal_selection_tests.rs"]
mod terminal_selection_tests;
#[path = "terminal_stamp_tests.rs"]
mod terminal_stamp_tests;
#[path = "terminal_tests.rs"]
mod terminal_tests;
#[path = "terminal_view_tests.rs"]
mod terminal_view_tests;

fn button(id: u64) -> (SemanticId, Node) {
    let mut node = Node::new(Role::Button);
    node.set_label("New session");
    node.add_action(Action::Click);
    (SemanticId(id), node)
}

fn tree() -> SemanticTree {
    let mut root = Node::new(Role::Window);
    root.set_children([SemanticId(2)]);
    SemanticTree::new(
        SemanticId(1),
        SemanticId(1),
        [(SemanticId(1), root), button(2)],
    )
    .unwrap()
}

fn click() -> ActionRequest {
    ActionRequest {
        action: Action::Click,
        target_tree: TreeId::ROOT,
        target_node: SemanticId(2),
        data: None,
    }
}

#[test]
fn validates_graph_and_real_geometry() {
    let mut root = Node::new(Role::Window);
    root.set_children([SemanticId(2)]);
    assert!(matches!(
        SemanticTree::new(SemanticId(1), SemanticId(1), [(SemanticId(1), root)]),
        Err(SemanticError::MissingNode(SemanticId(2)))
    ));
    let mut invalid = Node::new(Role::Window);
    invalid.set_bounds(Rect::new(0., 0., f64::NAN, 0.));
    assert!(matches!(
        SemanticTree::new(SemanticId(1), SemanticId(1), [(SemanticId(1), invalid)]),
        Err(SemanticError::InvalidBounds(_))
    ));
    assert!(matches!(
        SemanticTree::new(SemanticId(1), SemanticId(1), [button(1), button(1)]),
        Err(SemanticError::DuplicateId(_))
    ));
}

#[test]
fn rejects_unreachable_cycles_and_shared_children() {
    let mut root = Node::new(Role::Window);
    root.set_children([SemanticId(2), SemanticId(2)]);
    assert!(matches!(
        SemanticTree::new(
            SemanticId(1),
            SemanticId(1),
            [(SemanticId(1), root), button(2)]
        ),
        Err(SemanticError::MultipleParents(_))
    ));
    let mut cyclic = Node::new(Role::Window);
    cyclic.set_children([SemanticId(1)]);
    assert!(matches!(
        SemanticTree::new(SemanticId(1), SemanticId(1), [(SemanticId(1), cyclic)]),
        Err(SemanticError::Cycle(_))
    ));
    assert!(matches!(
        SemanticTree::new(SemanticId(1), SemanticId(1), [button(1), button(2)]),
        Err(SemanticError::Unreachable(_))
    ));
}

#[test]
fn unchanged_nodes_do_not_generate_native_events() {
    let old = tree();
    let same = old.delta(&old);
    assert!(same.nodes.is_empty());
    assert!(same.tree.is_none());
    let mut next = old.clone();
    next.nodes
        .get_mut(&SemanticId(2))
        .unwrap()
        .set_label("New terminal");
    let update = next.delta(&old);
    assert_eq!(update.nodes.len(), 1);
    assert_eq!(update.nodes[0].0, SemanticId(2));
}

#[test]
fn removals_update_the_parent_and_never_reference_missing_nodes() {
    let old = tree();
    let mut root = Node::new(Role::Window);
    root.set_label("Shellfox");
    let next = SemanticTree::new(SemanticId(1), SemanticId(1), [(SemanticId(1), root)]).unwrap();
    let update = next.delta(&old);
    assert_eq!(update.nodes.len(), 1);
    assert!(update.nodes[0].1.children().is_empty());
}

#[test]
fn action_queue_is_bounded_and_revalidates_disabled_or_removed_targets() {
    let mut bridge = bridge::Bridge::default();
    bridge.replace(tree());
    for _ in 0..1000 {
        bridge.do_action(click());
    }
    assert_eq!(bridge.drain().len(), 64);
    bridge.do_action(click());
    let mut next = tree();
    next.nodes.get_mut(&SemanticId(2)).unwrap().set_disabled();
    bridge.replace(next);
    assert!(bridge.drain().is_empty());
    bridge.do_action(click());
    assert!(bridge.drain().is_empty());
    bridge.replace(tree());
    bridge.do_action(click());
    bridge.close();
    assert!(bridge.drain().is_empty());
    assert!(bridge.request_initial_tree().is_none());
}

#[test]
fn rejects_unsupported_actions_and_wrong_tree_or_payload() {
    let mut bridge = bridge::Bridge::default();
    bridge.replace(tree());
    let mut request = click();
    request.action = Action::SetValue;
    request.data = Some(ActionData::Value("danger".into()));
    bridge.do_action(request);
    let mut request = click();
    request.data = Some(ActionData::Value("invalid click payload".into()));
    bridge.do_action(request);
    let mut request = click();
    request.target_node = SemanticId(999);
    bridge.do_action(request);
    assert!(bridge.drain().is_empty());
}

#[test]
fn activation_always_returns_a_full_current_tree() {
    let mut bridge = bridge::Bridge::default();
    assert!(bridge.request_initial_tree().is_none());
    bridge.replace(tree());
    for _ in 0..2 {
        let update = bridge.request_initial_tree().unwrap();
        assert_eq!(update.nodes.len(), 2);
        assert!(update.tree.is_some());
    }
}

#[test]
fn hidden_ancestors_reject_queued_and_new_actions() {
    let mut bridge = bridge::Bridge::default();
    bridge.replace(tree());
    bridge.do_action(click());
    let mut next = tree();
    next.nodes.get_mut(&SemanticId(1)).unwrap().set_hidden();
    bridge.replace(next);
    assert!(bridge.drain().is_empty());
    bridge.do_action(click());
    assert!(bridge.drain().is_empty());
}

#[test]
fn text_selection_requests_reject_out_of_range_and_changed_text_runs() {
    let mut document = Node::new(Role::Document);
    document.set_children([SemanticId(2)]);
    document.add_action(Action::SetTextSelection);
    let mut run = Node::new(Role::TextRun);
    run.set_value("old");
    run.set_character_lengths([1, 1, 1]);
    let old = SemanticTree::new(
        SemanticId(1),
        SemanticId(1),
        [(SemanticId(1), document), (SemanticId(2), run)],
    )
    .unwrap();
    let mut bridge = bridge::Bridge::default();
    bridge.replace(old.clone());
    let request = |index| ActionRequest {
        action: Action::SetTextSelection,
        target_tree: TreeId::ROOT,
        target_node: SemanticId(1),
        data: Some(ActionData::SetTextSelection(TextSelection {
            anchor: accesskit::TextPosition {
                node: SemanticId(2),
                character_index: 0,
            },
            focus: accesskit::TextPosition {
                node: SemanticId(2),
                character_index: index,
            },
        })),
    };
    bridge.do_action(request(99));
    assert!(bridge.drain().is_empty());
    bridge.do_action(request(3));
    let mut changed = old;
    changed
        .nodes
        .get_mut(&SemanticId(2))
        .unwrap()
        .set_value("new");
    bridge.replace(changed);
    assert!(bridge.drain().is_empty());
}

#[test]
fn numeric_and_text_edits_require_finite_values_and_writable_controls() {
    let mut input = Node::new(Role::SpinButton);
    input.add_action(Action::SetValue);
    let make =
        |node| SemanticTree::new(SemanticId(1), SemanticId(1), [(SemanticId(1), node)]).unwrap();
    let mut bridge = bridge::Bridge::default();
    bridge.replace(make(input.clone()));
    let request = |data| ActionRequest {
        action: Action::SetValue,
        target_tree: TreeId::ROOT,
        target_node: SemanticId(1),
        data: Some(data),
    };
    bridge.do_action(request(ActionData::NumericValue(f64::NAN)));
    bridge.do_action(request(ActionData::Value(
        "x".repeat(8 * 1024 * 1024 + 1).into(),
    )));
    assert!(bridge.drain().is_empty());
    bridge.do_action(request(ActionData::NumericValue(42.)));
    assert_eq!(
        bridge.drain()[0].kind,
        SemanticActionKind::SetNumericValue(42.)
    );
    input.set_read_only();
    bridge.replace(make(input));
    bridge.do_action(request(ActionData::Value("change".into())));
    assert!(bridge.drain().is_empty());
}

#[test]
fn edit_queue_bounds_payload_and_expected_text_bytes_not_just_request_count() {
    let mut input = Node::new(Role::MultilineTextInput);
    input.add_action(Action::SetValue);
    input.set_value("a".repeat(1024 * 1024));
    let mut bridge = bridge::Bridge::default();
    bridge.replace(
        SemanticTree::new(SemanticId(1), SemanticId(1), [(SemanticId(1), input)]).unwrap(),
    );
    for _ in 0..20 {
        bridge.do_action(ActionRequest {
            action: Action::SetValue,
            target_tree: TreeId::ROOT,
            target_node: SemanticId(1),
            data: Some(ActionData::Value("b".repeat(1024 * 1024).into())),
        });
    }
    assert_eq!(bridge.drain().len(), 8);
    bridge.do_action(ActionRequest {
        action: Action::SetValue,
        target_tree: TreeId::ROOT,
        target_node: SemanticId(1),
        data: Some(ActionData::Value("next".into())),
    });
    assert_eq!(bridge.drain().len(), 1);
}

#[test]
fn wrappers_preserve_the_child_element_identity() {
    use gpui::{Element, InteractiveElement, IntoElement};
    let frame = SemanticFrame::new(SemanticId(1));
    let child = gpui::div().id("same-control").into_element();
    let id = Element::id(&child);
    let source = child.source_location();
    let wrapper: SemanticElement<gpui::Stateful<gpui::Div>> = frame.wrap(
        SemanticNode::new(SemanticId(1), None, Role::Button, "New session"),
        child,
    );
    assert_eq!(wrapper.id(), id);
    assert_eq!(wrapper.source_location(), source);
    // GPUI's erased outer element reports None; the typed child's identity above is unchanged.
    let erased = wrapper.into_any_element();
    assert_eq!(Element::id(&erased), None);
}

#[test]
fn unicode_runs_require_character_boundaries_and_matching_byte_lengths() {
    let mut run = Node::new(Role::TextRun);
    run.set_value("雪");
    run.set_character_lengths([1, 2]);
    assert!(matches!(
        SemanticTree::new(SemanticId(1), SemanticId(1), [(SemanticId(1), run)]),
        Err(SemanticError::InvalidText(_))
    ));
}
