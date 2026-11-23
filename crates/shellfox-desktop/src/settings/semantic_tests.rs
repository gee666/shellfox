use super::*;
use crate::accessibility::{Role, SemanticFrame, SemanticId, SemanticNode};
use gpui::Element;
use std::{cell::Cell, rc::Rc};
#[test]
fn semantic_control_keeps_original_element_identity_and_dimensions() {
    let mut element = div()
        .id("settings-semantic-test")
        .w(px(22.))
        .h(px(28.))
        .border_1();
    let width = element.style().size.width;
    let height = element.style().size.height;
    let frame = SemanticFrame::new(SemanticId(1));
    let id = frame.id_for("settings/test/control").unwrap();
    let mut control = semantic_control::Control::new(
        element,
        Some((
            frame,
            SemanticNode::new(id, Some(SemanticId(1)), Role::Button, "Test"),
        )),
        Rc::new(Cell::new(true)),
    );
    assert_eq!(control.style().size.width, width);
    assert_eq!(control.style().size.height, height);
    assert_eq!(
        control.into_element().id(),
        Some(gpui::ElementId::from("settings-semantic-test"))
    );
}
#[test]
fn disabling_semantics_also_closes_the_shared_input_gate() {
    let gate = Rc::new(Cell::new(true));
    let control = semantic_control::Control::new(div().id("settings-gate"), None, gate.clone())
        .disabled(true)
        .disabled(false);
    assert!(!gate.get());
    drop(control);
}
#[test]
fn semantic_keys_are_window_local_exact_ids_not_hashes() {
    let frame = SemanticFrame::new(SemanticId(1));
    let same = frame.clone();
    assert!(frame.same_context(&same));
    assert_eq!(
        frame.id_for("settings/control/profile-wsl:Ubuntu").unwrap(),
        same.id_for("settings/control/profile-wsl:Ubuntu").unwrap()
    );
    assert_ne!(
        frame.id_for("settings/control/a/b").unwrap(),
        frame.id_for("settings/control/a:b").unwrap()
    );
}
