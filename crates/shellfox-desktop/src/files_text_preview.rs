use crate::{files::DocumentBackend, root::Root, theme::Palette};
use gpui::{AnyElement, Context, Image, ScrollHandle, StyledImage, div, img, prelude::*, px};
use std::{cell::RefCell, sync::Arc};
#[path = "files_markdown.rs"]
mod markdown;
#[path = "files_markdown_image.rs"]
mod markdown_image;
#[path = "files_svg.rs"]
mod svg;

#[derive(Clone, Copy)]
enum Kind {
    Markdown,
    Svg,
}

enum Content {
    Markdown(markdown::Markdown, markdown::Images),
    Svg(Arc<Image>),
    Error,
}

pub(crate) struct TextPreview {
    kind: Kind,
    pub showing: bool,
    scroll: ScrollHandle,
    cached: RefCell<Option<(Arc<str>, Content)>>,
}

impl TextPreview {
    pub fn for_path(path: &str) -> Option<Self> {
        let (_, extension) = path.rsplit_once('.')?;
        let kind = match extension.to_ascii_lowercase().as_str() {
            "md" | "mdx" | "markdown" | "mdown" | "mkd" => Kind::Markdown,
            "svg" => Kind::Svg,
            _ => return None,
        };
        Some(Self {
            kind,
            showing: true,
            scroll: ScrollHandle::new(),
            cached: RefCell::default(),
        })
    }

    pub fn render(
        &self,
        path: &str,
        backend: &DocumentBackend,
        text: Arc<str>,
        palette: Palette,
        window: &gpui::Window,
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
            };
            *cache = Some((text, parsed));
        }
        match cache.as_mut().map(|(_, parsed)| parsed) {
            Some(Content::Markdown(markdown, images)) => div()
                .id("file-markdown-preview")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .track_scroll(&self.scroll)
                .child(markdown.render(path, backend, images, palette, window, cx))
                .into_any_element(),
            Some(Content::Svg(image)) => render_image(image.clone()),
            _ => div()
                .p(px(8.))
                .text_color(palette.muted)
                .child("Could not display image.")
                .into_any_element(),
        }
    }
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
                .with_fallback(|| div().child("Could not display image.").into_any_element()),
        )
        .into_any_element()
}
