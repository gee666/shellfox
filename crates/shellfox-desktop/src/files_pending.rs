use crate::root::Root;
use gpui::Context;

pub(crate) enum FileAction {
    Open(String),
    OpenAt(String, u64, super::terminal_links::SourcePosition),
    ImageEditing(bool),
    Hide,
}

impl Root {
    pub(crate) fn request_file_action(&mut self, action: FileAction, cx: &mut Context<Self>) {
        let closing_retired_wsl = matches!(action, FileAction::Hide)
            && self.files.as_ref().is_some_and(|f| f.backend.is_wsl() && self.is_local_source());
        if (!closing_retired_wsl && !self.require_files_source(cx))
            || self.files.as_ref().is_some_and(|f| f.operation.is_some())
        {
            return;
        }
        if self.files_dirty(cx) {
            if let Some(files) = &mut self.files {
                files.pending_file_action = Some(action);
            }
            cx.notify();
        } else {
            self.perform_file_action(action, cx);
        }
    }

    pub(crate) fn dismiss_file_action(&mut self, cx: &mut Context<Self>) {
        if let Some(files) = &mut self.files
            && files.operation.is_none()
        {
            files.pending_file_action = None;
            cx.notify();
        }
    }

    pub(crate) fn discard_and_continue_file_action(&mut self, cx: &mut Context<Self>) {
        if let Some(files) = &mut self.files
            && !files.busy
            && let Some(action) = files.pending_file_action.take()
        {
            if let Some(editor) = &files.editor {
                let saved = editor.saved_text.to_string();
                editor
                    .text
                    .update(cx, |field, cx| field.set_text(saved, cx));
            }
            files.document_error = None;
            files.image_editor = None;
            self.perform_file_action(action, cx);
        }
    }

    pub(crate) fn save_and_continue_file_action(&mut self, cx: &mut Context<Self>) {
        if !self.require_files_source(cx) {
            return;
        }
        if let Some(files) = &mut self.files
            && !files.busy
            && files.pending_file_action.is_some()
            && files
                .editor
                .as_ref()
                .is_some_and(|e| !e.reload_required && e.dirty(cx))
        {
            files.save_and_continue = true;
            self.save_file(cx);
        }
    }

    pub(super) fn perform_file_action(&mut self, action: FileAction, cx: &mut Context<Self>) {
        match action {
            FileAction::Open(path) => self.open_file(path, cx),
            FileAction::OpenAt(path, epoch, position) => {
                if epoch == self.navigation.epoch() && !self.settings {
                    self.open_file_at(path, Some(position), cx);
                }
            }
            FileAction::ImageEditing(editing) => self.set_image_editing(editing, cx),
            FileAction::Hide => {
                if let Some(files) = &mut self.files {
                    files.visible = false;
                    if files.backend.is_wsl() {
                        files.backend.close_wsl_editor();
                        files.editor = None;
                        files.preview = None;
                        files.image_editor = None;
                        files.document_path = None;
                        files.document_error = None;
                    }
                }
                cx.notify();
            }
        }
    }
}

pub(crate) fn basename(path: &str) -> &str {
    path.rsplit(['/', '\\'])
        .find(|part| !part.is_empty())
        .unwrap_or(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_accept_windows_and_posix_paths_on_any_platform() {
        assert_eq!(basename(r"C:\project\notes.txt"), "notes.txt");
        assert_eq!(basename("/project/notes.txt"), "notes.txt");
        assert_eq!(basename("/"), "/");
    }
}
