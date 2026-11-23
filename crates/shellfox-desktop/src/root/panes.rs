//! Cached views for the sidebar, the tab strip and the Files pane.
//!
//! GPUI re-renders a notifying view and its ancestors, but reuses the last frame of any
//! clean cached view below them. Each pane re-renders when Root notifies, so a change that
//! only touches a terminal (cursor blink, output) no longer rebuilds these panes.
//!
//! So a pane may only show Root state changed with `cx.notify()` outside of drawing, or
//! child views it renders itself. Root forwards changes of other entities a pane reads (a
//! terminal's phase, see workspace/attach.rs) by notifying.
use super::Root;
use crate::accessibility::SemanticCache;
use gpui::{
    AnyElement, AnyView, Context, Entity, IntoElement, Render, StyleRefinement, Subscription,
    WeakEntity, Window, div, prelude::*,
};

#[derive(Clone, Copy)]
pub(crate) enum PaneKind {
    Sidebar,
    TabStrip,
    Files,
}

pub(crate) struct Pane {
    root: WeakEntity<Root>,
    kind: PaneKind,
    semantics: SemanticCache,
    _root_changes: Subscription,
}

impl Pane {
    fn new(root: &Entity<Root>, kind: PaneKind, cx: &mut Context<Self>) -> Self {
        Self {
            root: root.downgrade(),
            kind,
            semantics: SemanticCache::default(),
            _root_changes: cx.observe(root, |_, _, cx| cx.notify()),
        }
    }
}

impl Render for Pane {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.semantics.rendered();
        let kind = self.kind;
        self.root
            .update(cx, |root, cx| root.render_pane(kind, window, cx))
            .unwrap_or_else(|_| div().into_any_element())
    }
}

pub(crate) struct Panes {
    sidebar: Entity<Pane>,
    tab_strip: Entity<Pane>,
    files: Entity<Pane>,
}

impl Panes {
    pub(crate) fn new(cx: &mut Context<Root>) -> Self {
        let root = cx.entity();
        Self {
            sidebar: cx.new(|cx| Pane::new(&root, PaneKind::Sidebar, cx)),
            tab_strip: cx.new(|cx| Pane::new(&root, PaneKind::TabStrip, cx)),
            files: cx.new(|cx| Pane::new(&root, PaneKind::Files, cx)),
        }
    }
}

impl Root {
    /// The pane as a cached element that fills its parent.
    pub(crate) fn pane(&self, kind: PaneKind, cx: &gpui::App) -> AnyElement {
        let pane = match kind {
            PaneKind::Sidebar => &self.panes.sidebar,
            PaneKind::TabStrip => &self.panes.tab_strip,
            PaneKind::Files => &self.panes.files,
        };
        let cache = pane.read(cx).semantics.clone();
        let view = AnyView::from(pane.clone()).cached(StyleRefinement::default().size_full());
        self.semantics.context.frame().cached(&cache, view).into_any_element()
    }

    fn render_pane(&mut self, kind: PaneKind, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let palette = self.palette(cx);
        match kind {
            PaneKind::Sidebar => self.session_sidebar(palette, window, cx),
            PaneKind::TabStrip => self.tab_strip(palette, window, cx),
            PaneKind::Files => self.files_pane(palette, window, cx),
        }
    }
}
