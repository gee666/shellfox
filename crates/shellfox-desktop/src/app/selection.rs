use super::{Backend, TerminalView, runtime::Pending};
use crate::input;
use gpui::{ClipboardItem, Context, KeyDownEvent, KeyUpEvent, Window};
use shellfox_ghostty_vt::KeyAction;
use shellfox_ghostty_vt::{Selection, ViewportPoint};
use shellfox_runtime::Command;

impl TerminalView {
    fn paste(&mut self, cx: &mut Context<Self>) {
        if self.failed || self.closing {
            return;
        }
        if let Some(lease) = self.lease
            && let Some(text) = cx.read_from_clipboard().and_then(|item| item.text())
        {
            self.submit(Command::Paste { lease, text }, Pending::Applied);
        }
    }

    pub(super) fn key_down(
        &mut self,
        event: &KeyDownEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let bit = Self::clipboard_key_bit(&event.keystroke.key);
        if event.is_held && self.clipboard_keys & bit != 0 {
            cx.stop_propagation();
            return;
        }
        match Self::clipboard_shortcut(event.keystroke.modifiers, &event.keystroke.key) {
            Some("copy") => {
                self.clipboard_keys |= bit;
                self.copy_selection(cx);
                cx.stop_propagation();
            }
            Some("paste") => {
                self.clipboard_keys |= bit;
                self.paste(cx);
                cx.stop_propagation();
            }
            _ => {
                if let Some(key) = input::encode_physical(
                    &event.keystroke,
                    if event.is_held {
                        KeyAction::Repeat
                    } else {
                        KeyAction::Press
                    },
                ) {
                    self.send_key(key);
                    cx.stop_propagation();
                }
            }
        }
        cx.notify();
    }

    pub(super) fn key_up(&mut self, event: &KeyUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        let bit = Self::clipboard_key_bit(&event.keystroke.key);
        if self.clipboard_keys & bit != 0
            || Self::clipboard_shortcut(event.keystroke.modifiers, &event.keystroke.key).is_some()
        {
            self.clipboard_keys &= !bit;
            cx.stop_propagation();
        } else if let Some(key) = input::encode_physical(&event.keystroke, KeyAction::Release) {
            self.send_key(key);
            cx.stop_propagation();
        }
    }

    fn clipboard_key_bit(key: &str) -> u8 {
        match key {
            "c" => 1,
            "v" => 2,
            _ => 0,
        }
    }

    pub(super) fn clipboard_shortcut(mods: gpui::Modifiers, key: &str) -> Option<&'static str> {
        let enabled = if cfg!(target_os = "macos") {
            mods.platform && !mods.control && !mods.alt
        } else {
            mods.control && mods.shift && !mods.alt && !mods.platform
        };
        if !enabled {
            return None;
        }
        match key {
            "c" => Some("copy"),
            "v" => Some("paste"),
            _ => None,
        }
    }

    pub(super) fn copy_selection(&mut self, cx: &mut Context<Self>) -> bool {
        if self.failed {
            return false;
        }
        let selected = self.screen.cells.iter().any(|cell| cell.selected);
        match &mut self.backend {
            Backend::Fixture(terminal) => match terminal.copy_selection() {
                Ok(Some(text)) => {
                    cx.write_to_clipboard(ClipboardItem::new_string(text));
                    return true;
                }
                Ok(None) => return false,
                Err(error) => {
                    self.fail(error);
                    cx.notify();
                    return false;
                }
            },
            Backend::Attached(connection) => {
                let view = connection.access.presentation_lease();
                self.submit(
                    Command::Presentation {
                        view,
                        action: shellfox_runtime::PresentationAction::Copy,
                    },
                    Pending::Copy,
                );
            }
        }
        selected
    }

    pub(super) fn select(&mut self, selection: Option<Selection>, cx: &mut Context<Self>) {
        let result = self.apply_presentation(
            shellfox_runtime::PresentationAction::Selection { selection },
            |terminal| terminal.set_selection(selection),
        );
        match result {
            Ok(()) => {
                // Only an accepted, known local selection gesture resets this
                // view's clock. This is not output/authority/frame revision.
                if let Some(epoch) = self.cursor_reset_epoch.checked_add(1) {
                    self.cursor_reset_epoch = epoch;
                } else {
                    self.fail("View cursor reset epoch exhausted");
                }
            }
            Err(error) => self.status = format!("Selection rejected: {error}"),
        }
        cx.notify();
    }
}

