use super::{
    Field,
    editor_line::Projection,
    snapshot::{LINE_WINDOW_BYTES, Snapshot},
    text::{base_font, shape},
};
use gpui::{Context, Font, Window, point, px};
use std::{ops::Range, sync::Arc};

#[derive(Clone)]
pub(super) struct HorizontalLayout {
    text: Arc<str>,
    pub width: f32,
    sections: Arc<[Section]>,
}

struct Section {
    range: Range<usize>,
    left: f32,
    right: f32,
    column: usize,
}

pub(super) struct Slice {
    pub range: Range<usize>,
    pub left: f32,
    pub column: usize,
}

impl HorizontalLayout {
    fn build(snapshot: &Snapshot, measure: &Measure<'_>) -> Self {
        let mut sections = Vec::new();
        let mut start = 0;
        let mut width = 0_f32;
        for line in snapshot.text.split('\n') {
            if line.len() <= LINE_WINDOW_BYTES {
                width = width.max(measure.width(line, 0).0);
            } else {
                let (mut left, mut column, mut offset) = (0., 0, 0);
                while offset < line.len() {
                    let mut end = (offset + LINE_WINDOW_BYTES).min(line.len());
                    while !line.is_char_boundary(end) {
                        end -= 1;
                    }
                    let (advance, next_column) = measure.width(&line[offset..end], column);
                    sections.push(Section {
                        range: start + offset..start + end,
                        left,
                        right: left + advance,
                        column,
                    });
                    left += advance;
                    column = next_column;
                    offset = end;
                }
                width = width.max(left);
            }
            start += line.len() + 1;
        }
        Self {
            text: snapshot.text.clone(),
            width,
            sections: sections.into(),
        }
    }

    pub fn slice(&self, range: Range<usize>, visible: Range<f32>) -> Slice {
        let first = self
            .sections
            .partition_point(|section| section.range.start < range.start);
        let last = self
            .sections
            .partition_point(|section| section.range.start < range.end);
        let sections = &self.sections[first..last];
        if sections.is_empty() {
            return Slice {
                range,
                left: 0.,
                column: 0,
            };
        }
        let first = sections
            .partition_point(|section| section.right < visible.start)
            .min(sections.len() - 1);
        let last = sections
            .partition_point(|section| section.left <= visible.end)
            .saturating_sub(1)
            .max(first);
        let section = &sections[first];
        Slice {
            range: section.range.start..sections[last].range.end,
            left: section.left,
            column: section.column,
        }
    }

    fn x_for_cursor(&self, cursor: usize, line: Range<usize>, measure: &Measure<'_>) -> f32 {
        let index = self
            .sections
            .partition_point(|section| section.range.start <= cursor);
        let (start, left, column) = index
            .checked_sub(1)
            .and_then(|index| self.sections.get(index))
            .filter(|section| section.range.start >= line.start && section.range.end >= cursor)
            .map_or((line.start, 0., 0), |section| {
                (section.range.start, section.left, section.column)
            });
        left + measure.width(&self.text[start..cursor], column).0
    }
}

struct Measure<'a> {
    window: &'a Window,
    font: &'a Font,
    size: f32,
    cell: f32,
}
impl Measure<'_> {
    fn width(&self, text: &str, column: usize) -> (f32, usize) {
        if text.is_ascii() && !text.contains('\t') {
            let count = text.strip_suffix('\r').unwrap_or(text).len();
            return (self.cell * count as f32, column + count);
        }
        let (display, _, next) = Projection::build(text, column);
        let foreground = self.window.text_style().color;
        let width = shape(&display, self.font, foreground, self.size, self.window).width;
        (f32::from(width), next)
    }
}

impl Field {
    pub(crate) fn set_file_viewport(&mut self, width: f32, wrap: bool, cx: &mut Context<Self>) {
        if self.file_viewport != Some(width) {
            self.file_viewport = Some(width);
            cx.notify();
        }
        self.set_wrap_width(wrap.then_some((width - 16.).max(8.)), cx);
        if wrap && self.nowrap.take().is_some() {
            self.scroll
                .set_offset(point(px(0.), self.scroll.offset().y));
            self.follow_horizontal = true;
        }
    }

    pub(super) fn update_horizontal(&mut self, window: &Window) {
        let Some(viewport) = self.file_viewport else {
            return;
        };
        if self.wrap_width.is_some() {
            return;
        }
        let font = base_font(self.monospace, self.font_size, self.code_editor, window);
        let cell =
            f32::from(shape(" ", &font, self.palette.foreground, self.font_size, window).width);
        let measure = Measure {
            window,
            font: &font,
            size: self.font_size,
            cell,
        };
        let layout = self
            .nowrap
            .get_or_insert_with(|| HorizontalLayout::build(&self.snapshot, &measure));
        if !Arc::ptr_eq(&layout.text, &self.snapshot.text) {
            *layout = HorizontalLayout::build(&self.snapshot, &measure);
        }
        if std::mem::take(&mut self.follow_horizontal)
            && self.focus.is_focused(window)
            && let Some(line) = self.snapshot.line(self.snapshot.row(self.cursor))
        {
            let x = layout.x_for_cursor(self.cursor, line, &measure);
            let previous = -f32::from(self.scroll.offset().x);
            let visible = (viewport - 16.).max(1.);
            let offset = if x < previous {
                x
            } else if x + 1. > previous + visible {
                x + 1. - visible
            } else {
                previous
            };
            self.scroll
                .set_offset(point(px(-offset.max(0.)), self.scroll.offset().y));
        }
    }
}
