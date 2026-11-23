use super::{buffer, placeholder::Style, shape};
use crate::theme::Palette;
use gpui::{Bounds, Pixels, Point, ShapedLine, TextAlign, Window, fill, point, px, size};
use std::ops::Range;

pub(super) fn alignment(align: TextAlign, width: f32, available: f32) -> f32 {
    let space = (available - width).max(0.);
    match align {
        TextAlign::Left => 0.,
        TextAlign::Center => space / 2.,
        TextAlign::Right => space,
    }
}

pub(super) fn horizontal_scroll(
    text: &str,
    cursor: usize,
    secret: bool,
    style: &Style<'_>,
    viewport: f32,
    current: f32,
    window: &Window,
) -> f32 {
    let width = |text: &str| {
        f32::from(
            shape(
                &buffer::display_text(text, secret),
                style.font,
                style.color,
                style.size,
                window,
            )
            .width,
        )
    };
    let visible = viewport.max(1.);
    let caret = width(&text[..cursor]);
    if width(text) <= visible {
        0.
    } else if caret < current {
        caret
    } else if caret + 3. > current + visible {
        caret + 3. - visible
    } else {
        current
    }
}

pub(super) struct SelectionStyle<'a> {
    pub origin: Point<Pixels>,
    pub height: f32,
    pub caret_adjust: f32,
    pub secret: bool,
    pub projection: Option<&'a super::super::editor_line::Projection>,
    pub palette: Palette,
}

pub(super) fn paint_selection(
    line: &ShapedLine,
    source: &str,
    line_start: usize,
    selection: &Range<usize>,
    style: SelectionStyle<'_>,
    window: &mut Window,
) {
    let start = selection.start.saturating_sub(line_start).min(source.len());
    let end = selection.end.saturating_sub(line_start).min(source.len());
    let index = |offset| {
        style.projection.map_or_else(
            || buffer::display_index(source, offset, style.secret),
            |projection| projection.display_index(offset),
        )
    };
    let left = line.x_for_index(index(start));
    let right = line.x_for_index(index(end));
    let bounds = Bounds::new(
        point(
            style.origin.x + left + px(style.caret_adjust),
            style.origin.y,
        ),
        size((right - left).max(px(1.)), px(style.height)),
    );
    let color = if start == end {
        style.palette.foreground
    } else {
        style.palette.accent.opacity(0.35)
    };
    window.paint_quad(fill(bounds, color));
}
