use crate::accessibility::SemanticElement;
use gpui::{
    App, Bounds, Div, Element, ElementId, GlobalElementId, InspectorElementId, IntoElement,
    LayoutId, Pixels, Stateful, Window,
};
/// Unlike AnyElement erasure, this boundary retains the child's exposed ID and
/// forwards its exact layout/prepaint state in both presentation modes.
pub(super) enum ControlElement {
    Plain(Box<Stateful<Div>>),
    Measured(Box<SemanticElement<Stateful<Div>>>),
}
impl IntoElement for ControlElement {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}
impl Element for ControlElement {
    type RequestLayoutState = <Stateful<Div> as Element>::RequestLayoutState;
    type PrepaintState = <Stateful<Div> as Element>::PrepaintState;
    fn id(&self) -> Option<ElementId> {
        match self {
            Self::Plain(child) => Element::id(child.as_ref()),
            Self::Measured(child) => Element::id(child.as_ref()),
        }
    }
    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        match self {
            Self::Plain(child) => child.source_location(),
            Self::Measured(child) => child.source_location(),
        }
    }
    fn request_layout(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        match self {
            Self::Plain(child) => child.request_layout(id, inspector, window, cx),
            Self::Measured(child) => child.request_layout(id, inspector, window, cx),
        }
    }
    fn prepaint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        state: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        match self {
            Self::Plain(child) => child.prepaint(id, inspector, bounds, state, window, cx),
            Self::Measured(child) => child.prepaint(id, inspector, bounds, state, window, cx),
        }
    }
    fn paint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        state: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        match self {
            Self::Plain(child) => child.paint(id, inspector, bounds, state, prepaint, window, cx),
            Self::Measured(child) => {
                child.paint(id, inspector, bounds, state, prepaint, window, cx)
            }
        }
    }
}
