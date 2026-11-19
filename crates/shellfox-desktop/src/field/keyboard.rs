use super::Field;
use gpui::{Context, KeyDownEvent, Window, point, px};
use std::ops::Range;

impl Field {
    pub(super) fn key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let key = event.keystroke.key.as_str();
        let mods = event.keystroke.modifiers;
        // The OS owns composition editing. Ctrl+Alt is AltGr, not a clipboard chord.
        if native_owns_key(self.marked.is_some(), mods) {
            return;
        }
        let command = command_modifier(mods);
        if !self.multiline && matches!(key, "up" | "down") {
            if self.number_kind.is_some() {
                self.step(key == "up", window, cx);
                window.prevent_default();
                cx.stop_propagation();
            }
            return;
        }
        if self.asset_editor && key == "tab"
            || form_key_bubbles(self.multiline, self.code_editor, key, mods)
        {
            return;
        }
        if self.asset_editor
            && (command
                || matches!(
                    key,
                    "enter"
                        | "backspace"
                        | "delete"
                        | "left"
                        | "right"
                        | "home"
                        | "end"
                        | "up"
                        | "down"
                ))
        {
            self.history.break_group();
        }
        if self.asset_editor && command && matches!(key, "z" | "y") {
            self.undo_edit(key == "y" || mods.shift);
        } else if self.multiline && matches!(key, "enter" | "tab") {
            self.replace(None, if key == "tab" { "\t" } else { "\n" });
        } else if command && key == "a" {
            self.anchor = 0;
            self.set_cursor(self.text.len(), true);
        } else if command && key == "v" {
            if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                self.replace(None, &text);
            }
        } else if command && matches!(key, "c" | "x") {
            if self.secret {
                window.prevent_default();
                cx.stop_propagation();
                return;
            }
            cx.write_to_clipboard(gpui::ClipboardItem::new_string(
                self.text[self.selection.clone()].to_owned(),
            ));
            if key == "x" {
                self.replace(None, "");
            }
        } else if matches!(key, "backspace" | "delete") {
            if self.selection.is_empty() {
                if key == "backspace" {
                    self.selection.start = previous(&self.text, self.cursor);
                } else {
                    self.selection.end = next(&self.text, self.cursor);
                }
            }
            self.replace(None, "");
        } else if matches!(key, "left" | "right" | "home" | "end" | "up" | "down") {
            let target = self.wrapped_navigation(key, command).unwrap_or_else(|| {
                navigation_target(
                    &self.text,
                    self.selection.clone(),
                    self.cursor,
                    key,
                    mods.shift,
                    command,
                )
            });
            self.set_cursor(target, mods.shift);
        } else {
            return;
        }
        self.ensure_caret_visible();
        window.prevent_default();
        cx.stop_propagation();
        cx.notify();
    }
    pub(super) fn ensure_caret_visible(&self) {
        if !self.multiline {
            return;
        }
        let row = self.visual_row(self.cursor);
        let mut offset = self.scroll.offset();
        let top = row as f32 * self.line_height + self.multiline_text_inset();
        let viewport = f32::from(self.scroll.bounds().size.height);
        if viewport <= 0. {
            return;
        }
        if top + f32::from(offset.y) < 0. {
            offset.y = px(-top);
        } else if top + self.line_height + f32::from(offset.y) > viewport {
            offset.y = px(viewport - top - self.line_height);
        }
        self.scroll.set_offset(point(offset.x, offset.y));
    }
}
fn previous(text: &str, cursor: usize) -> usize {
    text[..cursor]
        .char_indices()
        .next_back()
        .map_or(0, |(index, _)| index)
}
fn next(text: &str, cursor: usize) -> usize {
    text[cursor..]
        .chars()
        .next()
        .map_or(cursor, |ch| cursor + ch.len_utf8())
}
fn navigation_target(
    text: &str,
    selected: Range<usize>,
    cursor: usize,
    key: &str,
    shift: bool,
    document: bool,
) -> usize {
    if !shift && !selected.is_empty() {
        if key == "left" {
            return selected.start;
        }
        if key == "right" {
            return selected.end;
        }
    }
    // Horizontal movement must not scan an entire multi-megabyte minified line.
    if key == "left" {
        return previous(text, cursor);
    }
    if key == "right" {
        return next(text, cursor);
    }
    let start = text[..cursor].rfind('\n').map_or(0, |index| index + 1);
    let end = text[cursor..]
        .find('\n')
        .map_or(text.len(), |index| cursor + index);
    match key {
        "home" => {
            if document {
                0
            } else {
                start
            }
        }
        "end" => {
            if document {
                text.len()
            } else {
                end
            }
        }
        "up" | "down" => {
            let column = text[start..cursor].chars().count();
            let target = if key == "up" {
                if start == 0 {
                    return cursor;
                }
                let end = start - 1;
                let start = text[..end].rfind('\n').map_or(0, |index| index + 1);
                start..end
            } else {
                if end == text.len() {
                    return cursor;
                }
                let start = end + 1;
                let end = text[start..]
                    .find('\n')
                    .map_or(text.len(), |index| start + index);
                start..end
            };
            text[target.clone()]
                .char_indices()
                .nth(column)
                .map_or(target.end, |(index, _)| target.start + index)
        }
        _ => cursor,
    }
}
fn form_key_bubbles(multiline: bool, code_editor: bool, key: &str, mods: gpui::Modifiers) -> bool {
    multiline
        && !code_editor
        && (key == "tab" || (key == "enter" && (mods.control || mods.platform)))
}

pub(super) fn native_owns_key(marked: bool, mods: gpui::Modifiers) -> bool {
    marked || mods.alt
}
pub(super) fn command_modifier(mods: gpui::Modifiers) -> bool {
    if cfg!(target_os = "macos") {
        mods.platform && !mods.control && !mods.alt
    } else {
        mods.control && !mods.platform && !mods.alt
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn form_textarea_leaves_tab_and_control_enter_for_parent_actions() {
        assert!(form_key_bubbles(
            true,
            false,
            "tab",
            gpui::Modifiers::default()
        ));
        assert!(form_key_bubbles(
            true,
            false,
            "enter",
            gpui::Modifiers {
                control: true,
                ..Default::default()
            }
        ));
        assert!(!form_key_bubbles(
            true,
            false,
            "enter",
            gpui::Modifiers::default()
        ));
        assert!(!form_key_bubbles(
            true,
            true,
            "tab",
            gpui::Modifiers::default()
        ));
    }
    #[test]
    fn select_all_arrow_collapses_to_the_correct_edge() {
        assert_eq!(navigation_target("a😀b", 0..6, 6, "left", false, false), 0);
        assert_eq!(navigation_target("a😀b", 0..6, 0, "right", false, false), 6);
        assert_eq!(navigation_target("a😀b", 0..6, 6, "left", true, false), 5);
    }
    #[test]
    fn vertical_navigation_preserves_unicode_columns_and_line_home_end() {
        let text = "a😀b\n界x\nlast";
        assert_eq!(navigation_target(text, 5..5, 5, "down", false, false), 11);
        assert_eq!(navigation_target(text, 11..11, 11, "up", false, false), 5);
        assert_eq!(navigation_target(text, 11..11, 11, "home", false, false), 7);
        assert_eq!(
            navigation_target(text, 11..11, 11, "end", false, true),
            text.len()
        );
    }
}
