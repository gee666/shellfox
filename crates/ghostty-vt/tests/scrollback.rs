//! Scrollback keeps at least xterm's 3000 lines, as the Electron terminal did.
use shellfox_ghostty_vt::Terminal;

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// Writes full-width colored lines and returns how many history rows remain.
fn retained_rows(cols: u16, lines: usize) -> Result<u64, Box<dyn std::error::Error>> {
    let mut terminal = Terminal::new(cols, 24)?;
    let fill = "x".repeat(usize::from(cols) - 6);
    for line in 1..=lines {
        terminal.feed(format!("\x1b[3{}m{line:05} {fill}\r\n", line % 8).as_bytes())?;
    }
    let mut view = terminal.new_presentation_view()?;
    Ok(terminal.view_render(&mut view)?.total_rows)
}

#[test]
fn five_thousand_lines_keep_at_least_three_thousand_in_history() -> TestResult {
    for cols in [80, 200] {
        let rows = retained_rows(cols, 5000)?;
        assert!(rows >= 3000 + 24, "{cols} columns kept only {rows} rows");
    }
    Ok(())
}

#[test]
fn history_stays_bounded() -> TestResult {
    let rows = retained_rows(200, 20_000)?;
    assert!(
        (3000..20_000).contains(&rows),
        "200 columns kept {rows} rows"
    );
    Ok(())
}

/// Bytes Windows ConPTY sent for `1..60` typed into an interactive PowerShell
/// at 80x24: a screen clear, then plain CRLF lines at the bottom row.
#[test]
fn conpty_output_reaches_scrollback() -> TestResult {
    let mut stream =
        b"\x1b[?9001h\x1b[?1004h\x1b[?25l\x1b[2J\x1b[m\x1b[HPS C:\\Users\\user>\x1b[1C\
        \x1b]0;powershell.exe\x07\x1b[?25h\x1b[?25l\x1b[1;17H> \x1b[97m1\x1b[90m..\x1b[97m60\r\n\
        \x1b[?25h\x1b[m"
            .to_vec();
    for line in 1..=60 {
        stream.extend_from_slice(format!("{line}\r\n").as_bytes());
    }
    stream.extend_from_slice(b"PS C:\\Users\\user> ");
    let mut terminal = Terminal::new(80, 24)?;
    terminal.feed(&stream)?;
    let mut view = terminal.new_presentation_view()?;
    let frame = terminal.view_render(&mut view)?;
    assert_eq!(frame.total_rows, 62);
    assert_eq!(frame.top_row, 38);
    terminal.view_scroll(&mut view, -38)?;
    let frame = terminal.view_render(&mut view)?;
    assert_eq!(frame.top_row, 0);
    let first: String = frame.screen.cells[..23]
        .iter()
        .map(|c| c.text.as_str())
        .collect();
    assert!(first.starts_with("PS C:\\Users\\user> 1..60"), "{first:?}");
    Ok(())
}
