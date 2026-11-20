use super::{
    Editor, EditorFile, FileOperation, Preview, editor_state::SaveSnapshot, pending::FileAction,
    terminal_links::SourcePosition,
};
use crate::{field::Field, root::Root};
use gpui::{AppContext, Context};
use shellfox_files::{EditorCancellation, ErrorCode, FileError, ReadResult};

enum LoadedDocument {
    Text {
        path: String,
        text: String,
        size: u64,
        revision: String,
    },
    Preview(Preview),
}

fn prepare_document(read: ReadResult) -> LoadedDocument {
    match read {
        ReadResult::Text {
            path,
            text,
            size,
            revision,
            ..
        } => LoadedDocument::Text {
            path,
            text,
            size,
            revision,
        },
        ReadResult::Binary {
            path,
            size,
            revision,
            ..
        } => LoadedDocument::Preview(Preview::Binary {
            path,
            size,
            revision,
        }),
        ReadResult::Image {
            path,
            mime_type,
            revision,
            bytes,
            ..
        } => LoadedDocument::Preview(Preview::image(path, &mime_type, revision, bytes)),
    }
}

fn focus_source(field: &gpui::Entity<Field>, position: SourcePosition, cx: &mut Context<Root>) {
    let field = field.clone();
    let entity = cx.entity().downgrade();
    cx.defer(move |cx| {
        let Some(root) = entity.upgrade() else { return };
        let root = root.read(cx);
        if !root.settings
            && root.files.as_ref().is_some_and(|files| {
                files.visible
                    && files.pending_file_action.is_none()
                    && files
                        .editor
                        .as_ref()
                        .is_some_and(|editor| editor.text == field)
            })
        {
            let _ = cx.update_window(position.window, |_, window, cx| {
                field.read(cx).focus(window)
            });
        }
    });
}

impl Root {
    pub(crate) fn request_open_file(&mut self, path: String, cx: &mut Context<Self>) {
        if self.files_dirty(cx) && self.files.as_ref().and_then(|f| f.opened_path()) == Some(&path)
        {
            return;
        }
        self.request_file_action(FileAction::Open(path), cx);
    }

    pub(super) fn reveal_open_file(
        &mut self,
        path: &str,
        position: SourcePosition,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(editor) = self.files.as_mut().and_then(|files| files.editor.as_mut()) else {
            return false;
        };
        if editor.file.path != path {
            return false;
        }
        if let Some(preview) = &mut editor.text_preview {
            preview.showing = false;
        }
        editor.text.update(cx, |field, cx| {
            field.goto_source_position(position.line, position.column);
            cx.notify();
        });
        focus_source(&editor.text, position, cx);
        cx.notify();
        true
    }

    pub(crate) fn open_file(&mut self, path: String, cx: &mut Context<Self>) {
        self.open_file_at(path, None, cx);
    }

