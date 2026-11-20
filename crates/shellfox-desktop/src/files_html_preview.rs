use crate::{files::DocumentBackend, root::Root, theme::Palette};
use gpui::{AnyElement, Context, div, prelude::*, px};
use percent_encoding::percent_decode_str;
use shellfox_files::EditorCancellation;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::Arc,
};

#[path = "files_html_assets.rs"]
mod assets;
#[path = "files_html_markup.rs"]
mod markup;
#[path = "files_html_surface.rs"]
mod surface;
pub(crate) use surface::Surface;

const MAX_DOCUMENT: usize = 12 * 1024 * 1024;

pub(super) struct HtmlPreview {
    prepared: Rc<RefCell<Option<anyhow::Result<assets::Prepared>>>>,
    cancel: EditorCancellation,
    warnings_open: Rc<Cell<bool>>,
}

impl HtmlPreview {
    pub fn load(
        backend: DocumentBackend,
        path: String,
        source: Arc<str>,
        cx: &mut Context<Root>,
    ) -> Self {
        let cancel = EditorCancellation::default();
        let prepared = Rc::new(RefCell::new(None));
        let target = Rc::downgrade(&prepared);
        let task = cx.background_spawn(assets::prepare(backend, path, source, cancel.clone()));
        cx.spawn(async move |root, cx| {
            let result = task.await;
            if let Some(target) = target.upgrade() {
                *target.borrow_mut() = Some(result);
                let _ = root.update(cx, |_, cx| cx.notify());
            }
        })
        .detach();
        Self {
            prepared,
            cancel,
            warnings_open: Rc::default(),
        }
    }

    pub fn render(&self, surface: &Surface, palette: Palette) -> AnyElement {
        let status = self.prepared.borrow();
        let message = match status.as_ref() {
            Some(Ok(prepared)) => {
                return div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .when(!prepared.warnings.is_empty(), |body| {
                        let expanded = self.warnings_open.get();
                        let open = self.warnings_open.clone();
                        let count = prepared.warnings.len();
                        body.child(
                            div()
                                .id("html-preview-warnings")
                                .px(px(8.))
                                .py(px(4.))
                                .text_color(palette.muted)
                                .cursor_pointer()
                                .child(format!(
                                    "{} {count} unavailable preview assets",
                                    if expanded { "▾" } else { "▸" }
                                ))
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
                                    .children(
                                        prepared
                                            .warnings
                                            .iter()
                                            .cloned()
                                            .map(|warning| div().child(warning)),
                                    ),
                            )
                        })
                    })
                    .child(surface.element(prepared.bytes.clone()))
                    .into_any_element();
            }
            Some(Err(error)) => error.to_string(),
            None => "Preparing HTML preview…".to_owned(),
        };
        div()
            .p(px(8.))
            .text_color(palette.muted)
            .child(message)
            .into_any_element()
    }
}

impl Drop for HtmlPreview {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

fn relative_asset(value: &str, base: &str) -> Option<String> {
    let clean = value.trim();
    if clean.is_empty()
        || clean.len() > 4096
        || clean.starts_with(['#', '/', '\\'])
        || url::Url::parse(clean).is_ok()
    {
        return None;
    }
    let path = clean.split(['?', '#']).next()?;
    let decoded = percent_decode_str(path).decode_utf8().ok()?;
    if decoded.is_empty() || decoded.contains('\0') {
        return None;
    }
    // read_asset resolves the resulting path against the document and rejects escapes.
    Some(format!("{base}{decoded}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_assets_and_inline_styles_are_found_and_rewritten() {
        let source = r#"<!DOCTYPE html><base href="http://localhost/"><meta http-equiv="refresh" content="0;url=other"><link rel="stylesheet" href="style.css"><script src="project.js"></script><img src="./logo%20small.png" srcset="image.png 2x, https://example.test/logo.png 3x"><style>p { color: red }</style><p style="background: url(bg.png)">works</p>"#;
        let references = markup::references(source).unwrap();
        assert_eq!(
            references
                .assets
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            ["./logo small.png", "image.png", "project.js", "style.css"]
        );
        assert!(references.assets["style.css"]);
        assert_eq!(
            references.styles,
            ["background: url(bg.png)", "p { color: red }"]
        );
        let bytes = markup::rewrite(
            source,
            |path, css| {
                (css || path == "project.js").then(|| "data:text/plain;base64,Yg==".to_owned())
            },
            |css| css.replace("red", "green"),
            MAX_DOCUMENT,
        )
        .unwrap();
        let html = String::from_utf8(bytes).unwrap();
        assert!(!html.contains("<base"));
        assert!(!html.contains("http-equiv"));
        assert!(!html.contains("example.test"));
        assert!(html.contains("<script src=\"data:text/plain;base64,Yg==\"></script>"));
        assert!(html.contains("<style>p { color: green }</style>"));
        assert!(html.contains("<img src=\"data:,\" srcset=\"\">"));
    }

    #[test]
    fn rejects_an_oversized_document_without_accumulating_it() {
        assert!(
            markup::rewrite(
                "<!DOCTYPE html><p>Hello.</p>",
                |_, _| None,
                str::to_owned,
                12
            )
            .is_err()
        );
    }
}
