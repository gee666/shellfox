use super::*;
use gpui::{EntityInputHandler, Point, UTF16Selection, point, size};
use std::ops::Range;
impl EntityInputHandler for PeerTerminalView {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        actual: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let text: Vec<_> = self.preedit.encode_utf16().collect();
        let slice = text.get(range.clone())?;
        let text = String::from_utf16(slice).ok()?;
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
        if range.is_some_and(|range| range.start > range.end)
            || text.is_empty()
            || text.chars().any(char::is_control)
        {
            return;
        }
        self.admit(
            m::Input::Key {
                event: keys::committed(text),
            },
            cx,
        );
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
            self.preedit = text.into();
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
        let screen = self.screen.as_ref()?;
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
