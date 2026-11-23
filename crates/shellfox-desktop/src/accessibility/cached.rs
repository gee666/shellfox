//! Semantic nodes of cached GPUI views. GPUI reuses a clean cached view's previous paint
//! without running it, so the nodes that paint measured are recorded and replayed with it.
use super::{SemanticFrame, SemanticNode};
use gpui::{
    App, Bounds, Element, ElementId, GlobalElementId, InspectorElementId, IntoElement, LayoutId,
    Pixels, Window,
};
use std::{cell::RefCell, rc::Rc};

/// Owned by one cached view. Its render calls `rendered`, so the next paint measures again.
#[derive(Clone, Default)]
pub struct SemanticCache(Rc<RefCell<CacheState>>);

#[derive(Default)]
struct CacheState {
    rendered: bool,
    nodes: Vec<SemanticNode>,
    /// The error the measured paint hit; a replay of its partial nodes reports it again.
    error: Option<super::SemanticError>,
}

impl SemanticCache {
    pub fn rendered(&self) {
        self.0.borrow_mut().rendered = true;
    }
}

impl SemanticFrame {
    /// Wraps a cached view (`AnyView::cached`) whose subtree holds semantic nodes.
    /// Bounds are part of GPUI's cache key, so replayed nodes are still where they were painted.
    /// Deferred overlays paint after the view and are not replayed; keep them outside.
    pub fn cached<E: IntoElement>(
        &self,
        cache: &SemanticCache,
        child: E,
    ) -> CachedSemantics<E::Element> {
        CachedSemantics {
            child: child.into_element(),
            cache: cache.clone(),
            frame: self.clone(),
        }
    }
}

impl SemanticFrame {
    /// After the cached child painted: keep what a fresh paint measured, or replay the last.
    pub(super) fn settle_cached(
        &self,
        cache: &SemanticCache,
        start: usize,
        error_before: bool,
        focused: impl Fn(&gpui::FocusHandle) -> bool,
    ) {
        let mut cache = cache.0.borrow_mut();
        if std::mem::take(&mut cache.rendered) {
            cache.nodes = self.recorded_since(start);
            cache.error = if error_before {
                None
            } else {
                self.collecting_error()
            };
        } else {
            if let Some(error) = cache.error.clone() {
                self.record_error(error);
            }
            for node in &cache.nodes {
                let focused = node.focus.as_ref().is_some_and(&focused);
                self.record(node.clone(), focused);
            }
        }
    }
}

pub struct CachedSemantics<E> {
    child: E,
    cache: SemanticCache,
    frame: SemanticFrame,
}

impl<E: Element> IntoElement for CachedSemantics<E> {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl<E: Element> Element for CachedSemantics<E> {
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
        let start = self.frame.recorded();
        let error_before = self.frame.collecting_error().is_some();
        self.child
            .paint(id, inspector, bounds, state, prepaint, window, cx);
        self.frame
            .settle_cached(&self.cache, start, error_before, |handle| {
                handle.is_focused(window)
            });
    }
}
