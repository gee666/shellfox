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
    assert!((3000..20_000).contains(&rows), "200 columns kept {rows} rows");
    Ok(())
}
