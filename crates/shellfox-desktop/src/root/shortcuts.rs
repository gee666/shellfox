//! Sidebar and Files shortcuts as GPUI actions. The workspace offers the
//! `LayoutShortcuts` key context only while no dialog, menu or IME
//! composition owns the keyboard, so blocked keystrokes reach that owner.
use crate::{editor_composition::EditorOutcome, root::Root};
use gpui::{App, Context, KeyBinding, KeyContext, Window, actions};

actions!(shellfox_layout, [ToggleSessions, ToggleFiles, ToggleFolders]);

const CONTEXT: &str = "LayoutShortcuts";

/// `secondary` is Cmd on macOS and Ctrl elsewhere.
pub(crate) fn bind_keys(cx: &mut App) {
    cx.bind_keys(bindings());
}

fn bindings() -> [KeyBinding; 3] {
    [
        KeyBinding::new("secondary-b", ToggleSessions, Some(CONTEXT)),
        KeyBinding::new("secondary-shift-b", ToggleFiles, Some(CONTEXT)),
        KeyBinding::new("secondary-shift-e", ToggleFolders, Some(CONTEXT)),
    ]
}

impl Root {
    pub(crate) fn layout_key_context(&self, cx: &App) -> KeyContext {
        let mut context = KeyContext::default();
        context.add("Shellfox");
        if !self.layout_shortcuts_blocked(cx) {
            context.add(CONTEXT);
        }
        context
    }

    fn layout_shortcuts_blocked(&self, cx: &App) -> bool {
        let dialog_blocks = match &self.dialogs.current {
            Some(crate::dialogs::Dialog::RenameTab { title, .. }) => title.read(cx).is_composing(),
            None => false,
            Some(_) => true,
        };
        self.close.closing
            || dialog_blocks
            || self.sidebar.session_menu.is_some()
            || self.sidebar.menu_open()
            || self
                .settings.panel
                .as_ref()
                .is_some_and(|panel| self.settings.open && panel.read(cx).blocks_layout_shortcuts())
            || self.tabs.profile_menu.is_some()
            || self.tabs.menu.is_some()
            || self.files.panel.as_ref().is_some_and(|files| {
                files.visible
                    && (files.pending_file_action.is_some()
                        || (files.browser
                            && (files.missing_folder.is_some()
                                || files.location.read(cx).is_composing()))
                        || files
                            .editor
                            .as_ref()
                            .is_some_and(|editor| editor.text.read(cx).is_composing()))
            })
            || self.layout_terminal().is_some_and(|view| {
                let view = view.read(cx);
                view.link_menu_open() || !view.preedit.is_empty()
            })
    }

    pub(crate) fn on_toggle_sessions(
        &mut self,
        _: &ToggleSessions,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_sessions_layout(window, cx);
    }

    pub(crate) fn on_toggle_files(
        &mut self,
        _: &ToggleFiles,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.settings.open || !self.files_shortcut_available(cx) {
            cx.propagate();
            return;
        }
        self.toggle_files_layout(window, cx);
        cx.notify();
    }

    pub(crate) fn on_toggle_folders(
        &mut self,
        _: &ToggleFolders,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.settings.open
            || !self.files_shortcut_available(cx)
            || self.files.panel.as_ref().is_none_or(|files| !files.visible)
            || !self.files_source_current()
        {
            cx.propagate();
            return;
        }
        self.toggle_file_browser(window, cx);
        cx.notify();
    }

    pub(crate) fn toggle_sessions_layout(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let restore = self.sidebar.visible
            && (self.sidebar.region.contains_focused(window, cx)
                || self.sidebar.resizer.is_focused(window));
        self.sidebar.visible = !self.sidebar.visible;
        self.sidebar.resizing = false;
        self.sidebar.resize_origin = None;
        if restore {
            self.sidebar.toggle.focus(window);
        }
        cx.notify();
    }

    pub(crate) fn toggle_files_layout(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let restore = self
            .files.panel
            .as_ref()
            .is_some_and(|files| files.visible && files.panel_focus.contains_focused(window, cx));
        self.toggle_files(cx);
        if restore && self.files.panel.as_ref().is_none_or(|files| !files.visible) {
            if self.is_local_source() && self.selected_tab().is_none() {
                self.focus_files_toggle(window, cx);
            } else {
                self.focus_selected(window, cx);
            }
        }
    }

    fn layout_terminal(&self) -> Option<&gpui::Entity<crate::terminal::TerminalView>> {
        if self.is_local_source() {
            return self.selected_tab().and_then(|tab| self.tabs.views.get(&tab));
        }
        self.peers.selected_peer().and_then(|(peer, state)| {
            self.peer_ui
                .v1_views
                .get(&(peer.clone(), state.selected_tab?))
        })
    }

    fn files_shortcut_available(&self, cx: &App) -> bool {
        if self.is_local_source() {
            return matches!(
                cx.try_global::<EditorOutcome>(),
                Some(EditorOutcome::Ready(_))
            );
        }
        self.peer_file_backend().is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{Action, Keystroke};

    #[test]
    fn bindings_use_ctrl_or_cmd_and_only_the_exact_chords() {
        let primary = if cfg!(target_os = "macos") { "cmd" } else { "ctrl" };
        let expected: [(&str, &dyn Action); 3] = [
            ("b", &ToggleSessions),
            ("shift-b", &ToggleFiles),
            ("shift-e", &ToggleFolders),
        ];
        // Electron binds nothing to a lone Alt or Alt+Enter; both reach the terminal.
        for stroke in ["alt", "alt-enter", "enter"] {
            let stroke = Keystroke::parse(stroke).unwrap();
            assert!(bindings().iter().all(|binding| binding.match_keystrokes(std::slice::from_ref(&stroke)).is_none()));
        }
        for (binding, (chord, action)) in bindings().iter().zip(expected) {
            let stroke = Keystroke::parse(&format!("{primary}-{chord}")).unwrap();
            assert_eq!(binding.match_keystrokes(&[stroke]), Some(false), "{chord}");
            assert!(binding.action().partial_eq(action), "{chord}");
            for extra in ["alt", if primary == "cmd" { "ctrl" } else { "cmd" }] {
                let stroke = Keystroke::parse(&format!("{extra}-{primary}-{chord}")).unwrap();
                assert_eq!(binding.match_keystrokes(&[stroke]), None, "{extra}-{chord}");
            }
        }
    }
}
