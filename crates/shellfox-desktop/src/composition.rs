use crate::app::TerminalView;
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
        // Native commits are encoded as terminal text exactly once. Controls use key events.
        if !text.is_empty() && shellfox_ghostty_vt::key::validate_text(text).is_ok() {
            self.send_key(shellfox_ghostty_vt::key::KeyEvent::text(text));
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
