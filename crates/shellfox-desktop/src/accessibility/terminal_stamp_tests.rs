use super::terminal_view_tests::{cell, document, frame, geometry};
use super::*;
use accesskit::{ActionData, ActionHandler, ActionRequest, TextPosition, TreeId};
use std::{
    collections::BTreeMap,
    sync::atomic::Ordering,
    time::{Duration, Instant},
};

fn tree(snapshot: TerminalSnapshot) -> SemanticTree {
    let mut root = snapshot.document;
    root.set_children(snapshot.runs.iter().map(|(id, _)| *id).collect::<Vec<_>>());
    let tree = SemanticTree::new(
        SemanticId(10),
        SemanticId(10),
        std::iter::once((SemanticId(10), root)).chain(snapshot.runs),
    )
    .unwrap();
    let stamps = snapshot
        .terminal_stamp
        .map(|stamp| BTreeMap::from([(SemanticId(10), stamp)]))
        .unwrap_or_default();
    tree.with_terminal_views(stamps).unwrap()
}
fn range() -> TextSelection {
    TextSelection {
        anchor: TextPosition {
            node: SemanticId(11),
            character_index: 0,
        },
        focus: TextPosition {
            node: SemanticId(11),
            character_index: 1,
        },
    }
}
fn request() -> ActionRequest {
    ActionRequest {
        action: Action::SetTextSelection,
        target_tree: TreeId::ROOT,
        target_node: SemanticId(10),
        data: Some(ActionData::SetTextSelection(range())),
    }
}

#[test]
fn admission_stamp_never_adopts_a_newer_document_when_native_node_data_is_equal() {
    let mut view = frame(4, vec![cell("a", 1); 4]);
    let mut doc = document(1);
    let now = Instant::now();
    let old = tree(doc.snapshot_view(&view, geometry(), false, now).unwrap());
    let old_stamp = old.terminal_view(SemanticId(10)).unwrap();
    let mut bridge = bridge::Bridge::default();
    bridge.replace(old.clone());
    bridge.do_action(request());
    view.version += 1;
    let new = tree(
        doc.snapshot_view(&view, geometry(), false, now + Duration::from_millis(50))
            .unwrap(),
    );
    assert!(new.same_native_tree(&old));
    assert!(new.delta(&old).nodes.is_empty());
    assert_ne!(new.terminal_view(SemanticId(10)), Some(old_stamp));
    bridge.replace(new);
    let queued = bridge.drain().pop().unwrap();
    assert_eq!(queued.terminal_stamp, Some(old_stamp));
    assert!(
        doc.selection_action_for_view(&range(), queued.terminal_stamp, &view)
            .is_none()
    );
}

#[test]
fn metadata_only_publication_is_used_by_new_admission_without_native_text_changes() {
    let mut view = frame(4, vec![cell("a", 1); 4]);
    let mut doc = document(1);
    let now = Instant::now();
    let mut bridge = bridge::Bridge::default();
    bridge.replace(tree(
        doc.snapshot_view(&view, geometry(), false, now).unwrap(),
    ));
    view.version = 33;
    bridge.replace(tree(
        doc.snapshot_view(&view, geometry(), false, now + Duration::from_millis(50))
            .unwrap(),
    ));
    bridge.do_action(request());
    let action = bridge.drain().pop().unwrap();
    assert_eq!(action.terminal_stamp.unwrap().version, 33);
    assert!(matches!(
        doc.selection_action_for_view(&range(), action.terminal_stamp, &view),
        Some(shellfox_runtime::PresentationAction::SelectionAtVersion { version: 33, .. })
    ));
}

#[test]
fn missing_terminal_stamp_rejects_admission_and_requests_a_new_publication() {
    let view = frame(4, vec![cell("a", 1); 4]);
    let mut doc = document(1);
    let snapshot = doc
        .snapshot_view(&view, geometry(), false, Instant::now())
        .unwrap();
    let mut missing = tree(snapshot);
    missing.terminal_views.clear();
    let mut bridge = bridge::Bridge::default();
    bridge.replace(missing);
    bridge.do_action(request());
    assert!(bridge.drain().is_empty());
    assert!(bridge.needs_full.load(Ordering::Acquire));
    assert!(
        doc.selection_action_for_view(&range(), None, &view)
            .is_none()
    );
}

#[test]
fn coalesced_text_publishes_its_sampled_stamp_not_a_newer_getter_version() {
    let mut view = frame(4, vec![cell("a", 1); 4]);
    let mut doc = document(1);
    let now = Instant::now();
    doc.snapshot_view(&view, geometry(), false, now).unwrap();
    view.version = 2;
    let snapshot = doc
        .snapshot_view(&view, geometry(), false, now + Duration::from_millis(1))
        .unwrap();
    assert_eq!(snapshot.terminal_stamp.unwrap().version, 1);
    let mut bridge = bridge::Bridge::default();
    bridge.replace(tree(snapshot));
    bridge.do_action(request());
    let action = bridge.drain().pop().unwrap();
    assert_eq!(action.terminal_stamp.unwrap().version, 1);
    assert!(
        doc.selection_action_for_view(&range(), action.terminal_stamp, &view)
            .is_none()
    );
}

