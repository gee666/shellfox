pub(super) use super::operation_tracker::Operations;
use super::*;
pub(super) fn mutating(request: &api::Request) -> bool {
    !matches!(request, api::Request::RemoteState | api::Request::SshList)
}
impl SettingsPanel {
    /// Owed autosaves and real mutation futures only. Merely opening Settings,
    /// polling, or retaining a failed/invalid draft does not make the window busy.
    pub fn pending_operations(&self) -> bool {
        self.operations.pending()
            || self.environment_writing
            || self.environment_write.is_some()
            || self.ssh_open_intent.pending()
            || self.draft.as_ref().is_some_and(|draft| {
                draft.inflight.is_some() || self.forms.errors.is_empty() && draft.queued_write()
            })
    }
    /// Root calls this after close approval, before starting host shutdown.
    /// False means nothing was frozen; await/review the outstanding operation.
    pub fn try_freeze_for_close(&mut self, cx: &mut Context<Self>) -> bool {
        let pending = self.pending_operations();
        if !self.operations.try_freeze(pending) {
            return false;
        }
        self.sync_close_read_only(cx);
        cx.notify();
        true
    }
    /// Cancellation re-enables the same drafts, including pre-freeze native input
    /// whose observer had not run yet. It never creates a second save queue.
    pub fn resume_after_close_cancelled(&mut self, cx: &mut Context<Self>) {
        self.operations.resume();
        self.sync_close_read_only(cx);
        for (key, field) in self.forms.fields.clone() {
            if !field.read(cx).is_composing() {
                let text = field.read(cx).text.clone();
                self.changed_input(key, text, cx);
            }
        }
        if self.visible && self.services.is_some() {
            self.load_auxiliary(cx);
        }
        self.begin_save(cx);
        cx.notify();
    }
    pub(super) fn sync_close_read_only(&self, cx: &mut App) {
        let frozen = self.operations.frozen();
        let mut fields = self.forms.fields.values().cloned().collect::<Vec<_>>();
        fields.extend([self.remote.hostname.clone(), self.remote.auth.clone()]);
        for field in fields {
            field.update(cx, |field, cx| field.set_read_only(frozen, cx));
        }
        if let Some(environment) = &self.environment {
            environment.text.update(cx, |field, cx| {
                field.set_read_only(frozen || environment.pending, cx)
            });
        }
        if let Some(editor) = &self.ssh.editor {
            editor.configure_fields(frozen || self.ssh.pending, cx);
        }
    }
}
#[cfg(test)]
mod tests {
    #[test]
    fn read_only_requests_do_not_make_an_open_panel_busy() {
        for request in [
            super::api::Request::RemoteState,
            super::api::Request::SshList,
        ] {
            assert!(!super::mutating(&request));
        }
        assert!(super::mutating(&super::api::Request::SshImport));
        assert!(super::mutating(&super::api::Request::Login {
            auth_key: None
        }));
    }
}
