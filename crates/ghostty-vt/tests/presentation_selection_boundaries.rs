use shellfox_ghostty_vt::{HistoryPoint, Selection, Terminal, ViewportPoint};

#[test]
fn inclusive_wide_tail_and_equal_cell_endpoints_select_whole_graphemes(
) -> Result<(), Box<dyn std::error::Error>> {
    let mut terminal = Terminal::new(12, 3)?;
    terminal.feed(b"\x1b[?2027h")?;
    terminal.feed("A界e\u{301}👩‍💻Z".as_bytes())?;
    let mut view = terminal.new_presentation_view()?;
    for (anchor, focus, text) in [(1, 2, "界"), (2, 1, "界"), (3, 3, "e\u{301}"), (5, 4, "👩‍💻")]
    {
        terminal.view_select(
            &mut view,
            Some(Selection::Range {
                start: ViewportPoint { x: anchor, y: 0 },
                end: ViewportPoint { x: focus, y: 0 },
                rectangle: false,
            }),
        )?;
        let endpoints = terminal
            .view_selection(&mut view)?
            .ok_or("missing inclusive selection")?;
        assert_eq!(endpoints.anchor, HistoryPoint { x: anchor, row: 0 });
        assert_eq!(endpoints.focus, HistoryPoint { x: focus, row: 0 });
        assert_eq!(terminal.view_copy(&mut view)?.as_deref(), Some(text));
    }
    terminal.view_select(&mut view, None)?;
    assert!(terminal.view_selection(&mut view)?.is_none());
    Ok(())
}

#[test]
fn clustering_disabled_reports_actual_cells_not_reclustered_zwj_text(
) -> Result<(), Box<dyn std::error::Error>> {
    let mut terminal = Terminal::new(12, 3)?;
    terminal.feed(b"\x1b[?2027l")?;
    terminal.feed("A界e\u{301}👩‍💻Z".as_bytes())?;
    let mut view = terminal.new_presentation_view()?;
    let frame = terminal.view_render(&mut view)?;
    assert_eq!(frame.screen.cells[4].text, "👩\u{200d}");
    assert_eq!(frame.screen.cells[6].text, "💻");
    terminal.view_select(
        &mut view,
        Some(Selection::Range {
            start: ViewportPoint { x: 5, y: 0 },
            end: ViewportPoint { x: 4, y: 0 },
            rectangle: false,
        }),
    )?;
    let value = terminal
        .view_selection(&mut view)?
        .ok_or("missing actual cell range")?;
    assert_eq!(value.anchor.x, 5);
    assert_eq!(value.focus.x, 4);
    assert_eq!(
        terminal.view_copy(&mut view)?.as_deref(),
        Some("👩\u{200d}")
    );
    Ok(())
}
