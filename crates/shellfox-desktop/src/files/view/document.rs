use super::text_button;
use crate::{
    files::{
        Files, Preview,
        text_preview::{PreviewDocument, image_error, render_image},
    },
    root::Root,
    theme::Palette,
};
use gpui::{AnyElement, Context, Window, div, prelude::*, px};
mod image_view;
mod link_error;
mod number;
mod toolbar;
use number::grouped_size;
use toolbar::file_toolbar;

impl Root {
    pub(super) fn file_document(
        &self,
        palette: Palette,
        width: f32,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(files) = &self.files.panel else {
            return div().into_any_element();
        };
        let mut document = div()
            .flex_1()
            .min_h_0()
            .min_w_0()
            .flex()
            .flex_col()
            .overflow_hidden()
            .on_key_down(cx.listener(Self::file_editor_key));
        let path = files.opened_path().or(files.document_path.as_deref());
        let loading =
            files.operation.is_some() && files.editor.is_none() && files.preview.is_none();
        if loading {
            document = document.child(feedback("Loading file…", palette));
        }
        if let Some(error) = &files.document_error {
            document = document.child(
                div()
                    .p(px(8.))
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(6.))
                    .text_color(palette.status_color(0xf87171))
                    .child(self.semantic_region(
                        "files/document-error",
                        crate::accessibility::Role::Label,
                        error.clone(),
                        div().child(error.clone()),
                    ))
                    .when(files.opened_path().is_none() && path.is_some(), |style| {
                        style.child(
                            text_button("retry-file", "Retry file", palette)
                                .on_click(cx.listener(|root, _, _, cx| root.reload_file(cx))),
                        )
                    }),
            );
        }
        let open =
            files.editor.is_some() || files.image_editor.is_some() || files.preview.is_some();
        if open {
            document = document.children(link_error::render(self, files, false, palette, cx));
        }
        let preview = files
            .editor
            .as_ref()
            .and_then(|editor| editor.text_preview.as_ref());
        let available_preview =
            preview.filter(|preview| preview.available(&self.files.html_preview, window));
        if let Some(path) = path {
            document = document.child(file_toolbar(
                self,
                files,
                path,
                available_preview,
                palette,
                cx,
            ));
        }
        if let Some(editor) = &files.editor {
            if let Some(preview) = available_preview.filter(|preview| preview.showing) {
                return document
                    .child(preview.render(
                        PreviewDocument {
                            path: &editor.file.path,
                            backend: &files.backend,
                            text: editor.text.read(cx).snapshot_text(),
                        },
                        palette,
                        window,
                        self,
                        cx,
                    ))
                    .into_any_element();
            }
            if preview.is_some_and(|preview| preview.showing) {
                document = document.child(
                    div()
                        .flex_shrink_0()
                        .px(px(8.))
                        .py(px(4.))
                        .text_size(px(11.))
                        .text_color(palette.muted)
                        .child("HTML preview unavailable. Showing source."),
                );
            }
            editor.text.update(cx, |field, cx| {
                field.set_file_viewport(width, files.wrap, cx)
            });
            let scroll = &editor.text.read(cx).scroll;
            document = document.child(
                div()
                    .relative()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .child(
                        div()
                            .id("file-editor-scroll")
                            .track_scroll(scroll)
                            .overflow_y_scroll()
                            .when(!files.wrap, |view| view.overflow_x_scroll())
                            .flex_1()
                            .min_h_0()
                            .child(editor.text.clone()),
                    )
                    .child(editor.scrollbar.render(scroll, palette))
                    .when(!files.wrap, |frame| {
                        frame.child(editor.horizontal_scrollbar.horizontal(scroll, palette))
                    }),
            );
        } else if let Some(image_editor) = &files.image_editor {
            let browser = if files.browser {
                crate::files::resizer::ResizeAxis::Height.clamp(self, window, files.browser_height)
                    + 4.
            } else {
                0.
            };
            let height = (f32::from(window.viewport_size().height) - 67. - browser).max(280.);
            document = document.child(image_view::render(
                image_editor,
                width,
                height,
                palette,
                files.busy(),
                cx,
            ));
        } else if let Some(preview) = &files.preview {
            document = document.child(match preview {
                Preview::Binary { size, .. } => feedback(
                    format!(
                        "Binary file. No preview available. {} bytes.",
                        grouped_size(*size)
                    ),
                    palette,
                )
                .into_any_element(),
                Preview::Image {
                    image: Some(image), ..
                } => render_image(image.clone()),
                Preview::Image { image: None, .. } => image_error(),
            });
        } else if !loading {
            if let Some(link) = link_error::render(self, files, true, palette, cx) {
                document = document.child(link);
            } else if files.document_error.is_none() {
                document = document.child(feedback("Select a file.", palette));
            }
        }
        document.into_any_element()
    }

    pub(super) fn set_file_preview(&mut self, show: bool, cx: &mut Context<Self>) {
        if let Some(preview) = self
            .files
            .panel
            .as_mut()
            .and_then(|files| files.editor.as_mut())
            .and_then(|editor| editor.text_preview.as_mut())
        {
            preview.showing = show;
            cx.notify();
        }
    }

    pub(super) fn copy_open_file_path(&mut self, cx: &mut Context<Self>) {
        if !self.require_files_source(cx) {
            return;
        }
        if let Some(path) = self.files.panel.as_ref().and_then(Files::opened_path) {
            cx.write_to_clipboard(gpui::ClipboardItem::new_string(path.to_owned()));
            self.notify_info(cx, "Path copied");
        }
    }
}

fn feedback(text: impl Into<gpui::SharedString>, palette: Palette) -> gpui::Div {
    div().p(px(8.)).text_color(palette.muted).child(text.into())
}
