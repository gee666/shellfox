use crate::{app::TerminalFileLink, root::Root};
use gpui::{AnyWindowHandle, Context, Window};
use shellfox_core::TabId;
use std::path::Path;

#[derive(Clone, Copy)]
pub(crate) struct SourcePosition {
    pub line: usize,
    pub column: Option<usize>,
    pub window: AnyWindowHandle,
}

impl Root {
    pub(crate) fn open_terminal_file_link(
        &mut self,
        id: TabId,
        target: &TerminalFileLink,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        if self.settings || !self.is_local_source() || self.selected_tab() != Some(id) {
            return;
        }
        let Some((backend, stored_directory)) = self.selected_document_backend(cx) else {
            return;
        };
        let live_directory = self.views.get(&id).and_then(|view| view.read(cx).working_directory());
        let path = if let Some(scope) = backend.wsl_scope() {
            match scope.guest_from(&target.path, live_directory.as_deref()) {
                Ok(path) => path,
                Err(error) => {
                    self.error = Some(error.to_string());
                    cx.notify();
                    return;
                }
            }
        } else if Path::new(&target.path).is_absolute()
            || target.path.starts_with("~/")
            || target.path.starts_with("~\\")
        {
            target.path.clone()
        } else {
            let Some(cwd) = live_directory.or(stored_directory) else {
                return;
            };
            let candidate = Path::new(&cwd).join(&target.path);
            let Some(path) = candidate.into_os_string().into_string().ok() else {
                return;
            };
            path
        };
        self.select_file_scope(backend, cx);
        let position = target.line.map(|line| SourcePosition {
            line,
            column: target.column,
            window: window.window_handle(),
        });
        self.navigate_files(
            path,
            super::list::Navigation::TerminalLink(self.navigation.epoch(), position),
            cx,
        );
    }
}
