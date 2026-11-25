//! A view restored from a v1 snapshot and then fitted to its own size must
//! stay in step with the host, so a TUI's cursor-relative redraws land on the
//! same rows in both.
use super::*;

/// pi-style frame: padded history, a status block, an editor holding the
/// cursor and a footer, written with CRLF line ends. No soft-wrapped rows: the
/// vt-1.0.1 formatter writes them as hard lines, so they reflow differently.
fn frame(cols: usize, elapsed: u32) -> Vec<u8> {
    let mut out = String::from("\x1b[2J\x1b[H\x1b[3J");
    for i in 0..80 {
        out.push_str(&format!("{:<cols$}\r\n", format!(" message {i}")));
    }
    out.push_str(&format!("{:<cols$}\r\n", format!(" Elapsed {elapsed}s")));
    out.push_str(&format!("{:<cols$}\r\n", " Working"));
    out.push_str(&"-".repeat(cols));
    out.push_str("\r\n\r\n");
    out.push_str(&"-".repeat(cols));
    out.push_str("\r\nWITH SUBS footer\r\nbrowser: connected");
    // Hardware cursor back on the editor row.
    out.push_str("\x1b[3A\r");
    out.into_bytes()
}

/// pi's differential update: up to the changed line, rewrite it, come back.
fn redraw(elapsed: u32) -> Vec<u8> {
    format!("\x1b[?2026h\x1b[3A\r\x1b[2K Elapsed {elapsed}s\x1b[3B\r\x1b[?2026l").into_bytes()
}

fn text(terminal: &mut Terminal) -> Vec<String> {
    let screen = terminal.render().unwrap();
    let cols = usize::from(screen.cols);
    let rows: Vec<String> = screen
        .cells
        .chunks(cols)
        .map(|row| {
            let line: String = row.iter().map(|c| c.text.as_str()).collect();
            line.trim_end().to_owned()
        })
        .collect();
    let cursor = format!("cursor {},{}", screen.cursor.x, screen.cursor.y);
    rows.into_iter().chain(std::iter::once(cursor)).collect()
}

#[test]
fn snapshot_view_fitted_to_another_size_stays_in_step_with_its_host() {
    for (host, view) in [
        ((200, 50), (60, 50)),
        ((60, 24), (60, 50)),
        ((120, 30), (80, 20)),
    ] {
        let mut source = Terminal::new(host.0, host.1).unwrap();
        source.feed(&frame(usize::from(host.0), 1)).unwrap();
        let snapshot = remote_v1_ansi(&mut source).unwrap();
        let mut restored = Terminal::new(host.0, host.1).unwrap();
        restored.feed(&snapshot).unwrap();
        assert_eq!(text(&mut restored), text(&mut source), "{host:?}");
        // The view fits locally first; the PTY follows, and the TUI keeps
        // redrawing relative to its cursor until it notices the new size.
        restored.resize(view.0, view.1).unwrap();
        source.resize(view.0, view.1).unwrap();
        for elapsed in 2..5 {
            restored.feed(&redraw(elapsed)).unwrap();
            source.feed(&redraw(elapsed)).unwrap();
        }
        assert_eq!(
            text(&mut restored),
            text(&mut source),
            "{host:?} -> {view:?}"
        );
        let full = frame(usize::from(view.0), 5);
        restored.feed(&full).unwrap();
        source.feed(&full).unwrap();
        assert_eq!(
            text(&mut restored),
            text(&mut source),
            "{host:?} -> {view:?}"
        );
    }
}
