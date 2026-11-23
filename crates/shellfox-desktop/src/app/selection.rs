use super::{Backend, TerminalView, runtime::Pending};
use crate::input::{self, ClipboardAction, mac_select_all, page_scroll};
use gpui::{ClipboardItem, Context, KeyDownEvent, KeyUpEvent, Window};
use shellfox_ghostty_vt::key::KeyAction;
use shellfox_ghostty_vt::{Selection, ViewportPoint};
use shellfox_runtime::Command;

impl TerminalView {
    fn paste(&mut self, cx: &mut Context<Self>) {
        if self.failed || self.closing {
            return;
        }
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()).filter(|text| !text.is_empty()) {
            if let Some(lease) = self.lease {
                self.submit(Command::Paste { lease, text }, Pending::Applied);
            } else if let Err(error) = self.peer_paste(&text) {
                self.reject(error);
            }
        }
    }

    /// The macOS Edit menu items, for the focused terminal.
    pub(super) fn menu_copy(&mut self, _: &crate::app_menu::Copy, _: &mut Window, cx: &mut Context<Self>) {
        self.copy_selection(cx);
    }

    pub(super) fn menu_paste(&mut self, _: &crate::app_menu::Paste, _: &mut Window, cx: &mut Context<Self>) {
        self.paste(cx);
    }

    pub(super) fn menu_select_all(&mut self, _: &crate::app_menu::SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.select(Some(Selection::All), cx);
    }

    pub(super) fn key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.link_menu_key(event, window, cx) { return; }
        let bit = input::clipboard_key_bit(&event.keystroke.key);
        match input::clipboard_shortcut(&event.keystroke) {
            Some(ClipboardAction::Copy) => {
                self.clipboard_keys |= bit;
                self.copy_selection(cx);
                cx.stop_propagation();
            }
            Some(ClipboardAction::Paste) => {
                self.clipboard_keys |= bit;
                self.paste(cx);
                cx.stop_propagation();
            }
            None => {
                if cfg!(target_os = "macos") && mac_select_all(&event.keystroke) {
                    self.select(Some(Selection::All), cx);
                    cx.stop_propagation();
                } else if let Some(delta) = page_scroll(&event.keystroke, self.screen.rows) {
                    self.scroll_lines(delta);
                    cx.stop_propagation();
                } else if let Some(key) = input::key_event(
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
        let bit = input::clipboard_key_bit(&event.keystroke.key);
        if self.clipboard_keys & bit != 0
            || input::clipboard_shortcut(&event.keystroke).is_some()
            || page_scroll(&event.keystroke, self.screen.rows).is_some()
        {
            self.clipboard_keys &= !bit;
            cx.stop_propagation();
        }
    }

    pub(super) fn copy_selection(&mut self, cx: &mut Context<Self>) -> bool {
        if self.failed {
            return false;
        }
        let selected = self.screen.cells.iter().any(|cell| cell.selected);
        let text = match &mut self.backend {
            Backend::Fixture(terminal) => Some(terminal.copy_selection()),
            Backend::Peer(peer) => Some(peer.terminal.view_copy(&mut peer.presentation)),
            Backend::Attached(_) => None,
        };
        if let Some(text) = text {
            match text {
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
            }
        } else if let Backend::Attached(connection) = &self.backend {
            let view = connection.access.presentation_lease();
            self.submit(
                Command::Presentation { view, action: shellfox_runtime::PresentationAction::Copy },
                Pending::Copy,
            );
        }
        selected
    }

    pub(super) fn begin_pointer_selection(&mut self, point: ViewportPoint, clicks: usize, mods: gpui::Modifiers, cx: &mut Context<Self>) {
        if mods.shift && !self.mouse_tracking {
            let selection = match &self.screen {
                super::SharedScreen::Native(frame) => frame.selection.zip(Some(frame.top_row)),
                super::SharedScreen::Peer(frame) => frame.selection.zip(Some(frame.rendered.top_row)),
                _ => None,
            }.and_then(|(previous, top)| {
                let y = previous.anchor.row.checked_sub(top)?;
                if y >= u64::from(self.screen.rows) { return None; }
                let start = ViewportPoint { x: previous.anchor.x, y: y as u16 };
                Some((start, previous.rectangle))
            });
            if let Some((start, rectangle)) = selection {
                self.selection_drag.begin(start, rectangle);
                let selection = self.selection_drag.extend(point);
                self.select(selection, cx);
            }
            return;
        }
        match clicks {
            2 => self.select(Some(Selection::Word(point)), cx),
            3 => self.select(Some(Selection::Line(point)), cx),
            1 => {
                self.select(None, cx);
                self.selection_drag.begin(point, mods.alt && !cfg!(target_os = "macos"));
                self.link_press = Some(point);
            }
            _ => {}
        }
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
    rectangle: bool,
    pub moved: bool,
}

impl SelectionDrag {
    pub fn begin(&mut self, point: ViewportPoint, rectangle: bool) {
        self.anchor = Some(point);
        self.rectangle = rectangle;
        self.moved = false;
    }

    pub fn extend(&mut self, point: ViewportPoint) -> Option<Selection> {
        let start = self.anchor?;
        self.moved |= start.x != point.x || start.y != point.y;
        self.moved.then_some(Selection::Range {
            start,
            end: point,
            rectangle: self.rectangle,
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
    fn shift_page_keys_scroll_one_row_less_than_the_viewport() {
        for (key, expected) in [
            ("shift-pageup", Some(-23)),
            ("ctrl-shift-pagedown", Some(23)),
            ("ctrl-pageup", None),
            ("shift-up", None),
        ] {
            assert_eq!(page_scroll(&gpui::Keystroke::parse(key).unwrap(), 24), expected);
        }
    }

    #[test]
    fn drag_uses_native_unicode_and_soft_wrap_copy() {
        let mut terminal = shellfox_ghostty_vt::Terminal::new(12, 3).unwrap();
        terminal.feed("A界e\u{301}🙂Z".as_bytes()).unwrap();
        let mut drag = SelectionDrag::default();
        drag.begin(ViewportPoint { x: 5, y: 0 }, false);
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
        drag.begin(ViewportPoint { x: 0, y: 0 }, false);
        wrapped
            .set_selection(drag.extend(ViewportPoint { x: 4, y: 1 }))
            .unwrap();
        assert_eq!(
            wrapped.copy_selection().unwrap().as_deref(),
            Some("abcdefghij")
        );
    }

    #[test]
    fn column_drag_retains_rectangular_selection_through_release() {
        let mut drag = SelectionDrag::default();
        drag.begin(ViewportPoint { x: 1, y: 1 }, true);
        assert!(matches!(drag.extend(ViewportPoint { x: 2, y: 3 }), Some(Selection::Range { rectangle: true, .. })));
        drag.finish();
        assert!(!drag.active());
    }

    #[test]
    fn click_does_not_select_and_release_stops_dragging() {
        let mut drag = SelectionDrag::default();
        let start = ViewportPoint { x: 2, y: 1 };
        drag.begin(start, false);
        assert!(drag.active());
        assert!(drag.extend(start).is_none());
        assert!(drag.extend(ViewportPoint { x: 5, y: 3 }).is_some());
        assert!(drag.moved);
        drag.finish();
        assert!(!drag.active());
        assert!(drag.extend(start).is_none());
    }
}
