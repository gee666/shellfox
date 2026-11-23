use crate::terminal::TerminalView;
use gpui::{
    Bounds, Context, EntityInputHandler, Pixels, Point, UTF16Selection, Window, point, px, size,
};
use shellfox_ghostty_vt::key::{KeyEvent, KeyModifiers, PhysicalKey, validate_text};
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
        for event in commit(text) {
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
/// sent exactly once; line breaks become Enter. Marked text never is sent.
/// Other controls cannot be typed and are dropped.
fn commit(text: &str) -> Vec<KeyEvent> {
    let mut events = Vec::new();
    for (i, line) in text.split('\n').enumerate() {
        if i > 0 {
            let enter = KeyEvent::press(PhysicalKey::Enter, KeyModifiers::NONE);
            events.push(enter.release());
            events.insert(events.len() - 1, enter);
        }
        let line: String = line
            .chars()
            .filter(|c| validate_text(c.encode_utf8(&mut [0; 4])).is_ok())
            .collect();
        if !line.is_empty() {
            events.push(KeyEvent::text(line));
        }
    }
    events
}

#[cfg(test)]
mod tests {
    use super::commit;
    use shellfox_ghostty_vt::Terminal;

    fn bytes(terminal: &mut Terminal, text: &str) -> Vec<u8> {
        commit(text)
            .iter()
            .flat_map(|e| terminal.encode_key_event(e).unwrap())
            .collect()
    }

    #[test]
    fn ime_dead_key_and_emoji_commits_are_text_in_every_keyboard_mode() {
        for mode in [&b""[..], b"\x1b[>1u", b"\x1b[>31u"] {
            let mut terminal = Terminal::new(80, 24).unwrap();
            terminal.feed(mode).unwrap();
            for text in ["日本語", "한국어", "é", "ñ", "🦊👍🏽", "a\u{301}"] {
                assert_eq!(bytes(&mut terminal, text), text.as_bytes(), "{text}");
            }
        }
    }

    #[test]
    fn dictated_line_breaks_are_enter_and_controls_are_dropped() {
        let mut terminal = Terminal::new(80, 24).unwrap();
        assert!(commit("").is_empty());
        assert_eq!(bytes(&mut terminal, "\x1b[A"), b"[A");
        assert_eq!(bytes(&mut terminal, "ls\npwd"), b"ls\rpwd");
    }
}
