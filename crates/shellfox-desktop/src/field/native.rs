use super::{Field, buffer::byte_offset, paint};
use gpui::{Bounds, Context, EntityInputHandler, Pixels, Point, UTF16Selection, Window};
use std::ops::Range;

impl EntityInputHandler for Field {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        actual: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        if self.secret {
            *actual = None;
            return None;
        }
        let bytes = self.byte_range(range.clone())?;
        if bytes.start > bytes.end {
            return None;
        }
        *actual = Some(range);
        Some(self.text[bytes].to_owned())
    }
    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.utf16_range(self.selection.clone())?,
            reversed: self.cursor < self.anchor,
        })
    }
    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked
            .clone()
            .and_then(|range| self.utf16_range(range))
    }
    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.marked = None;
        cx.notify();
    }
    fn replace_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.replace(range, text);
        self.ensure_caret_visible();
        cx.notify();
    }
    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        selection: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(marked) = self.replace(range, text) {
            if let Some(selection) = selection.and_then(|range| {
                Some(byte_offset(text, range.start)?..byte_offset(text, range.end)?)
            }) && selection.start <= selection.end
            {
                self.anchor = marked.start + selection.start;
                self.set_cursor(marked.start + selection.end, true);
            }
            self.marked = (!text.is_empty()).then_some(marked);
        }
        self.ensure_caret_visible();
        cx.notify();
    }
    fn bounds_for_range(
        &mut self,
        range: Range<usize>,
        bounds: Bounds<Pixels>,
        window: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let index = self.snapshot.byte_for_utf16(range.start)?;
        paint::caret_bounds(self, index, bounds, window)
    }
    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        window: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        let index = paint::index_at(self, point, window)?;
        self.snapshot.utf16_for_byte(index)
    }
}
