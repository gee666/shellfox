use crate::{root::Root, terminal::TerminalFileLink};
use gpui::{AnyWindowHandle, Context, Window};
use shellfox_core::TabId;
use shellfox_protocol::AuthenticatedPeerId;
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
    Peer {
        tab: TabId,
        navigation: u64,
        source: u64,
    },
}

impl LinkOrigin {
    pub(super) fn matches_peer(
        self,
        current_source: u64,
        current_tab: Option<TabId>,
        current_navigation: Option<u64>,
    ) -> bool {
        matches!(self, Self::Peer { tab, navigation, source }
            if current_source == source && current_tab == Some(tab) && current_navigation == Some(navigation))
    }
}

impl Root {
    pub(crate) fn link_is_current(&self, origin: LinkOrigin) -> bool {
        if self.settings.open {
            return false;
        }
        match origin {
            LinkOrigin::Local(epoch) => {
                self.is_local_source() && self.workspace.navigation.epoch() == epoch
            }
            LinkOrigin::Peer { .. } => self.peers.selected_peer().is_some_and(|(peer, state)| {
                origin.matches_peer(
                    self.source_selection_epoch(),
                    state.selected_tab,
                    self.peer_ui.navigation.get(peer).copied(),
                )
            }),
        }
    }

    /// Looks up an unopenable link again, as if it were clicked in the current view.
    pub(crate) fn retry_file_link(&mut self, cx: &mut Context<Self>) {
        let Some(link) = self
            .files
            .panel
            .as_ref()
            .and_then(|f| f.link_error.as_ref())
        else {
            return;
        };
        let (path, position) = (link.path.clone(), link.position);
        let origin = match link.origin {
            LinkOrigin::Local(_) => self
                .is_local_source()
                .then(|| LinkOrigin::Local(self.workspace.navigation.epoch())),
            LinkOrigin::Peer { tab, .. } => self.peers.selected_peer().and_then(|(peer, state)| {
                (state.selected_tab == Some(tab)).then_some(())?;
                Some(LinkOrigin::Peer {
                    tab,
                    navigation: *self.peer_ui.navigation.get(peer)?,
                    source: self.source_selection_epoch(),
                })
            }),
        };
        if let Some(origin) = origin {
            self.navigate_files(
                path,
                super::list::Navigation::TerminalLink(origin, position),
                cx,
            );
        }
    }

    pub(crate) fn copy_link_error_path(&mut self, cx: &mut Context<Self>) {
        if let Some(link) = self
            .files
            .panel
            .as_ref()
            .and_then(|f| f.link_error.as_ref())
        {
            cx.write_to_clipboard(gpui::ClipboardItem::new_string(link.path.clone()));
            self.notify_info(cx, "Path copied");
        }
    }

    pub(crate) fn open_terminal_file_link(
        &mut self,
        id: TabId,
        target: &TerminalFileLink,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        if self.settings.open || !self.is_local_source() || self.selected_tab() != Some(id) {
            return;
        }
        let Some((backend, stored_directory)) = self.selected_document_backend(cx) else {
            return;
        };
        let live_directory = self
            .tabs
            .views
            .get(&id)
            .and_then(|view| view.read(cx).working_directory());
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
            super::list::Navigation::TerminalLink(
                LinkOrigin::Local(self.workspace.navigation.epoch()),
                position,
            ),
            cx,
        );
    }

    pub(crate) fn open_peer_terminal_file_link(
        &mut self,
        peer: &AuthenticatedPeerId,
        tab: TabId,
        target: &TerminalFileLink,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        if !self
            .peers
            .selected_peer()
            .is_some_and(|(selected, _)| selected == peer)
        {
            return;
        }
        let Some(&navigation) = self.peer_ui.navigation.get(peer) else {
            return;
        };
        let origin = LinkOrigin::Peer {
            tab,
            navigation,
            source: self.source_selection_epoch(),
        };
        if !self.link_is_current(origin) {
            return;
        }
        let Ok((backend, cwd)) = self.peer_file_backend() else {
            return;
        };
        let path = if let Some(scope) = backend.wsl_scope() {
            match scope.guest(&target.path) {
                Ok(path) => path,
                Err(error) => {
                    self.error = Some(error.to_string());
                    cx.notify();
                    return;
                }
            }
        } else if target.path.starts_with(['/', '\\'])
            || target.path.starts_with("~/")
            || target.path.as_bytes().get(1) == Some(&b':')
        {
            target.path.clone()
        } else {
            let Some(cwd) = cwd else {
                return;
            };
            let sep = if cwd.contains('\\') { '\\' } else { '/' };
            format!("{}{sep}{}", cwd.trim_end_matches(['/', '\\']), target.path)
        };
        self.select_file_scope(backend, cx);
        let position = target.line.map(|line| SourcePosition {
            line,
            column: target.column,
            window: window.window_handle(),
        });
        self.navigate_files(
            path,
            super::list::Navigation::TerminalLink(origin, position),
            cx,
        );
    }
}
