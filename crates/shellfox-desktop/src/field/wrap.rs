use super::{Field, snapshot::Snapshot, text::base_font};
use gpui::{Window, px};
use std::{ops::Range, sync::Arc};

#[derive(Clone)]
pub(super) struct WrapLayout {
    text: Arc<str>,
    width: f32,
    // Only soft breaks add index entries. A file with 8 million hard lines
    // does not need another 8 million records to enable wrapping.
    breaks: Arc<[SoftBreak]>,
}

struct SoftBreak {
    row: usize,
    offset: usize,
}

impl WrapLayout {
    fn build(
        snapshot: &Snapshot,
        width: f32,
        mut split: impl FnMut(&str, &mut dyn FnMut(usize)),
    ) -> Self {
        let mut breaks = Vec::new();
        let mut start = 0;
        for (logical, line) in snapshot.text.split('\n').enumerate() {
            split(line, &mut |index| {
                if index != 0
                    && index < line.len()
                    && line.is_char_boundary(index)
                    && breaks
                        .last()
                        .is_none_or(|last: &SoftBreak| last.offset < start + index)
                {
                    breaks.push(SoftBreak {
                        row: logical + breaks.len() + 1,
                        offset: start + index,
                    });
                }
            });
            start += line.len() + 1;
        }
        Self {
            text: snapshot.text.clone(),
            width,
            breaks: breaks.into(),
        }
    }

    pub fn matches(&self, snapshot: &Snapshot, width: f32) -> bool {
        Arc::ptr_eq(&self.text, &snapshot.text) && self.width == width
    }

    pub fn count(&self, snapshot: &Snapshot) -> usize {
        snapshot.line_count() + self.breaks.len()
    }

    pub fn row(&self, snapshot: &Snapshot, offset: usize) -> usize {
        snapshot.row(offset) + self.breaks.partition_point(|split| split.offset <= offset)
    }

    pub fn line(&self, snapshot: &Snapshot, row: usize) -> Option<Range<usize>> {
        let prior = self.breaks.partition_point(|split| split.row <= row);
        let hard_line = snapshot.line(row - prior)?;
        let start = prior
            .checked_sub(1)
            .map(|i| &self.breaks[i])
            .filter(|prev| prev.row == row)
            .map_or(hard_line.start, |prev| prev.offset);
        let end = self
            .breaks
            .get(prior)
            .filter(|next| next.row == row + 1 && next.offset < hard_line.end)
            .map_or(hard_line.end, |next| next.offset);
        Some(start..end)
    }

    pub fn visible_rows(
        &self,
        snapshot: &Snapshot,
        top: f32,
        height: f32,
        clip: Range<f32>,
    ) -> Range<usize> {
        if clip.end <= clip.start || height <= 0. {
            return 0..0;
        }
        let count = self.count(snapshot);
        let row = |y: f32| ((y - top) / height).max(0.);
        ((row(clip.start).floor() as usize).min(count))
            ..((row(clip.end).ceil() as usize).min(count))
    }
}

impl Field {
    pub(crate) fn set_wrap_width(&mut self, width: Option<f32>, cx: &mut gpui::Context<Self>) {
        if self.wrap_width != width {
            self.wrap_width = width;
            self.wrap_layout = None;
            self.measured_semantics = None;
            cx.notify();
        }
    }

    pub(super) fn update_wrap(&mut self, window: &Window) {
        let Some(width) = self.wrap_width else {
            return;
        };
        if self
            .wrap_layout
            .as_ref()
            .is_some_and(|layout| layout.matches(&self.snapshot, width))
        {
            return;
        }
        let font = base_font(self.monospace, self.font_size, self.code_editor, window);
        let mut wrapper = window.text_system().line_wrapper(font, px(self.font_size));
        let asset = self.asset_editor;
        self.wrap_layout = Some(WrapLayout::build(&self.snapshot, width, |line, emit| {
            if line.is_empty() {
                return;
            }
            if asset && (line.contains('\t') || line.ends_with('\r')) {
                let (display, projection, _) = super::editor_line::Projection::build(line, 0);
                for boundary in wrapper.wrap_line(&[gpui::LineFragment::text(&display)], px(width))
                {
                    emit(projection.source_index(boundary.ix));
                }
            } else {
                for boundary in wrapper.wrap_line(&[gpui::LineFragment::text(line)], px(width)) {
                    emit(boundary.ix);
                }
            }
        }));
        self.scroll_x = 0.;
        if self.focus.is_focused(window) {
            self.ensure_caret_visible();
        }
    }

    pub(super) fn visual_row(&self, offset: usize) -> usize {
        self.wrap_layout
            .as_ref()
            .filter(|layout| {
                self.wrap_width
                    .is_some_and(|w| layout.matches(&self.snapshot, w))
            })
            .map_or_else(
                || self.snapshot.row(offset),
                |wrap| wrap.row(&self.snapshot, offset),
            )
    }

    pub(super) fn wrapped_navigation(&self, key: &str, ctrl: bool) -> Option<usize> {
        if ctrl || !matches!(key, "home" | "end" | "up" | "down") {
            return None;
        }
        let layout = self.wrap_layout.as_ref().filter(|layout| {
            self.wrap_width
                .is_some_and(|w| layout.matches(&self.snapshot, w))
        })?;
        let row = layout.row(&self.snapshot, self.cursor);
        let source = layout.line(&self.snapshot, row)?;
        match key {
            "home" => Some(source.start),
            "end" => Some(source.end),
            "up" | "down" => {
                let Some(target) = row
                    .checked_add_signed(if key == "up" { -1 } else { 1 })
                    .and_then(|row| layout.line(&self.snapshot, row))
                else {
                    return Some(self.cursor);
                };
                let column = self.text[source.start..self.cursor].chars().count();
                Some(
                    self.text[target.clone()]
                        .char_indices()
                        .nth(column)
                        .map_or(target.end, |(index, _)| target.start + index),
                )
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn soft_wraps_preserve_utf8_offsets_blank_lines_and_the_original_buffer() {
        let snapshot = Snapshot::new("界ab\n\ncd\n");
        let wrap = WrapLayout::build(&snapshot, 15., |line, emit| {
            if line == "界ab" {
                emit(1);
                emit(3);
                emit(4);
            }
        });
        assert_eq!(wrap.count(&snapshot), 6);
        let ranges = (0..6)
            .map(|row| wrap.line(&snapshot, row).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(ranges, [0..3, 3..4, 4..5, 6..6, 7..9, 10..10]);
        assert_eq!(wrap.row(&snapshot, 4), 2);
        assert_eq!(wrap.row(&snapshot, 7), 4);
        assert_eq!(&*snapshot.text, "界ab\n\ncd\n");
    }
}
