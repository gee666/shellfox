use super::{SemanticError, SemanticFrame, SemanticNode, SemanticTree};
use accesskit::Rect;
use gpui::{
    App, Bounds, Element, ElementId, GlobalElementId, InspectorElementId, IntoElement, LayoutId,
    Pixels, Window,
};
use std::{cell::RefCell, rc::Rc};

impl SemanticFrame {
    pub fn wrap<E: IntoElement>(
        &self,
        node: SemanticNode,
        child: E,
    ) -> SemanticElement<E::Element> {
        SemanticElement {
            child: child.into_element(),
            node,
            frame: self.clone(),
        }
    }

    /// Reset before root paint; publish at the end of the effect cycle, after ALL overlays.
    /// Semantic views must stay uncached. Deferred/prompt/tooltip children paint later.
    pub fn finish<E: IntoElement>(
        &self,
        child: E,
        commit: impl FnMut(Result<SemanticTree, SemanticError>, &mut Window, &mut App) + 'static,
    ) -> impl IntoElement {
        FinishElement {
            child: child.into_element(),
            frame: self.clone(),
            commit: Rc::new(RefCell::new(Box::new(commit))),
        }
    }
}

pub struct SemanticElement<E> {
    child: E,
    node: SemanticNode,
    frame: SemanticFrame,
}

impl<E: Element> IntoElement for SemanticElement<E> {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl<E: Element> Element for SemanticElement<E> {
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
        let mut clipped = bounds.intersect(&window.content_mask().bounds);
        clipped.size.width = clipped.size.width.max(gpui::px(0.));
        clipped.size.height = clipped.size.height.max(gpui::px(0.));
        let mut data = self.node.data.clone();
        data.set_bounds(native_rect(clipped, window));
        if clipped.size.width <= gpui::px(0.) || clipped.size.height <= gpui::px(0.) {
            data.set_hidden();
        }
        self.frame.insert_measured(
            SemanticNode {
                id: self.node.id,
                parent: self.node.parent,
                data,
                focus: self.node.focus.clone(),
            },
            window,
        );
        self.child
            .paint(id, inspector, bounds, state, prepaint, window, cx);
    }
}

/// AccessKit macOS uses points; Windows and AT-SPI use physical pixels.
pub(crate) fn native_rect(bounds: Bounds<Pixels>, window: &Window) -> Rect {
    let scale = if cfg!(target_os = "macos") {
        1.
    } else {
        f64::from(window.scale_factor())
    };
    Rect::new(
        f64::from(f32::from(bounds.left())) * scale,
        f64::from(f32::from(bounds.top())) * scale,
        f64::from(f32::from(bounds.right())) * scale,
        f64::from(f32::from(bounds.bottom())) * scale,
    )
}

type Commit = Box<dyn FnMut(Result<SemanticTree, SemanticError>, &mut Window, &mut App)>;
struct FinishElement<E> {
    child: E,
    frame: SemanticFrame,
    commit: Rc<RefCell<Commit>>,
}
impl<E: Element> IntoElement for FinishElement<E> {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}
impl<E: Element> Element for FinishElement<E> {
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
        let generation = self.frame.begin_frame();
        self.child
            .paint(id, inspector, bounds, state, prepaint, window, cx);
        let frame = self.frame.clone();
        let commit = self.commit.clone();
        // Window::defer runs after the Window borrow/draw stack is returned to App.
        // A max-priority Deferred sentinel is too early for GPUI prompts and tooltips.
        window.defer(cx, move |window, cx| {
            if let Some(result) = frame.complete_after_draw(generation) {
                (commit.borrow_mut())(result, window, cx);
            }
        });
    }
}
