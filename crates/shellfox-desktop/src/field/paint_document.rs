use super::{Field, semantics};
use gpui::{AnyElement, IntoElement};

pub(super) fn unavailable_document(
    binding: &semantics::Binding,
) -> crate::accessibility::SemanticNode {
    let mut node = crate::accessibility::SemanticNode::new(
        binding.id,
        Some(binding.parent),
        crate::accessibility::Role::Group,
        binding.name.clone(),
    );
    node.data.set_description(
        "Large-document text accessibility is unavailable; keyboard editing remains available.",
    );
    node
}

pub(super) fn document(
    field: &Field,
    state: &semantics::TextState,
    virtualized: bool,
) -> Option<Result<crate::accessibility::SemanticNode, crate::accessibility::SemanticError>> {
    field.semantic.as_ref().map(|binding| {
        if virtualized {
            // This is NOT a partial TextPattern. Full large-document AT is a separate reviewed producer.
            let mut node = unavailable_document(binding);
            node.focus = Some(field.focus.clone());
            Ok(node)
        } else {
            binding.document(
                state,
                field.secret,
                field.multiline,
                &field.placeholder,
                field.focus.clone(),
            )
        }
    })
}

pub(super) fn wrap(
    binding: Option<semantics::Binding>,
    node: Option<Result<crate::accessibility::SemanticNode, crate::accessibility::SemanticError>>,
    element: impl IntoElement,
) -> AnyElement {
    match (binding, node) {
        (Some(binding), Some(Ok(node))) => binding.frame.wrap(node, element).into_any_element(),
        (Some(binding), Some(Err(error))) => {
            binding.frame.reject(error);
            element.into_any_element()
        }
        _ => element.into_any_element(),
    }
}
