use super::{FileOperation, Files, Preview, image_editor::ImageEditor};
use crate::{
    root::Root,
    root::source::{Phase, Source},
};
use gpui::Context;
use shellfox_files::{EditorCancellation, ErrorCode, FileError, ReadResult};
use std::time::{Duration, Instant};

#[derive(Debug, PartialEq, Eq)]
enum Revision<'a> {
    Text(&'a str, &'a str),
    Image(&'a str, &'a str),
    Binary(&'a str, &'a str),
}

impl<'a> Revision<'a> {
    fn path(&self) -> &'a str {
        match *self {
            Self::Text(path, _) | Self::Image(path, _) | Self::Binary(path, _) => path,
        }
    }
}

impl<'a> From<&'a ReadResult> for Revision<'a> {
    fn from(file: &'a ReadResult) -> Self {
        match file {
            ReadResult::Text { path, revision, .. } => Self::Text(path, revision),
            ReadResult::Image { path, revision, .. } => Self::Image(path, revision),
            ReadResult::Binary { path, revision, .. } => Self::Binary(path, revision),
        }
    }
}

impl Files {
    fn opened_revision(&self) -> Option<Revision<'_>> {
        if let Some(editor) = &self.editor {
            return Some(Revision::Text(&editor.file.path, &editor.file.revision));
        }
        match self.preview.as_ref()? {
            Preview::Image { path, revision, .. } => Some(Revision::Image(path, revision)),
            Preview::Binary { path, revision, .. } => Some(Revision::Binary(path, revision)),
        }
    }

    fn unconfirmed_save(&self) -> bool {
        self.editor
            .as_ref()
            .is_some_and(|editor| editor.reload_required)
            || matches!(&self.image_editor, Some(ImageEditor::Ready(draft)) if draft.reload_required)
    }
}

impl Root {
    pub(crate) fn resume_peer_files(&mut self, cx: &mut Context<Self>) {
        self.revalidate_peer_files(false, cx);
    }

    pub(crate) fn can_resume_file_draft(&self, cx: &Context<Self>) -> bool {
        self.files.as_ref().is_some_and(|files| {
            files.resume_conflict && files.dirty(cx) && self.may_revalidate(files, true)
        })
    }

    pub(crate) fn resume_file_draft(&mut self, cx: &mut Context<Self>) {
        if self.can_resume_file_draft(cx) {
            self.revalidate_peer_files(true, cx);
        }
    }

    fn may_revalidate(&self, files: &Files, explicit: bool) -> bool {
        files.visible
            && !files.busy
            && files.transfer.is_none()
            && !files.unconfirmed_save()
            && (explicit
                || (!files.resume_conflict
                    && files.resume_retry.is_none_or(|due| due <= Instant::now())))
            && !self.closing_window
            && !self.peer_ui.frozen
            && matches!(&files.source, Source::Peer(_))
            && files.source == self.peers.selected
            && files.source_epoch == self.source_selection_epoch()
            && self.active_file_tab(&files.backend)
            && self
                .peers
                .selected_peer()
                .is_some_and(|(_, state)| state.phase == Phase::Connected)
    }

    fn revalidate_peer_files(&mut self, explicit: bool, cx: &mut Context<Self>) {
        let Some(files) = &self.files else { return };
        let cached = self.files_source_current();
        let (fresh, preferred) = if cached {
            if !files.check_cached_revision
                || !files.visible
                || files.busy
                || self.closing_window
                || self.peer_ui.frozen
                || files.unconfirmed_save()
                || files.resume_retry.is_some_and(|due| due > Instant::now())
            {
                return;
            }
            (files.backend.clone(), None)
        } else {
            if !self.may_revalidate(files, explicit) {
                return;
            }
            let Ok(binding) = self.peer_file_backend_for(files.backend.is_wsl()) else {
                return;
            };
            binding
        };
        if !files.backend.same_panel_scope(&fresh) {
            return;
        }
        let epoch = files.source_epoch;
        let Some(files) = &mut self.files else { return };
        let path = files.opened_revision().map(|file| file.path().to_owned());
        let previous = files.backend.clone();
        files.request += 1;
        let request = files.request;
        files.busy = true;
        files.resume_retry = Some(Instant::now() + Duration::from_secs(2));
        let cancel = EditorCancellation::default();
        files.operation = Some(FileOperation::Revalidate(cancel.clone()));
        let reading = fresh.clone();
        let read_cancel = cancel.clone();
        let read = cx.background_executor().spawn(async move {
            match path {
                Some(path) => reading.read(&path, &read_cancel).await.map(Some),
                None => Ok(None),
            }
        });
        cx.spawn(async move |root, cx| {
            let reply = read.await;
            let _ = root.update(cx, |root, cx| {
                let current = root.source_selection_epoch() == epoch
                    && root
                        .files
                        .as_ref()
                        .is_some_and(|files| files.source == root.peers.selected)
                    && !root.closing_window
                    && !root.peer_ui.frozen
                    && root.active_file_tab(&fresh);
                let Some(files) = &mut root.files else { return };
                if files.request != request || !files.backend.same_origin(&previous) {
                    return;
                }
                files.busy = false;
                files.operation = None;
                if !current || cancel.is_cancelled() || files.unconfirmed_save() {
                    cx.notify();
                    return;
                }
                match reply {
                    Ok(file) if files.opened_revision() == file.as_ref().map(Revision::from) => {
                        // Rebind presentation only; no old operation is replayed.
                        if fresh
                            .commit_if_current(|| {
                                files.backend = fresh.clone();
                                files.resume_conflict = false;
                                files.check_cached_revision = false;
                                files.resume_retry = None;
                                files.document_error = None;
                                files.tree.cancel_loading();
                            })
                            .is_ok()
                            && files.listing.is_none()
                        {
                            root.list_files(preferred.unwrap_or_else(|| "~".into()), cx);
                        }
                    }
                    Ok(_) => mark_conflict(files, cached),
                    Err(error)
                        if error
                            .downcast_ref::<FileError>()
                            .is_some_and(|e| e.code == ErrorCode::NotFound) =>
                    {
                        mark_conflict(files, cached)
                    }
                    Err(error) => files.document_error = Some(error.to_string()),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
}

fn mark_conflict(files: &mut Files, cached: bool) {
    files.resume_conflict = true;
    files.check_cached_revision = false;
    files.document_error = Some(
        if cached {
            "The file changed since this draft was last open. The draft was not applied."
        } else {
            "The file changed while disconnected. The draft was not applied."
        }
        .into(),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resume_requires_the_same_path_revision_and_kind() {
        let original = Revision::Text("/file", "r1");
        let text = |path: &str, revision: &str| ReadResult::Text {
            path: path.into(),
            mime_type: "text/plain".into(),
            size: 1,
            revision: revision.into(),
            text: "a".into(),
        };
        assert_eq!(original, Revision::from(&text("/file", "r1")));
        assert_ne!(original, Revision::from(&text("/different", "r1")));
        assert_ne!(original, Revision::from(&text("/file", "r2")));
        assert_ne!(
            original,
            Revision::from(&ReadResult::Binary {
                path: "/file".into(),
                mime_type: "application/octet-stream".into(),
                size: 1,
                revision: "r1".into(),
            })
        );
    }
}
