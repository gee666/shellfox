use super::{Editor, EditorFile, editor_state::SaveSnapshot};
use crate::{field::Field, root::Root};
use gpui::Context;
use shellfox_files::{EditorCancellation, TextFile};
impl Root {
    pub(crate) fn request_open_file(&mut self, path: String, cx: &mut Context<Self>) {
        if !self.require_files_source(cx) || self.files.as_ref().is_some_and(|f| f.busy) {
            return;
        }
        if self.files_dirty(cx) {
            if let Some(f) = &mut self.files {
                f.pending_open = Some(path);
            }
            cx.notify();
        } else {
            self.open_file(path, cx);
        }
    }
    pub(crate) fn open_file(&mut self, path: String, cx: &mut Context<Self>) {
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
        files.pending_open = None;
        let cancellation = EditorCancellation::default();
        files.operation = Some(cancellation.clone());
        if let Some(editor) = &files.editor {
            editor
                .text
                .update(cx, |field, cx| field.set_read_only(true, cx));
        }
        let task = cx
            .background_executor()
            .spawn(async move { task_backend.read(&path, &cancellation).await });
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
                if let Some(editor) = &files.editor {
                    editor
                        .text
                        .update(cx, |field, cx| field.set_read_only(false, cx));
                }
                if !current {
                    if let Some(e) = &mut files.editor {
                        e.reload_required = true;
                    }
                    root.error = Some(
                        "Document result belongs to a retired source. Reopen explicitly.".into(),
                    );
                    cx.notify();
                    return;
                }
                match result {
                    Ok(TextFile {
                        path,
                        text,
                        size,
                        revision,
                    }) => {
                        let text = Field::asset_editor(text, cx);
                        cx.observe(&text, |_, _, cx| cx.notify()).detach();
                        let saved_text = text.read(cx).snapshot_text();
                        let mut prepared = Some(Editor {
                            text,
                            saved_text,
                            reload_required: false,
                            file: EditorFile {
                                path,
                                size,
                                revision,
                            },
                        });
                        // Build entities outside the source lock; publish only a bounded swap under
                        // the ORIGINAL witness, then drop the retired document and notify afterward.
                        match backend
                            .commit_if_current(|| std::mem::swap(&mut files.editor, &mut prepared))
                        {
                            Ok(()) => root.error = None,
                            Err(e) => root.error = Some(format!("Document source retired: {e:#}")),
                        }
                        drop(prepared);
                    }
                    Err(e) => root.error = Some(e.to_string()),
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
            self.error =
                Some("Reload before saving again. The previous save was not confirmed.".into());
            cx.notify();
            return;
        }
        if !editor.dirty(cx) {
            return;
        }
        // This immutable backend plus target/revision/text is the complete original document
        // identity. It is never rebuilt from the selected peer's labels or current connection.
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
        let cancellation = EditorCancellation::default();
        files.operation = Some(cancellation.clone());
        let request = snapshot.clone();
        let task = cx
            .background_executor()
            .spawn(async move { task_backend.write(&request.request(), &cancellation).await });
        cx.spawn(async move|root,cx|{let result=task.await;let _=root.update(cx,|root,cx|{
            let current=root.files_result_current(epoch,&backend);
            let Some(files)=&mut root.files else{return};if files.request!=serial||!files.backend.same_origin(&backend){return;}
            files.busy=false;files.operation=None;
            if !current{if let Some(e)=&mut files.editor{e.reload_required=true;}root.error=Some("Save result belongs to its original retired source. Reload there before saving again.".into());cx.notify();return;}
            match result{Ok(written)=>{
                if let Some(editor)=&mut files.editor{
                    if let Some(mut prepared)=snapshot.prepare_completion(&editor.file,written){
                        match backend.commit_if_current(||prepared.commit(&mut editor.file,&mut editor.saved_text)){
                            Ok(())=>root.error=None,Err(e)=>{editor.reload_required=true;root.error=Some(format!("Save source retired; outcome requires reload: {e:#}"));}
                        }
                        drop(prepared);
                    }else{editor.reload_required=true;root.error=Some("Save completed for an older document revision. Reload before saving again.".into());}
                }
            },Err(e)=>{if let Some(editor)=&mut files.editor{editor.reload_required=true;}root.error=Some(e.to_string());}}cx.notify();
        });}).detach();
        cx.notify();
    }
    pub(crate) fn cancel_file_operation(&mut self, cx: &mut Context<Self>) {
        // Cancellation targets the ORIGINAL operation even after selection/approval changes.
        // It cannot authorize new I/O, reacquire a binding or clear a pending close guard.
        if let Some(operation) = self.files.as_ref().and_then(|f| f.operation.as_ref()) {
            operation.cancel();
        }
        cx.notify();
    }
}
