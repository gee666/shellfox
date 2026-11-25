use shellfox_ghostty_vt::{Rgb, Terminal, TerminalColors};
fn rgb(r: u8, g: u8, b: u8) -> Rgb {
    Rgb { r, g, b }
}
fn colors() -> TerminalColors {
    TerminalColors {
        foreground: rgb(220, 221, 222),
        background: rgb(10, 11, 12),
        cursor: Some(rgb(180, 181, 182)),
        palette: Some([rgb(40, 41, 42); 256]),
    }
}

#[test]
fn colors_change_parser_defaults_existing_cells_and_palette(
) -> Result<(), Box<dyn std::error::Error>> {
    let mut terminal = Terminal::new(10, 3)?;
    terminal.feed(b"A\x1b[31mB\x1b[38;2;1;2;3mC\x1b[0m")?;
    let mut theme = colors();
    terminal.set_colors(&theme)?;
    terminal.feed(b"\x1b]12;?\x1b\\")?;
    assert_eq!(
        terminal.drain_replies()?,
        b"\x1b]12;rgb:b4b4/b5b5/b6b6\x1b\\"
    );
    let screen = terminal.render()?;
    assert_eq!(screen.cells[0].foreground, theme.foreground);
    assert_eq!(screen.cells[0].background, theme.background);
    assert_eq!(screen.cells[1].foreground, rgb(40, 41, 42));
    assert_eq!(screen.cells[2].foreground, rgb(1, 2, 3));
    theme.foreground = rgb(110, 111, 112);
    theme.background = rgb(20, 21, 22);
    terminal.set_colors(&theme)?;
    terminal.feed(b"D\x1b[38;5;255mE")?;
    let screen = terminal.render()?;
    assert_eq!(screen.cells[0].foreground, theme.foreground);
    assert_eq!(screen.cells[3].foreground, theme.foreground);
    assert_eq!(screen.cells[4].foreground, rgb(40, 41, 42));
    assert!(screen
        .cells
        .iter()
        .all(|c| c.background == theme.background));
    terminal.feed(b"\x1b[?1049hX")?;
    assert_eq!(terminal.render()?.cells[0].background, theme.background);
    terminal.feed(b"\x1b[?1049l")?;
    assert_eq!(terminal.render()?.cells[0].background, theme.background);
    Ok(())
}

#[test]
fn application_color_overrides_and_resets_follow_ghostty() -> Result<(), Box<dyn std::error::Error>>
{
    let mut terminal = Terminal::new(10, 3)?;
    let mut theme = colors();
    terminal.set_colors(&theme)?;
    terminal
        .feed(b"A\x1b]10;#123456\x07\x1b]11;#654321\x07\x1b]4;1;#abcdef\x07\x1b[31mB\x1b[0m")?;
    theme.foreground = rgb(110, 111, 112);
    terminal.set_colors(&theme)?;
    let screen = terminal.render()?;
    assert_eq!(screen.cells[0].foreground, rgb(0x12, 0x34, 0x56));
    assert_eq!(screen.cells[0].background, rgb(0x65, 0x43, 0x21));
    assert_eq!(screen.cells[1].foreground, rgb(0xab, 0xcd, 0xef));
    terminal.feed(b"\x1b]110\x07\x1b]111\x07\x1b]104;1\x07")?;
    let screen = terminal.render()?;
    assert_eq!(screen.cells[0].foreground, theme.foreground);
    assert_eq!(screen.cells[0].background, theme.background);
    assert_eq!(screen.cells[1].foreground, rgb(40, 41, 42));
    // None restores the real Ghostty palette; no renderer-only substitution.
    theme.palette = None;
    terminal.set_colors(&theme)?;
    assert_ne!(terminal.render()?.cells[1].foreground, rgb(40, 41, 42));
    Ok(())
}

#[test]
fn cells_report_how_each_color_was_set() -> Result<(), Box<dyn std::error::Error>> {
    use shellfox_ghostty_vt::ColorSource::{Default, Palette, Rgb as Explicit};
    let mut terminal = Terminal::new(10, 2)?;
    // ECH with a palette background stores the color in the erased cells.
    terminal.feed(b"A\x1b[31;48;5;200mB\x1b[38;2;1;2;3;49mC\x1b[0m\x1b[44m\x1b[2X")?;
    let expect = |cells: &[shellfox_ghostty_vt::Cell]| {
        let sources: Vec<_> = cells[..5]
            .iter()
            .map(|c| (c.foreground_source, c.background_source))
            .collect();
        assert_eq!(
            sources,
            [
                (Default, Default),
                (Palette(1), Palette(200)),
                (Explicit, Default),
                (Default, Palette(4)),
                (Default, Palette(4)),
            ]
        );
    };
    expect(&terminal.render()?.cells);
    let mut view = terminal.new_presentation_view()?;
    expect(&terminal.view_render(&mut view)?.screen.cells);
    Ok(())
}
