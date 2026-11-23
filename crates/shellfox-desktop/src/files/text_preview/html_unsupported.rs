//! Platforms without an embedded webview always show HTML as source, with a notice.
use crate::{accessibility::SemanticFrame, files::DocumentBackend, root::Root, theme::Palette};
use gpui::{AnyElement, Context, Window, div, prelude::*};
use std::sync::Arc;

pub(crate) struct Surface;

impl Surface {
    pub fn new(_: &std::path::Path) -> Self {
        Self
    }

    pub fn available(&self, _: &Window) -> bool {
        false
    }

    pub fn begin_frame(&self) {}

    pub fn place(&self) {}

    pub fn end_frame(&self) {}

    pub fn begin_render(&self) {}

    pub fn end_render(&self, _: bool) {}
}

pub(super) struct HtmlPreview;

impl HtmlPreview {
    pub fn load(_: DocumentBackend, _: String, _: Arc<str>, _: &mut Context<Root>) -> Self {
        Self
    }

    pub fn render(&self, _: &Surface, _: Palette, _: &SemanticFrame) -> AnyElement {
        div().into_any_element()
    }
}