    pub(super) fn open_file_at(
        &mut self,
        path: String,
        position: Option<super::terminal_links::SourcePosition>,
        cx: &mut Context<Self>,
    ) {
        if !self.require_files_source(cx) {
            return;
        }
        let Some(files) = &mut self.files else { return };
        if files.busy {
            return;
        }
        files.request += 1;
        let request = files.request;
        let epoch = files.source_epoch;
        let backend = files.backend.clone();
        let task_backend = backend.clone();
        files.busy = true;
        files.pending_file_action = None;
        files.document_path = Some(path.clone());
        files.document_error = None;
        files.editor = None;
        files.preview = None;
        files.image_editor = None;
        let cancellation = EditorCancellation::default();
        files.operation = Some(FileOperation::Open(cancellation.clone()));
        let task = cx.background_executor().spawn(async move {
            task_backend
                .read(&path, &cancellation)
                .await
                .map(prepare_document)
        });
        cx.spawn(async move |root, cx| {
            let result = task.await;
            let _ = root.update(cx, |root, cx| {
                let current = root.files_result_current(epoch, &backend);
                let Some(files) = &mut root.files else { return };
                if files.request != request || !files.backend.same_origin(&backend) {
                    return;
                }
                files.busy = false;
                files.operation = None;
                if !current {
                    files.document_error = Some(
                        "Document result belongs to a retired source. Reopen explicitly.".into(),
                    );
                    cx.notify();
                    return;
                }
                let mut editor = None;
                let mut preview = None;
                match result {
                    Ok(LoadedDocument::Text {
                        path,
                        mut text,
                        size,
                        revision,
                    }) => {
                        let utf8_bom = text.starts_with('\u{feff}');
                        if utf8_bom { text.remove(0); }
                        let text = Field::asset_editor(&path, text, cx);
                        if let Some(position) = position {
                            text.update(cx, |field, _| {
                                field.goto_source_position(position.line, position.column)
                            });
                        }
                        cx.observe(&text, |_, _, cx| cx.notify()).detach();
                        let saved_text = text.read(cx).snapshot_text();
                        editor = Some(Editor {
                            text,
                            saved_text,
                            text_preview: super::text_preview::TextPreview::for_path(&path).map(
                                |mut preview| {
                                    preview.showing = position.is_none();
                                    preview
                                },
                            ),
                            reload_required: false,
                            scrollbar: Default::default(),
                            horizontal_scrollbar: Default::default(),
                            file: EditorFile {
                                path,
                                size,
                                revision,
                                utf8_bom,
                            },
                        });
                    }
                    Ok(LoadedDocument::Preview(loaded)) => preview = Some(loaded),
                    Err(error) => files.document_error = Some(error.to_string()),
                }
                // Publish under the original admission; build and drop payloads outside it.
                if let Err(error) = backend.commit_if_current(|| {
                    std::mem::swap(&mut files.editor, &mut editor);
                    std::mem::swap(&mut files.preview, &mut preview);
                }) {
                    files.document_error = Some(format!("Document source retired: {error:#}"));
                } else if let (Some(position), Some(editor)) = (position, &files.editor) {
                    focus_source(&editor.text, position, cx);
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    pub(crate) fn save_file(&mut self, cx: &mut Context<Self>) {
        if !self.require_files_source(cx) {
            return;
        }
        let Some(files) = &mut self.files else { return };
        if files.busy {
            return;
        }
        let Some(editor) = &files.editor else { return };
        if editor.reload_required {
            files.document_error =
                Some("Reload before saving again. The previous save was not confirmed.".into());
            cx.notify();
            return;
        }
        if !editor.dirty(cx) {
            return;
        }
        let snapshot = SaveSnapshot {
            text: editor.text.read(cx).snapshot_text(),
            path: editor.file.path.clone(),
            revision: editor.file.revision.clone(),
            utf8_bom: editor.file.utf8_bom,
        };
        files.request += 1;
        let serial = files.request;
        let backend = files.backend.clone();
        let task_backend = backend.clone();
        files.busy = true;
        files.document_error = None;
        let cancellation = EditorCancellation::default();
        files.operation = Some(FileOperation::Save(cancellation.clone()));
        let request = snapshot.clone();
        let task = cx
            .background_executor()
            .spawn(async move { task_backend.write(&request.request(), &cancellation).await });
        cx.spawn(async move |root, cx| {
            let result = task.await;
            let _ = root.update(cx, |root, cx| {
                let Some((files, presented)) = root.file_write_recipient(serial, &backend) else {
                    return;
                };
                files.busy = false;
                files.operation = None;
                let continue_on_save = std::mem::take(&mut files.save_and_continue);
                let Some(editor) = &mut files.editor else {
                    return;
                };
                match result {
                    Ok(written) => {
                        let outcome = snapshot
                            .prepare_completion(&editor.file, written)
                            .ok_or_else(|| {
                                anyhow::anyhow!("Save completed for an older document revision.")
                            })
                            .and_then(|mut prepared| {
                                backend.commit_if_current(|| {
                                    prepared.commit(&mut editor.file, &mut editor.saved_text)
                                })
                            });
                        if let Err(error) = outcome {
                            editor.reload_required = true;
                            files.document_error =
                                Some(format!("Reload before saving again: {error:#}"));
                        }
                    }
                    Err(error) => {
                        // A conflict is a confirmed refusal, not a lost save receipt.
                        let conflict = error
                            .downcast_ref::<FileError>()
                            .is_some_and(|e| e.code == ErrorCode::FileConflict);
                        editor.reload_required = !conflict;
                        files.document_error = Some(error.to_string());
                    }
                }
                if presented
                    && continue_on_save
                    && files.document_error.is_none()
                    && !editor.dirty(cx)
                {
                    if let Some(action) = files.pending_file_action.take() {
                        root.perform_file_action(action, cx);
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    pub(crate) fn cancel_file_operation(&mut self, cx: &mut Context<Self>) {
        if let Some(operation) = self.files.as_ref().and_then(|f| f.operation.as_ref()) {
            operation.cancel();
        }
        cx.notify();
    }
}
