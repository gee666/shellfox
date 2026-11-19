use super::{Editor, EditorFile, Preview, editor_state::SaveSnapshot, pending::FileAction};
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
        ReadResult::Binary { path, size, .. } => {
            LoadedDocument::Preview(Preview::Binary { path, size })
        }
        ReadResult::Image {
            path,
            mime_type,
            revision,
            bytes,
            ..
        } => LoadedDocument::Preview(Preview::image(path, &mime_type, revision, bytes)),
    }
}

impl Root {
    pub(crate) fn request_open_file(&mut self, path: String, cx: &mut Context<Self>) {
        if self.files_dirty(cx) && self.files.as_ref().and_then(|f| f.opened_path()) == Some(&path)
        {
            return;
        }
        self.request_file_action(FileAction::Open(path), cx);
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
        files.operation = Some(cancellation.clone());
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
                        text,
                        size,
                        revision,
                    }) => {
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
                    let field = editor.text.clone();
                    let entity = cx.entity().downgrade();
                    cx.defer(move |cx| {
                        let Some(root) = entity.upgrade() else {
                            return;
                        };
                        if !root.read(cx).settings
                            && root
                                .read(cx)
                                .files
                                .as_ref()
                                .and_then(|files| files.editor.as_ref())
                                .is_some_and(|editor| editor.text == field)
                        {
                            let _ = cx.update_window(position.window, |_, window, cx| {
                                field.read(cx).focus(window)
                            });
                        }
                    });
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
        };
        files.request += 1;
        let serial = files.request;
        let epoch = files.source_epoch;
        let backend = files.backend.clone();
        let task_backend = backend.clone();
        files.busy = true;
        files.document_error = None;
        let cancellation = EditorCancellation::default();
        files.operation = Some(cancellation.clone());
        let request = snapshot.clone();
        let task = cx
            .background_executor()
            .spawn(async move { task_backend.write(&request.request(), &cancellation).await });
        cx.spawn(async move |root, cx| {
            let result = task.await;
            let _ = root.update(cx, |root, cx| {
                let current = root.files_result_current(epoch, &backend);
                let Some(files) = &mut root.files else { return };
                if files.request != serial || !files.backend.same_origin(&backend) { return; }
                files.busy = false;
                files.operation = None;
                let continue_on_save = std::mem::take(&mut files.save_and_continue);
                let Some(editor) = &mut files.editor else { return };
                if !current {
                    editor.reload_required = true;
                    files.document_error = Some("Save result belongs to its original retired source. Reload there before saving again.".into());
                } else {
                    match result {
                        Ok(written) => {
                            if let Some(mut prepared) = snapshot.prepare_completion(&editor.file, written) {
                                if let Err(error) = backend.commit_if_current(|| prepared.commit(&mut editor.file, &mut editor.saved_text)) {
                                    editor.reload_required = true;
                                    files.document_error = Some(format!("Save source retired; outcome requires reload: {error:#}"));
                                }
                            } else {
                                editor.reload_required = true;
                                files.document_error = Some("Save completed for an older document revision. Reload before saving again.".into());
                            }
                        }
                        Err(error) => {
                            // Native or peer I/O can have uncertain completion. Only an explicit
                            // revision conflict is safe to retry against the same revision.
                            let conflict = error.downcast_ref::<FileError>().is_some_and(|e| e.code == ErrorCode::FileConflict);
                            editor.reload_required = !conflict;
                            files.document_error = Some(error.to_string());
                        }
                    }
                }
                if continue_on_save && files.document_error.is_none() && !editor.dirty(cx)
                    && let Some(action) = files.pending_file_action.take()
                {
                    root.perform_file_action(action, cx);
                }
                cx.notify();
            });
        }).detach();
        cx.notify();
    }

    pub(crate) fn cancel_file_operation(&mut self, cx: &mut Context<Self>) {
        if let Some(operation) = self.files.as_ref().and_then(|f| f.operation.as_ref()) {
            operation.cancel();
        }
        cx.notify();
    }
}
