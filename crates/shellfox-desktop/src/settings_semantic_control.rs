use crate::accessibility::{Role, SemanticFrame, SemanticNode};
use gpui::{AnyElement, Div, Interactivity, Stateful, StyleRefinement, prelude::*};
use std::{cell::Cell, rc::Rc};

/// Forwards the existing control builder unchanged. Only final conversion adds
/// a layout-neutral paint wrapper, after every caller has applied its styles.
pub(super) struct Control {
    element: Stateful<Div>,
    semantic: Option<(SemanticFrame, SemanticNode)>,
    enabled: Rc<Cell<bool>>,
}
impl Control {
    pub fn new(
        element: Stateful<Div>,
        semantic: Option<(SemanticFrame, SemanticNode)>,
        enabled: Rc<Cell<bool>>,
    ) -> Self {
        Self {
            element,
            semantic,
            enabled,
        }
    }
    pub fn value(mut self, value: impl Into<String>) -> Self {
        if let Some((_, node)) = &mut self.semantic {
            node.data.set_value(value.into());
        }
        self
    }
    pub fn role(mut self, role: Role) -> Self {
        if let Some((_, node)) = &mut self.semantic {
            node.data.set_role(role);
        }
        self
    }
    pub fn parent(mut self, parent: crate::accessibility::SemanticId) -> Self {
        if let Some((_, node)) = &mut self.semantic {
            node.parent = Some(parent);
        }
        self
    }
    pub fn selected(mut self, selected: bool) -> Self {
        if let Some((_, node)) = &mut self.semantic {
            node.data.set_selected(selected);
        }
        self
    }
    pub fn pressed(mut self, checked: bool) -> Self {
        if let Some((_, node)) = &mut self.semantic {
            node.data.set_toggled(if checked {
                accesskit::Toggled::True
            } else {
                accesskit::Toggled::False
            });
        }
        self
    }
    pub fn expanded(mut self, expanded: bool) -> Self {
        if let Some((_, node)) = &mut self.semantic {
            node.data.set_expanded(expanded);
            node.data.add_action(accesskit::Action::Expand);
            node.data.add_action(accesskit::Action::Collapse);
        }
        self
    }
    pub fn disabled(mut self, disabled: bool) -> Self {
        if disabled {
            self.enabled.set(false);
            if let Some((_, node)) = &mut self.semantic {
                node.data.set_disabled();
            }
        }
        self
    }
}
impl Styled for Control {
    fn style(&mut self) -> &mut StyleRefinement {
        self.element.style()
    }
}
impl InteractiveElement for Control {
    fn interactivity(&mut self) -> &mut Interactivity {
        self.element.interactivity()
    }
}
impl StatefulInteractiveElement for Control {}
impl ParentElement for Control {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.element.extend(elements)
    }
}
impl IntoElement for Control {
    type Element = super::semantic_element::ControlElement;
    fn into_element(self) -> Self::Element {
        match self.semantic {
            Some((frame, node)) => super::semantic_element::ControlElement::Measured(Box::new(
                frame.wrap(node, self.element),
            )),
            None => super::semantic_element::ControlElement::Plain(Box::new(self.element)),
        }
    }
}
