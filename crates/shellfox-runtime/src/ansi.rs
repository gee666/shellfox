use crate::{RuntimeError, MAX_SCREEN_CELLS};
use shellfox_ghostty_vt::Screen;
use std::fmt::Write;

const MAX_ANSI_BYTES: usize = 4 * 1024 * 1024;

/// Public ANSI only, deliberately no Ghostty snapshot bytes. This rebuilds the
/// visible cell image, not scrollback, active-buffer or application input modes.
/// A caller must explicitly request this limited recovery capability.
pub(crate) fn visible_screen(screen: &Screen) -> Result<Vec<u8>, RuntimeError> {
    let cells = usize::from(screen.cols) * usize::from(screen.rows);
    if cells > MAX_SCREEN_CELLS || cells != screen.cells.len() {
        return Err(RuntimeError::validation("invalid screen dimensions"));
    }
    let mut out = String::from("\x1b[0m\x1b[?25l\x1b[2J\x1b[H");
    // SGR survives cursor moves, so a run of equally styled cells is written
    // once: the text stays plain text, and the replay a fraction of the size.
    let mut current = None;
    for y in 0..screen.rows {
        let _ = write!(out, "\x1b[{};1H", y + 1);
        for x in 0..screen.cols {
            let cell = &screen.cells[usize::from(y) * usize::from(screen.cols) + usize::from(x)];
            if cell.width == 0 {
                continue;
            }
            let style = sgr(cell);
            if current.as_ref() != Some(&style) {
                out.push_str(&style);
                current = Some(style);
            }
            if cell.text.is_empty() {
                out.push(' ');
            } else {
                out.extend(cell.text.chars().filter(|c| !c.is_control()));
            }
            if out.len() > MAX_ANSI_BYTES {
                return Err(RuntimeError::validation("ANSI recovery exceeds 4 MiB"));
            }
        }
    }
    let _ = write!(
        out,
        "\x1b[0m\x1b[{};{}H\x1b[?25{}",
        screen.cursor.y + 1,
        screen.cursor.x + 1,
        if screen.cursor.visible { 'h' } else { 'l' }
    );
    Ok(out.into_bytes())
}

/// The complete style of one cell, from a reset.
fn sgr(cell: &shellfox_ghostty_vt::Cell) -> String {
    let (fg, bg) = (cell.foreground, cell.background);
    let mut style = format!(
        "\x1b[0;38;2;{};{};{};48;2;{};{};{}",
        fg.r, fg.g, fg.b, bg.r, bg.g, bg.b
    );
    for (enabled, code) in [
        (cell.bold, 1),
        (cell.faint, 2),
        (cell.italic, 3),
        (cell.underline, 4),
        (cell.inverse, 7),
        (cell.invisible, 8),
        (cell.strikethrough, 9),
    ] {
        if enabled {
            let _ = write!(style, ";{code}");
        }
    }
    style.push('m');
    style
}

#[cfg(test)]
mod tests {
    use super::*;
    use shellfox_ghostty_vt::Terminal;
    #[test]
    fn ansi_roundtrip_is_text_not_binary_and_does_not_emit_query_replies() {
        let mut source = Terminal::new(20, 3).unwrap();
        source
            .feed(b"\x1b[31mhello\x1b[0m\r\nworld\x1b[2;4H")
            .unwrap();
        let screen = source.render().unwrap();
        let bytes = visible_screen(&screen).unwrap();
        assert!(std::str::from_utf8(&bytes).is_ok());
        let mut client = Terminal::new(20, 3).unwrap();
        client.feed(&bytes).unwrap();
        assert!(client.drain_replies().unwrap().is_empty());
        let restored = client.render().unwrap();
        let normalize = |cells: Vec<shellfox_ghostty_vt::Cell>| {
            cells
                .into_iter()
                .map(|mut cell| {
                    if cell.text.is_empty() {
                        cell.text.push(' ');
                    }
                    cell
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(normalize(restored.cells), normalize(screen.cells));
        assert_eq!(restored.cursor, screen.cursor);
        // Equally styled cells form one run of plain text, as a shell prompt.
        let text = String::from_utf8(bytes).unwrap();
        assert!(text.contains("hello") && text.contains("world"), "{text}");
        assert_eq!(text.matches("\x1b[0;38;2;").count(), 2, "{text}");
    }
}
