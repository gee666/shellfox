//! Shared paint only. Screen data is real Ghostty state, never a lease/authority DTO.
use super::{TerminalTheme, color, paints_glyph, resolved_colors};
use anyhow::Result;
use gpui::{
    App, Bounds, Font, FontStyle, FontWeight, Hsla, Pixels, TextRun, Window, fill, point, px, size,
};
use shellfox_ghostty_vt::Screen;
pub(crate) struct CellStyle<'a> {
    pub font: &'a Font,
    pub cell_width: f32,
    pub font_size: f32,
    pub line_height: f32,
    pub theme: TerminalTheme,
}
pub(crate) struct CellOverlay<R, C> {
    pub row: R,
    pub color: C,
}
pub(crate) fn paint_cells<R, C>(
    screen: &Screen,
    bounds: Bounds<Pixels>,
    style: CellStyle<'_>,
    mut overlay: CellOverlay<R, C>,
    window: &mut Window,
    cx: &mut App,
) -> Result<()>
where
    R: FnMut(usize, &mut Window),
    C: FnMut(usize, usize) -> Option<Hsla>,
{
    // Each background span is one quad. Glyphs stay on Ghostty's cell grid.
    for row in 0..usize::from(screen.rows) {
        let start = row * usize::from(screen.cols);
        let cells = &screen.cells[start..start + usize::from(screen.cols)];
        let y = bounds.top() + px(row as f32 * style.line_height);
        let background = |index: usize| {
            let cell = &cells[index];
            if cell.selected {
                style.theme.selection()
            } else {
                color(resolved_colors(cell).1)
            }
        };
        let mut col = 0;
        while col < cells.len() {
            let bg = background(col);
            let mut end = col + 1;
            while end < cells.len() && background(end) == bg {
                end += 1;
            }
            window.paint_quad(fill(
                Bounds::new(
                    point(bounds.left() + px(col as f32 * style.cell_width), y),
                    size(
                        px((end - col) as f32 * style.cell_width),
                        px(style.line_height),
                    ),
                ),
                bg,
            ));
            col = end;
        }
        (overlay.row)(row, window);
        for (col, cell) in cells.iter().enumerate() {
            let origin = point(bounds.left() + px(col as f32 * style.cell_width), y);
            if !paints_glyph(cell) {
                continue;
            }
            let mut font = (*style.font).clone();
            if cell.bold {
                font.weight = FontWeight::BOLD;
            }
            if cell.italic {
                font.style = FontStyle::Italic;
            }
            let overridden = (overlay.color)(col, row);
            let mut foreground = overridden.unwrap_or_else(|| color(resolved_colors(cell).0));
            if cell.faint && overridden.is_none() {
                foreground.a *= 0.5;
            }
            let mut chars = cell.text.chars();
            let sprite = match (chars.next(), chars.next()) {
                (Some(ch), None) => super::sprites::paint(
                    ch,
                    origin,
                    style.cell_width * f32::from(cell.width),
                    style.line_height,
                    foreground,
                    window,
                ),
                _ => false,
            };
            if !sprite {
                paint_glyph(cell, font, foreground, origin, &style, window, cx)?;
            }
            for offset in [
                cell.strikethrough.then_some(style.line_height * 0.5),
                cell.underline.then_some(style.line_height - 2.),
            ]
            .into_iter()
            .flatten()
            {
                window.paint_quad(fill(
                    Bounds::new(
                        origin + point(px(0.), px(offset)),
                        size(px(style.cell_width * f32::from(cell.width)), px(1.)),
                    ),
                    foreground,
                ));
            }
        }
    }

    Ok(())
}

fn paint_glyph(
    cell: &shellfox_ghostty_vt::Cell,
    font: Font,
    color: Hsla,
    origin: gpui::Point<Pixels>,
    style: &CellStyle<'_>,
    window: &mut Window,
    cx: &mut App,
) -> Result<()> {
    let run = TextRun {
        len: cell.text.len(),
        font,
        color,
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let line = window.text_system().shape_line(
        cell.text.clone().into(),
        px(style.font_size),
        &[run],
        None,
    );
    line.paint(origin, px(style.line_height), window, cx)?;
    Ok(())
}
