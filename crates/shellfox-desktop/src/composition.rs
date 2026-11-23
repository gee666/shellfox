use crate::app::TerminalView;
use shellfox_ghostty_vt::key::KeyEvent;
use gpui::{
    Bounds, Context, EntityInputHandler, Pixels, Point, UTF16Selection, Window, point, px, size,
};
use std::ops::Range;

// A terminal has no editable document. Only uncommitted composition is editable.
// Selection belongs to Ghostty. Native marked text remains separate from PTY input.
impl EntityInputHandler for TerminalView {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        actual: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let utf16: Vec<u16> = self.preedit.encode_utf16().collect();
        if range.end > utf16.len() || range.start > range.end {
            return None;
        }
        let text = String::from_utf16(&utf16[range.clone()]).ok()?;
        *actual = Some(range);
        Some(text)
    }

    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        let end = self.preedit.encode_utf16().count();
        Some(UTF16Selection {
            range: end..end,
            reversed: false,
        })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        (!self.preedit.is_empty()).then(|| 0..self.preedit.encode_utf16().count())
    }

    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.preedit.clear();
        cx.notify();
    }

    fn replace_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.preedit.clear();
        if range.as_ref().is_some_and(|range| range.start > range.end) {
            return;
        }
        if let Some(event) = commit(text) {
            self.send_key(event);
        }
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        _: Option<Range<usize>>,
        text: &str,
        _: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if text.len() <= 64 * 1024 {
            self.preedit = text.to_owned();
        }
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        _: Range<usize>,
        bounds: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let screen = &self.screen;
        Some(Bounds::new(
            bounds.origin
                + point(
                    px(f32::from(screen.cursor.x) * self.cell_width),
                    px(f32::from(screen.cursor.y) * self.line_height),
                ),
            size(px(self.cell_width), px(self.line_height)),
        ))
    }

    fn character_index_for_point(
        &mut self,
        _: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        None
    }
}

/// A native commit (IME, dead key, emoji picker, dictation) is terminal text,
/// sent exactly once. Marked text never is. Controls arrive as key events.
fn commit(text: &str) -> Option<KeyEvent> {
    (!text.is_empty() && shellfox_ghostty_vt::key::validate_text(text).is_ok())
        .then(|| KeyEvent::text(text))
}

#[cfg(test)]
mod tests {
    use super::commit;
    use shellfox_ghostty_vt::Terminal;

    #[test]
    fn ime_dead_key_and_emoji_commits_are_text_in_every_keyboard_mode() {
        for mode in [&b""[..], b"\x1b[>1u", b"\x1b[>31u"] {
            let mut terminal = Terminal::new(80, 24).unwrap();
            terminal.feed(mode).unwrap();
            for text in ["日本語", "한국어", "é", "ñ", "🦊👍🏽", "a\u{301}"] {
                let event = commit(text).unwrap();
                assert_eq!(terminal.encode_key_event(&event).unwrap(), text.as_bytes(), "{text}");
            }
        }
        assert!(commit("").is_none());
        assert!(commit("\x1b[A").is_none());
        assert!(commit("a\nb").is_none());
    }
}