/// Drag bookkeeping stays separate from the terminal's authoritative selection.
#[derive(Default)]
pub struct SelectionDrag {
    anchor: Option<ViewportPoint>,
    pub moved: bool,
}

impl SelectionDrag {
    pub fn begin(&mut self, point: ViewportPoint) {
        self.anchor = Some(point);
        self.moved = false;
    }

    pub fn extend(&mut self, point: ViewportPoint) -> Option<Selection> {
        let start = self.anchor?;
        self.moved |= start.x != point.x || start.y != point.y;
        self.moved.then_some(Selection::Range {
            start,
            end: point,
            rectangle: false,
        })
    }

    pub fn active(&self) -> bool {
        self.anchor.is_some()
    }

    pub fn finish(&mut self) {
        self.anchor = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copy_and_paste_shortcuts_require_platform_modifiers() {
        let modifiers = if cfg!(target_os = "macos") {
            gpui::Modifiers {
                platform: true,
                ..Default::default()
            }
        } else {
            gpui::Modifiers {
                control: true,
                shift: true,
                ..Default::default()
            }
        };
        assert_eq!(
            TerminalView::clipboard_shortcut(modifiers, "c"),
            Some("copy")
        );
        assert_eq!(
            TerminalView::clipboard_shortcut(modifiers, "v"),
            Some("paste")
        );
        assert_eq!(
            TerminalView::clipboard_shortcut(gpui::Modifiers::default(), "c"),
            None
        );
        assert_eq!(
            TerminalView::clipboard_shortcut(
                gpui::Modifiers {
                    alt: true,
                    ..modifiers
                },
                "c"
            ),
            None
        );
    }

    #[test]
    fn drag_uses_native_unicode_and_soft_wrap_copy() {
        let mut terminal = shellfox_ghostty_vt::Terminal::new(12, 3).unwrap();
        terminal.feed("A界e\u{301}🙂Z".as_bytes()).unwrap();
        let mut drag = SelectionDrag::default();
        drag.begin(ViewportPoint { x: 5, y: 0 });
        terminal
            .set_selection(drag.extend(ViewportPoint { x: 1, y: 0 }))
            .unwrap();
        assert_eq!(
            terminal.copy_selection().unwrap().as_deref(),
            Some("界e\u{301}🙂")
        );
        assert!(terminal.render().unwrap().cells[1].selected);
        let mut wrapped = shellfox_ghostty_vt::Terminal::new(5, 3).unwrap();
        wrapped.feed(b"abcdefghij").unwrap();
        drag.begin(ViewportPoint { x: 0, y: 0 });
        wrapped
            .set_selection(drag.extend(ViewportPoint { x: 4, y: 1 }))
            .unwrap();
        assert_eq!(
            wrapped.copy_selection().unwrap().as_deref(),
            Some("abcdefghij")
        );
    }

    #[test]
    fn click_does_not_select_and_release_stops_dragging() {
        let mut drag = SelectionDrag::default();
        let start = ViewportPoint { x: 2, y: 1 };
        drag.begin(start);
        assert!(drag.active());
        assert!(drag.extend(start).is_none());
        assert!(drag.extend(ViewportPoint { x: 5, y: 3 }).is_some());
        assert!(drag.moved);
        drag.finish();
        assert!(!drag.active());
        assert!(drag.extend(start).is_none());
    }
}
