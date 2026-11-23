use crate::{
    accessibility::{Role, SemanticFrame, SemanticNode},
    files::DocumentBackend,
    root::Root,
    theme::Palette,
};
use gpui::{AnyElement, Context, Task, div, prelude::*, px};
use shellfox_files::EditorCancellation;
use std::{cell::Cell, rc::Rc, sync::Arc};

mod assets;
mod sandbox;
mod surface;
pub(crate) use surface::Surface;

const MAX_DOCUMENT: usize = 12 * 1024 * 1024;

pub(super) struct HtmlPreview {
    page: anyhow::Result<Arc<assets::Page>>,
    cancel: EditorCancellation,
    warnings_open: Rc<Cell<bool>>,
    _warnings: Option<Task<()>>,
}

impl HtmlPreview {
    pub fn load(
        backend: DocumentBackend,
        path: String,
        source: Arc<str>,
        cx: &mut Context<Root>,
    ) -> Self {
        let cancel = EditorCancellation::default();
        let page = if source.len() <= MAX_DOCUMENT {
            Ok(Arc::new(assets::Page::new(
                source.as_bytes().into(),
                backend,
                path,
                cancel.clone(),
                cx.background_executor().clone(),
            )))
        } else {
            Err(anyhow::anyhow!(
                "This HTML document is too large to preview."
            ))
        };
        // Assets load while the page renders; repaint when one fails.
        let warnings = page.as_ref().ok().map(|page| {
            let mut changes = page.watch_warnings();
            cx.spawn(async move |root, cx| {
                while changes.changed().await.is_ok() {
                    if root.update(cx, |_, cx| cx.notify()).is_err() {
                        break;
                    }
                }
            })
        });
        Self {
            page,
            cancel,
            warnings_open: Rc::default(),
            _warnings: warnings,
        }
    }

    pub fn render(&self, surface: &Surface, palette: Palette, frame: &SemanticFrame) -> AnyElement {
        let page = match &self.page {
            Ok(page) => page,
            Err(error) => {
                return div()
                    .p(px(8.))
                    .text_color(palette.muted)
                    .child(error.to_string())
                    .into_any_element();
            }
        };
        let warnings = page.warnings();
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .when(!warnings.is_empty(), |body| {
                let expanded = self.warnings_open.get();
                let open = self.warnings_open.clone();
                let caption = format!("{} unavailable preview assets", warnings.len());
                let text = div().child(format!("{} {caption}", if expanded { "▾" } else { "▸" }));
                let label = match (
                    frame.id_for("files/html-warnings"),
                    frame.id_for("root/files"),
                ) {
                    (Ok(id), Ok(parent)) => frame
                        .wrap(
                            SemanticNode::new(id, Some(parent), Role::Label, caption),
                            text,
                        )
                        .into_any_element(),
                    (Err(error), _) | (_, Err(error)) => {
                        frame.reject(error);
                        text.into_any_element()
                    }
                };
                body.child(
                    div()
                        .id("html-preview-warnings")
                        .px(px(8.))
                        .py(px(4.))
                        .text_color(palette.muted)
                        .cursor_pointer()
                        .child(label)
                        .on_click(move |_, _, cx| {
                            open.set(!open.get());
                            cx.refresh_windows();
                        }),
                )
                .when(expanded, |body| {
                    body.child(
                        div()
                            .id("html-preview-warning-list")
                            .px(px(8.))
                            .max_h(px(96.))
                            .overflow_y_scroll()
                            .text_color(palette.muted)
                            .children(warnings.into_iter().map(|warning| div().child(warning))),
                    )
                })
            })
            .child(surface.element(page.clone()))
            .into_any_element()
    }
}

impl Drop for HtmlPreview {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}