#[test]
fn collector_metadata_resets_with_nodes_after_each_published_frame() {
    let view = frame(4, vec![cell("a", 1); 4]);
    let snapshot = document(1)
        .snapshot_view(&view, geometry(), false, Instant::now())
        .unwrap();
    let stamp = snapshot.terminal_stamp.unwrap();
    let frame = WindowSemanticContext::new(SemanticId(1)).frame();
    let generation = frame.begin_frame();
    frame.record(
        SemanticNode::new(SemanticId(1), None, Role::Window, "Shellfox"),
        false,
    );
    frame.record(
        SemanticNode {
            id: SemanticId(10),
            parent: Some(SemanticId(1)),
            data: snapshot.document,
            focus: None,
        },
        false,
    );
    for (id, data) in snapshot.runs {
        frame.record(
            SemanticNode {
                id,
                parent: Some(SemanticId(10)),
                data,
                focus: None,
            },
            false,
        );
    }
    frame.set_terminal_view(SemanticId(10), stamp).unwrap();
    assert_eq!(
        frame
            .finish_frame(generation)
            .unwrap()
            .terminal_view(SemanticId(10)),
        Some(stamp)
    );
    let generation = frame.begin_frame();
    frame.record(
        SemanticNode::new(SemanticId(1), None, Role::Window, "Shellfox"),
        false,
    );
    assert!(
        frame
            .finish_frame(generation)
            .unwrap()
            .terminal_view(SemanticId(10))
            .is_none()
    );
}

#[test]
fn conflicting_collection_stamps_fail_instead_of_retargeting_one_frame() {
    let view = frame(4, vec![cell("a", 1); 4]);
    let mut doc = document(1);
    let snapshot = doc
        .snapshot_view(&view, geometry(), false, Instant::now())
        .unwrap();
    let stamp = snapshot.terminal_stamp.unwrap();
    let frame = SemanticFrame::new(SemanticId(1));
    let generation = frame.begin_frame();
    frame.record(
        SemanticNode::new(SemanticId(1), None, Role::Window, "Shellfox"),
        false,
    );
    frame.set_terminal_view(SemanticId(10), stamp).unwrap();
    assert!(
        frame
            .set_terminal_view(
                SemanticId(10),
                TerminalViewStamp {
                    version: stamp.version + 1,
                    ..stamp
                }
            )
            .is_err()
    );
    assert!(matches!(
        frame.finish_frame(generation),
        Err(SemanticError::TerminalStamp(SemanticId(10)))
    ));
}

#[test]
fn rejected_collection_cannot_replace_the_last_published_admission_stamp() {
    let mut view = frame(4, vec![cell("a", 1); 4]);
    let mut doc = document(1);
    let now = Instant::now();
    let mut bridge = bridge::Bridge::default();
    let old = tree(doc.snapshot_view(&view, geometry(), false, now).unwrap());
    let published = old.terminal_view(SemanticId(10));
    bridge.replace(old);
    view.version += 1;
    let snapshot = doc
        .snapshot_view(&view, geometry(), false, now + Duration::from_millis(50))
        .unwrap();
    let collection = SemanticFrame::new(SemanticId(1));
    let generation = collection.begin_frame();
    collection
        .set_terminal_view(SemanticId(10), snapshot.terminal_stamp.unwrap())
        .unwrap();
    collection.reject(SemanticError::InvalidIdKey);
    assert!(collection.finish_frame(generation).is_err());
    bridge.do_action(request());
    let queued = bridge.drain().pop().unwrap();
    assert_eq!(queued.terminal_stamp, published);
    assert!(
        doc.selection_action_for_view(&range(), queued.terminal_stamp, &view)
            .is_none()
    );
}

#[test]
fn stamps_are_private_side_data_and_cannot_be_bound_to_field_nodes() {
    let view = frame(4, vec![cell("a", 1); 4]);
    let mut bare = tree(
        document(1)
            .snapshot_view(&view, geometry(), false, Instant::now())
            .unwrap(),
    );
    let stamp = bare.terminal_view(SemanticId(10)).unwrap();
    let nodes = bare.nodes.clone();
    bare.terminal_views.clear();
    let stamped = bare
        .clone()
        .with_terminal_views(BTreeMap::from([(SemanticId(10), stamp)]))
        .unwrap();
    assert_eq!(stamped.nodes, nodes); // No name/value/className or other native property changed.
    bare.nodes
        .get_mut(&SemanticId(10))
        .unwrap()
        .set_role(Role::MultilineTextInput);
    assert!(matches!(
        bare.with_terminal_views(BTreeMap::from([(SemanticId(10), stamp)])),
        Err(SemanticError::TerminalStamp(SemanticId(10)))
    ));
}

#[test]
fn field_text_selection_retains_its_existing_model_guard_without_terminal_metadata() {
    let view = frame(4, vec![cell("a", 1); 4]);
    let mut source = tree(
        document(1)
            .snapshot_view(&view, geometry(), false, Instant::now())
            .unwrap(),
    );
    source
        .nodes
        .get_mut(&SemanticId(10))
        .unwrap()
        .set_role(Role::MultilineTextInput);
    source.terminal_views.clear();
    let mut bridge = bridge::Bridge::default();
    bridge.replace(source);
    bridge.do_action(request());
    let action = bridge.drain().pop().unwrap();
    assert!(action.terminal_stamp.is_none());
}
