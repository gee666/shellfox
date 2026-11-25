//! Keys typed into a selected tab while its terminal view is still attaching.
//! The loading pane holds focus and keeps them; the view gets them, in order,
//! once it is admitted. A new terminal used to drop everything typed before
//! its first frame.
use crate::{input, root::Root, terminal::TerminalView};
use gpui::{Context, FocusHandle, KeyDownEvent, Window};
use shellfox_ghostty_vt::key::{KeyAction, KeyEvent};
use shellfox_host::TabId;

/// About a few pages of fast typing; an attach that takes longer has stalled.
const MAX_KEYS: usize = 4096;

pub(crate) struct PendingInput {
    pub(crate) tab: TabId,
    pub(crate) focus: FocusHandle,
    keys: Vec<KeyEvent>,
}

impl PendingInput {
    fn record(&mut self, event: &KeyDownEvent) -> bool {
        let Some(key) = kept_key(event).filter(|_| self.keys.len() < MAX_KEYS) else {
            return false;
        };
        self.keys.push(key);
        true
    }

    /// Sends every kept key to the view. A key's release went to the loading
    /// pane, so it follows its press here.
    pub(crate) fn replay(self, view: &mut TerminalView) {
        for key in self.keys {
            let release = key.identity().map(|_| key.release());
            view.send_key(key);
            if let Some(release) = release {
                view.send_key(release);
            }
        }
    }
}

/// The press a terminal view would send for this key, or the text it would
/// commit. Clipboard shortcuts need a view and are left out.
fn kept_key(event: &KeyDownEvent) -> Option<KeyEvent> {
    let stroke = &event.keystroke;
    if input::clipboard_shortcut(stroke).is_some() {
        return None;
    }
    let action = if event.is_held {
        KeyAction::Repeat
    } else {
        KeyAction::Press
    };
    input::key_event(stroke, action)
        .or_else(|| {
            stroke
                .key_char
                .as_ref()
                .filter(|_| !stroke.modifiers.control && !stroke.modifiers.platform)
                .map(|text| KeyEvent::text(text.as_str()))
        })
        .filter(|key| key.validate().is_ok())
}

impl Root {
    /// Starts keeping keys for `tab` and moves focus to its loading pane.
    pub(crate) fn hold_input(&mut self, tab: TabId, window: &mut Window, cx: &mut Context<Self>) {
        if self
            .tabs
            .pending
            .as_ref()
            .is_none_or(|pending| pending.tab != tab)
        {
            self.tabs.pending = Some(PendingInput {
                tab,
                focus: cx.focus_handle(),
                keys: Vec::new(),
            });
        }
        if let Some(pending) = &self.tabs.pending {
            pending.focus.focus(window);
        }
    }

    pub(crate) fn take_pending_input(&mut self, tab: TabId) -> Option<PendingInput> {
        self.tabs.pending.take_if(|pending| pending.tab == tab)
    }

    pub(crate) fn pending_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        if self
            .tabs
            .pending
            .as_mut()
            .is_some_and(|pending| pending.record(event))
        {
            cx.stop_propagation();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn down(spec: &str, key_char: Option<&str>) -> KeyDownEvent {
        let mut keystroke = gpui::Keystroke::parse(spec).unwrap();
        keystroke.key_char = key_char.map(str::to_owned);
        KeyDownEvent {
            keystroke,
            is_held: false,
        }
    }

    #[test]
    fn typed_text_control_keys_and_enter_are_kept_but_clipboard_shortcuts_are_not() {
        let text = kept_key(&down("a", Some("a"))).unwrap();
        assert_eq!(text.text, "a");
        let enter = kept_key(&down("enter", None)).unwrap();
        assert!(enter.identity().is_some());
        assert!(kept_key(&down("ctrl-u", None)).is_some());
        let paste = if cfg!(target_os = "macos") {
            "cmd-v"
        } else {
            "ctrl-shift-v"
        };
        assert!(kept_key(&down(paste, None)).is_none());
    }
}
