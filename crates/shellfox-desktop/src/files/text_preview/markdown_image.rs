use crate::files::{DocumentBackend, preview::raster_image};
use gpui::{Context, Entity, Image, Render, StyledImage, Window, div, img, prelude::*};
use std::sync::Arc;

pub(super) struct MarkdownImage {
    status: State,
    alt: String,
    cancel: shellfox_files::EditorCancellation,
}
enum State {
    Loading,
    Failed,
    Ready(Arc<Image>),
}

impl MarkdownImage {
    pub fn load(
        backend: DocumentBackend,
        path: String,
        asset: String,
        alt: String,
        cx: &mut gpui::App,
    ) -> Entity<Self> {
        cx.new(|cx: &mut Context<Self>| {
            let cancel = shellfox_files::EditorCancellation::default();
            let worker = cancel.clone();
            let task = cx.background_executor().spawn(async move {
                let value = backend.read_asset(&path, asset, worker).await.ok()?;
                backend.check_current().ok()?;
                if value.mime_type == "image/svg+xml" {
                    super::svg::rasterize(std::str::from_utf8(&value.bytes).ok()?).ok()
                } else {
                    raster_image(&value.mime_type, value.bytes)
                }
            });
            cx.spawn(async move |view, cx| {
                let image = task.await;
                let _ = view.update(cx, |view, cx| {
                    view.status = image.map_or(State::Failed, State::Ready);
                    cx.notify();
                });
            })
            .detach();
            Self {
                status: State::Loading,
                alt,
                cancel,
            }
        })
    }
}

impl Drop for MarkdownImage {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

impl Render for MarkdownImage {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let failed = format!("Image unavailable: {}", self.alt);
        match &self.status {
            State::Loading => div().child("Loading image…").into_any_element(),
            State::Failed => div().child(failed).into_any_element(),
            State::Ready(image) => img(image.clone())
                .max_w_full()
                .with_fallback(move || div().child(failed.clone()).into_any_element())
                .into_any_element(),
        }
    }
}
