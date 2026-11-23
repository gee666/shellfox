use crate::{files::DocumentBackend, root::Root, theme::Palette};
use gpui::{AnyElement, Context, Image, StyledImage, div, img, prelude::*, px};
use std::{cell::RefCell, sync::Arc};
#[cfg(any(windows, target_os = "macos"))]
pub(crate) mod html;
#[cfg(not(any(windows, target_os = "macos")))]
mod html_unsupported;
mod markdown;
mod markdown_image;
mod svg;
#[cfg(not(any(windows, target_os = "macos")))]
pub(crate) use html_unsupported as html;

#[derive(Clone, Copy)]
enum Kind {
    Markdown,
    Svg,
    Html,
}

enum Content {
    Markdown(markdown::Markdown, markdown::Images),
    Svg(Arc<Image>),
    Html(html::HtmlPreview),
    Error,
}

pub(crate) struct TextPreview {
    kind: Kind,
    pub showing: bool,
    cached: RefCell<Option<(Arc<str>, Content)>>,
}

impl TextPreview {
    pub fn for_path(path: &str) -> Option<Self> {
        let (_, extension) = path.rsplit_once('.')?;
        let kind = match extension.to_ascii_lowercase().as_str() {
            "md" | "mdx" | "markdown" | "mdown" | "mkd" => Kind::Markdown,
            "svg" => Kind::Svg,
            "html" | "htm" | "xhtml" => Kind::Html,
            _ => return None,
        };
        Some(Self {
            kind,
            showing: true,
            cached: RefCell::default(),
        })
    }

    pub fn available(&self, surface: &html::Surface, window: &gpui::Window) -> bool {
        !matches!(self.kind, Kind::Html) || surface.available(window)
    }

    pub fn render(
        &self,
        PreviewDocument {
            path,
            backend,
            text,
        }: PreviewDocument<'_>,
        palette: Palette,
        window: &gpui::Window,
        root: &Root,
        cx: &mut Context<Root>,
    ) -> AnyElement {
        let mut cache = self.cached.borrow_mut();
        if !cache
            .as_ref()
            .is_some_and(|(old, _)| Arc::ptr_eq(old, &text))
        {
            let parsed = match self.kind {
                Kind::Markdown => {
                    Content::Markdown(markdown::Markdown::parse(&text), Default::default())
                }
                Kind::Svg => svg::rasterize(&text).map_or(Content::Error, Content::Svg),
                Kind::Html => Content::Html(html::HtmlPreview::load(
                    backend.clone(),
                    path.to_owned(),
                    text.clone(),
                    cx,
                )),
            };
            *cache = Some((text, parsed));
        }
        match cache.as_mut().map(|(_, parsed)| parsed) {
            Some(Content::Markdown(markdown, images)) => markdown
                .render(path, backend, images, palette, window, cx)
                .into_any_element(),
            Some(Content::Svg(image)) => render_image(image.clone()),
            Some(Content::Html(html)) => html.render(
                &root.files.html_preview,
                palette,
                &root.semantics.context.frame(),
            ),
            _ => div()
                .p(px(8.))
                .text_color(palette.muted)
                .child("Could not display image.")
                .into_any_element(),
        }
    }
}

/// The open document a preview renders.
pub(crate) struct PreviewDocument<'a> {
    pub path: &'a str,
    pub backend: &'a DocumentBackend,
    pub text: Arc<str>,
}

pub(crate) fn image_error() -> AnyElement {
    div()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .p(px(8.))
        .child("Could not display image.")
        .into_any_element()
}

pub(crate) fn render_image(image: Arc<Image>) -> AnyElement {
    div()
        .flex_1()
        .min_h_0()
        .min_w_0()
        .p(px(8.))
        .flex()
        .items_center()
        .justify_center()
        .child(
            img(image)
                .id("file-image")
                .max_w_full()
                .max_h_full()
                .with_fallback(|| {
                    div()
                        .whitespace_nowrap()
                        .child("Could not display image.")
                        .into_any_element()
                }),
        )
        .into_any_element()
}
