use crate::{app::TerminalFileLink, root::Root};
use gpui::{AnyWindowHandle, Context, Window};
use shellfox_core::TabId;
use shellfox_protocol::{AuthenticatedPeerId, manager_v2 as m};
use std::path::Path;

#[derive(Clone, Copy)]
pub(crate) struct SourcePosition {
    pub line: usize,
    pub column: Option<usize>,
    pub window: AnyWindowHandle,
}

#[derive(Clone, Copy)]
pub(crate) enum LinkOrigin {
    Local(u64),
    Peer { tab: m::Id, navigation: u64, source: u64 },
}

impl LinkOrigin {
    pub(super) fn matches_peer(self, current_source: u64, current_tab: Option<m::Id>, current_navigation: Option<u64>) -> bool {
        matches!(self, Self::Peer { tab, navigation, source }
            if current_source == source && current_tab == Some(tab) && current_navigation == Some(navigation))
    }
}

impl Root {
    pub(crate) fn link_is_current(&self, origin: LinkOrigin) -> bool {
        if self.settings { return false; }
        match origin {
            LinkOrigin::Local(epoch) => self.is_local_source() && self.navigation.epoch() == epoch,
            LinkOrigin::Peer { .. } => self.peers.selected_peer().is_some_and(|(peer, state)| {
                origin.matches_peer(self.source_selection_epoch(), state.selected_tab,
                    self.peer_ui.navigation.get(peer).copied())
            }),
        }
    }

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
            super::list::Navigation::TerminalLink(LinkOrigin::Local(self.navigation.epoch()), position),
            cx,
        );
    }

    pub(crate) fn open_peer_terminal_file_link(
        &mut self,
        peer: &AuthenticatedPeerId,
        tab: m::Id,
        target: &TerminalFileLink,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        if !self.peers.selected_peer().is_some_and(|(selected, _)| selected == peer) { return; }
        let Some(&navigation) = self.peer_ui.navigation.get(peer) else { return; };
        let origin = LinkOrigin::Peer { tab, navigation, source: self.source_selection_epoch() };
        if !self.link_is_current(origin) { return; }
        let Ok((backend, cwd)) = self.peer_file_backend() else { return; };
        let path = if let Some(scope) = backend.wsl_scope() {
            match scope.guest(&target.path) {
                Ok(path) => path,
                Err(error) => { self.error = Some(error.to_string()); cx.notify(); return; }
            }
        } else if target.path.starts_with(['/', '\\']) || target.path.starts_with("~/")
            || target.path.as_bytes().get(1) == Some(&b':') {
            target.path.clone()
        } else {
            let Some(cwd) = cwd else { return; };
            let sep = if cwd.contains('\\') { '\\' } else { '/' };
            format!("{}{sep}{}", cwd.trim_end_matches(['/', '\\']), target.path)
        };
        self.select_file_scope(backend, cx);
        let position = target.line.map(|line| SourcePosition { line, column: target.column, window: window.window_handle() });
        self.navigate_files(path, super::list::Navigation::TerminalLink(origin, position), cx);
    }
}
