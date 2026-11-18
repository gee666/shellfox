//! TTY presentation of an actual installed Ghostty Screen. These escapes draw
//! cells on the user's terminal; they are NEVER used to recover the owner parser.
use anyhow::{Result, ensure};
use crossterm::{
    cursor, queue,
    style::{Attribute, Color, Print, SetAttribute, SetBackgroundColor, SetForegroundColor},
};
use shellfox_ghostty_vt::{Rgb, Screen};
use std::io::Write;
fn color(rgb: Rgb) -> Color {
    Color::Rgb {
        r: rgb.r,
        g: rgb.g,
        b: rgb.b,
    }
}
pub(crate) fn paint(screen: &Screen, viewport: (u16, u16), output: &mut impl Write) -> Result<()> {
    ensure!(
        screen.cells.len() == usize::from(screen.cols) * usize::from(screen.rows),
        "Invalid native frame dimensions."
    );
    queue!(output, cursor::Hide)?;
    for row in 0..screen.rows.min(viewport.1) {
        queue!(output, cursor::MoveTo(0, row))?;
        for (col, cell) in screen.cells[usize::from(row) * usize::from(screen.cols)
            ..usize::from(row + 1) * usize::from(screen.cols)]
            .iter()
            .take(usize::from(viewport.0))
            .enumerate()
        {
            if cell.width == 0 {
                continue;
            }
            if col + usize::from(cell.width) > usize::from(viewport.0) {
                break;
            }
            let (fg, bg) = if cell.inverse {
                (cell.background, cell.foreground)
            } else {
                (cell.foreground, cell.background)
            };
            queue!(
                output,
                SetAttribute(Attribute::Reset),
                SetForegroundColor(color(fg)),
                SetBackgroundColor(color(bg))
            )?;
            for (enabled, attribute) in [
                (cell.bold, Attribute::Bold),
                (cell.faint, Attribute::Dim),
                (cell.italic, Attribute::Italic),
                (cell.underline, Attribute::Underlined),
                (cell.strikethrough, Attribute::CrossedOut),
            ] {
                if enabled {
                    queue!(output, SetAttribute(attribute))?;
                }
            }
            let glyph: String = if cell.invisible || cell.text.is_empty() {
                " ".repeat(usize::from(cell.width))
            } else {
                cell.text
                    .chars()
                    .map(|ch| if ch.is_control() { '\u{fffd}' } else { ch })
                    .collect()
            };
            queue!(output, Print(glyph))?;
        }
    }
    queue!(output, SetAttribute(Attribute::Reset))?;
    if screen.cursor.visible
        && screen.cursor.x < screen.cols.min(viewport.0)
        && screen.cursor.y < screen.rows.min(viewport.1)
    {
        queue!(
            output,
            cursor::MoveTo(screen.cursor.x, screen.cursor.y),
            cursor::Show
        )?;
    }
    output.flush()?;
    Ok(())
}
