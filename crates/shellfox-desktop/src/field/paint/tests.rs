#[test]
fn temporary_large_editor_coverage_does_not_advertise_a_partial_text_pattern() {
    use crate::accessibility::{Action, Role, SemanticFrame, SemanticId};
    let binding = super::semantics::Binding {
        frame: SemanticFrame::new(SemanticId(1)),
        id: SemanticId(2),
        parent: SemanticId(1),
        name: "File contents".into(),
        key: "field/test".into(),
    };
    let node = super::document::unavailable_document(&binding);
    assert_eq!(node.data.role(), Role::Group);
    assert!(node.data.value().is_none());
    assert!(node.data.text_selection().is_none());
    assert!(node.data.bounds().is_none());
    for action in [
        Action::SetValue,
        Action::ReplaceSelectedText,
        Action::SetTextSelection,
    ] {
        assert!(!node.data.supports_action(action));
    }
    assert!(
        node.data
            .description()
            .is_some_and(|description| description.contains("unavailable"))
    );
}
