use crate::{RuntimeError, MAX_SCREEN_CELLS};
use shellfox_ghostty_vt::{ColorSource, Rgb, Screen};
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

/// The complete style of one cell, from a reset. Default and palette colors
/// stay default and palette, so the receiver's own theme shows them.
fn sgr(cell: &shellfox_ghostty_vt::Cell) -> String {
    let mut style = String::from("\x1b[0");
    color(&mut style, cell.foreground_source, cell.foreground, 30);
    color(&mut style, cell.background_source, cell.background, 40);
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
/// `base` is 30 for the foreground, 40 for the background.
fn color(style: &mut String, source: ColorSource, rgb: Rgb, base: u8) {
    let _ = match source {
        ColorSource::Default => write!(style, ";{}", base + 9),
        ColorSource::Palette(index @ 0..=7) => write!(style, ";{}", base + index),
        ColorSource::Palette(index @ 8..=15) => write!(style, ";{}", base + 60 + index - 8),
        ColorSource::Palette(index) => write!(style, ";{};5;{index}", base + 8),
        ColorSource::Rgb => write!(style, ";{};2;{};{};{}", base + 8, rgb.r, rgb.g, rgb.b),
    };
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
        assert_eq!(text.matches("\x1b[0;").count(), 2, "{text}");
    }

    #[test]
    fn default_and_palette_colors_stay_default_and_palette() {
        let mut source = Terminal::new(40, 2).unwrap();
        source
            .feed(b"plain\x1b[31mred\x1b[38;5;200;44mmix\x1b[0m\x1b[38;2;1;2;3;48;2;4;5;6mrgb\x1b[0m\x1b[97;100mbright")
            .unwrap();
        let bytes = visible_screen(&source.render().unwrap()).unwrap();
        let text = String::from_utf8(bytes.clone()).unwrap();
        for run in [
            "\x1b[0;39;49mplain",
            "\x1b[0;31;49mred",
            "\x1b[0;38;5;200;44mmix",
            "\x1b[0;38;2;1;2;3;48;2;4;5;6mrgb",
            "\x1b[0;97;100mbright",
        ] {
            assert!(text.contains(run), "{run:?} in {text:?}");
        }
        // A receiver with its own theme shows it, and keeps every source.
        let mut client = Terminal::new(40, 2).unwrap();
        let mut palette = [Rgb {
            r: 10,
            g: 10,
            b: 10,
        }; 256];
        palette[1] = Rgb { r: 250, g: 1, b: 1 };
        client
            .set_colors(&shellfox_ghostty_vt::TerminalColors {
                foreground: Rgb {
                    r: 200,
                    g: 201,
                    b: 202,
                },
                background: Rgb {
                    r: 20,
                    g: 21,
                    b: 22,
                },
                cursor: None,
                palette: Some(palette),
            })
            .unwrap();
        client.feed(&bytes).unwrap();
        let restored = client.render().unwrap();
        let at = |x: usize| &restored.cells[x];
        assert_eq!(at(0).foreground_source, ColorSource::Default);
        assert_eq!(at(0).background_source, ColorSource::Default);
        assert_eq!(
            at(0).foreground,
            Rgb {
                r: 200,
                g: 201,
                b: 202
            }
        );
        assert_eq!(
            at(0).background,
            Rgb {
                r: 20,
                g: 21,
                b: 22
            }
        );
        assert_eq!(at(5).foreground_source, ColorSource::Palette(1));
        assert_eq!(at(5).foreground, Rgb { r: 250, g: 1, b: 1 });
        assert_eq!(at(8).foreground_source, ColorSource::Palette(200));
        assert_eq!(at(8).background_source, ColorSource::Palette(4));
        assert_eq!(at(11).foreground_source, ColorSource::Rgb);
        assert_eq!(at(11).foreground, Rgb { r: 1, g: 2, b: 3 });
        assert_eq!(at(11).background, Rgb { r: 4, g: 5, b: 6 });
        assert_eq!(at(14).foreground_source, ColorSource::Palette(15));
        assert_eq!(at(14).background_source, ColorSource::Palette(8));
    }
}
