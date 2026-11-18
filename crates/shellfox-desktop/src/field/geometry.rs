use super::{Field, buffer};
use gpui::{Bounds, Pixels, Point, ShapedLine, Window, point, px, size};
use std::ops::Range;

pub(super) struct PaintedLine {
    pub source: Range<usize>,
    pub line: ShapedLine,
    pub origin: Point<Pixels>,
    pub caret_adjust: f32,
}
pub(super) fn index_at(field: &Field, position: Point<Pixels>, _: &Window) -> Option<usize> {
    let line = field.painted_lines.iter().find(|line| {
        position.y >= line.origin.y && position.y < line.origin.y + px(field.line_height)
    })?;
    let source = field.text.get(line.source.clone())?;
    if source.is_empty() {
        return Some(line.source.start);
    }
    let index = line
        .line
        .closest_index_for_x(position.x - line.origin.x)
        .min(line.line.len());
    let index = buffer::source_index(source, index, field.secret);
    source
        .is_char_boundary(index)
        .then_some(line.source.start + index)
}
pub(super) fn caret_bounds(
    field: &Field,
    index: usize,
    _: Bounds<Pixels>,
    _: &Window,
) -> Option<Bounds<Pixels>> {
    let line = field
        .painted_lines
        .iter()
        .find(|line| index >= line.source.start && index <= line.source.end)?;
    let source = field.text.get(line.source.clone())?;
    let index = index.checked_sub(line.source.start)?;
    if !source.is_char_boundary(index) {
        return None;
    }
    let display = buffer::display_index(source, index, field.secret);
    Some(Bounds::new(
        point(
            line.origin.x + line.line.x_for_index(display) + px(line.caret_adjust),
            line.origin.y,
        ),
        size(px(1.), px(field.line_height)),
    ))
}
