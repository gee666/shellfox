use gpui::{App, Font, Hsla, Pixels, Point, Window, px};

pub(super) fn first(placeholder: &str) -> &str {
    placeholder.lines().next().unwrap_or_default()
}

#[derive(Clone, Copy)]
pub(super) struct Style<'a> {
    pub font: &'a Font,
    pub color: Hsla,
    pub size: f32,
    pub height: f32,
}
impl<'a> Style<'a> {
    pub(super) fn new(font: &'a Font, color: Hsla, size: f32, height: f32) -> Self {
        Self {
            font,
            color,
            size,
            height,
        }
    }
    pub(super) fn color(self, color: Hsla) -> Self {
        Self { color, ..self }
    }
}

pub(super) fn paint_tail(
    placeholder: &str,
    style: Style<'_>,
    mut origin: Point<Pixels>,
    window: &mut Window,
    cx: &mut App,
) {
    // Placeholders never add rows to the editable document or its selection.
    for line in placeholder.lines().skip(1) {
        origin.y += px(style.height);
        let shaped = super::super::text::shape(line, style.font, style.color, style.size, window);
        if let Err(error) = shaped.paint(origin, px(style.height), window, cx) {
            eprintln!("editor placeholder paint: {error}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn multiline_placeholder_only_shapes_one_line_at_a_time() {
        assert_eq!(first("# One per line\r\nNAME=value"), "# One per line");
        assert_eq!(first("folder path"), "folder path");
    }
}
