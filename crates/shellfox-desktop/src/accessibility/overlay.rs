use super::SemanticFrame;
use accesskit::NodeId;
use gpui::{
    App, Bounds, Element, ElementId, GlobalElementId, InspectorElementId, IntoElement, LayoutId,
    Pixels, Window,
};

enum Boundary {
    Paint,
    DeferredRegistration(NodeId),
}

impl SemanticFrame {
    /// Wrap the actual dialog/overlay child, retaining its existing semantic root.
    pub fn overlay_subtree<E: IntoElement>(&self, child: E) -> impl IntoElement {
        OverlayElement {
            child: child.into_element(),
            frame: self.clone(),
            boundary: Boundary::Paint,
        }
    }

    /// Use for an existing semantic subtree. The ID must be measured during actual overlay paint.
    pub fn deferred_subtree<E: IntoElement>(
        &self,
        root: NodeId,
        child: E,
        priority: usize,
    ) -> impl IntoElement {
        let child = gpui::deferred(self.overlay_subtree(child)).with_priority(priority);
        OverlayElement {
            child,
            frame: self.clone(),
            boundary: Boundary::DeferredRegistration(root),
        }
    }
}

struct OverlayElement<E> {
    child: E,
    frame: SemanticFrame,
    boundary: Boundary,
}
impl<E: Element> IntoElement for OverlayElement<E> {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}
impl<E: Element> Element for OverlayElement<E> {
    type RequestLayoutState = E::RequestLayoutState;
    type PrepaintState = E::PrepaintState;
    fn id(&self) -> Option<ElementId> {
        self.child.id()
    }
    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        self.child.source_location()
    }
    fn request_layout(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        self.child.request_layout(id, inspector, window, cx)
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
        self.child
            .prepaint(id, inspector, bounds, state, window, cx)
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
        let overlay = matches!(self.boundary, Boundary::Paint);
        if let Boundary::DeferredRegistration(root) = self.boundary {
            self.frame.expect_overlay(root);
        }
        let _scope = overlay.then(|| self.frame.overlay_scope());
        self.child
            .paint(id, inspector, bounds, state, prepaint, window, cx);
    }
}
