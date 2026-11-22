use crate::{editor_composition::EditorOutcome, root::Root};
use gpui::{Context, KeyDownEvent, KeyUpEvent, Keystroke, Window};

#[derive(Debug, PartialEq, Eq)]
enum LayoutShortcut {
    Sessions,
    Files,
    Folders,
}

fn shortcut(stroke: &Keystroke, mac: bool) -> Option<LayoutShortcut> {
    let m = stroke.modifiers;
    if m.alt
        || if mac {
            !m.platform || m.control
        } else {
            !m.control || m.platform
        }
    {
        return None;
    }
    match (stroke.key.as_str(), m.shift) {
        ("b", false) => Some(LayoutShortcut::Sessions),
        ("b", true) => Some(LayoutShortcut::Files),
        ("e", true) => Some(LayoutShortcut::Folders),
        _ => None,
    }
}

fn key_bit(key: &str) -> u8 {
    match key {
        "b" => 1,
        "e" => 2,
        _ => 0,
    }
}

impl Root {
    pub(crate) fn layout_key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let bit = key_bit(&event.keystroke.key);
        if bit == 0 {
            return;
        }
        if event.is_held {
            if self.layout_keys & bit != 0 {
                cx.stop_propagation();
            }
            return;
        }
        self.layout_keys &= !bit;
        let Some(action) = shortcut(&event.keystroke, cfg!(target_os = "macos")) else {
            return;
        };
        let dialog_blocks = match &self.dialog {
            Some(crate::root::Dialog::RenameTab { title, .. }) => title.read(cx).is_composing(),
            None => false,
            Some(_) => true,
        };
        if self.closing_window
            || dialog_blocks
            || self.session_menu.is_some()
            || self.sidebar_ui.menu_open()
            || self
                .settings_panel
                .as_ref()
                .is_some_and(|panel| self.settings && panel.read(cx).blocks_layout_shortcuts())
            || self.profile_menu.is_some()
            || self.tab_menu.is_some()
            || self.files.as_ref().is_some_and(|files| {
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
        {
            return;
        }
        match action {
            LayoutShortcut::Sessions => self.toggle_sessions_layout(window, cx),
            LayoutShortcut::Files if !self.settings && self.files_shortcut_available(cx) => {
                self.toggle_files_layout(window, cx);
            }
            LayoutShortcut::Folders
                if !self.settings
                    && self.files_shortcut_available(cx)
                    && self.files.as_ref().is_some_and(|files| files.visible)
                    && self.files_source_current() =>
            {
                self.toggle_file_browser(window, cx)
            }
            _ => return,
        }
        self.layout_keys |= bit;
        cx.stop_propagation();
        cx.notify();
    }

    pub(crate) fn layout_key_up(
        &mut self,
        event: &KeyUpEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let bit = key_bit(&event.keystroke.key);
        if self.layout_keys & bit != 0 {
            self.layout_keys &= !bit;
            cx.stop_propagation();
        }
    }

    pub(crate) fn toggle_sessions_layout(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let restore = self.sidebar
            && (self.sidebar_ui.region.contains_focused(window, cx)
                || self.sidebar_ui.resizer.is_focused(window));
        self.sidebar = !self.sidebar;
        self.resizing_sidebar = false;
        self.sidebar_ui.resize_origin = None;
        if restore {
            self.sidebar_ui.toggle.focus(window);
        }
        cx.notify();
    }

    pub(crate) fn toggle_files_layout(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let restore = self
            .files
            .as_ref()
            .is_some_and(|files| files.visible && files.panel_focus.contains_focused(window, cx));
        self.toggle_files(cx);
        if restore && self.files.as_ref().is_none_or(|files| !files.visible) {
            if self.is_local_source() && self.selected_tab().is_none() {
                self.focus_files_toggle(window, cx);
            } else {
                self.focus_selected(window, cx);
            }
        }
    }

    fn layout_terminal(&self) -> Option<&gpui::Entity<crate::app::TerminalView>> {
        if self.is_local_source() {
            return self.selected_tab().and_then(|tab| self.views.get(&tab));
        }
        self.peers.selected_peer().and_then(|(peer, state)| {
            self.peer_ui
                .v1_views
                .get(&(peer.clone(), state.selected_tab?))
        })
    }

    fn files_shortcut_available(&self, cx: &gpui::App) -> bool {
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
    use LayoutShortcut::*;

    #[test]
    fn layout_modifiers_and_actions_match_electron() {
        for (key, desktop, mac) in [
            ("ctrl-b", Some(Sessions), None),
            ("cmd-b", None, Some(Sessions)),
            ("ctrl-shift-b", Some(Files), None),
            ("cmd-shift-b", None, Some(Files)),
            ("ctrl-shift-e", Some(Folders), None),
            ("cmd-shift-e", None, Some(Folders)),
            ("ctrl-e", None, None),
            ("ctrl-alt-b", None, None),
            ("cmd-ctrl-b", None, None),
        ] {
            let stroke = Keystroke::parse(key).unwrap();
            assert_eq!(shortcut(&stroke, false), desktop, "{key}");
            assert_eq!(shortcut(&stroke, true), mac, "{key}");
        }
    }
}
