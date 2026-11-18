use shellfox_ghostty_vt::{Error, Selection, Terminal, ViewportPoint};
fn p(x: u16, y: u16) -> ViewportPoint {
    ViewportPoint { x, y }
}
fn range(start: ViewportPoint, end: ViewportPoint, rectangle: bool) -> Selection {
    Selection::Range {
        start,
        end,
        rectangle,
    }
}

#[test]
fn unicode_copy_reversed_range_and_selected_render() -> Result<(), Box<dyn std::error::Error>> {
    let mut terminal = Terminal::new(12, 3)?;
    assert_eq!(terminal.copy_selection()?, None);
    terminal.feed("A界e\u{301}🙂Z".as_bytes())?;
    terminal.set_selection(Some(range(p(1, 0), p(5, 0), false)))?;
    let copy = terminal.copy_selection()?.ok_or("missing selection")?;
    assert_eq!(copy, "界e\u{301}🙂");
    let screen = terminal.render()?;
    assert!(!screen.cells[0].selected);
    assert!(screen.cells[1].selected);
    assert!(screen.cells[5].selected);
    assert!(!screen.cells[6].selected);
    terminal.set_selection(Some(range(p(5, 0), p(1, 0), false)))?;
    assert_eq!(terminal.copy_selection()?.as_deref(), Some(copy.as_str()));
    terminal.set_selection(None)?;
    assert_eq!(terminal.copy_selection()?, None);
    assert!(terminal.render()?.cells.iter().all(|c| !c.selected));
    drop(terminal);
    assert_eq!(copy, "界e\u{301}🙂");
    assert!(screen.cells[1].selected);
    Ok(())
}

#[test]
fn word_line_rectangle_wrap_and_all_use_ghostty() -> Result<(), Box<dyn std::error::Error>> {
    let mut terminal = Terminal::new(12, 3)?;
    terminal.feed("one 界e\u{301} two\r\nnext row".as_bytes())?;
    terminal.set_selection(Some(Selection::Word(p(0, 0))))?;
    assert_eq!(terminal.copy_selection()?.as_deref(), Some("one"));
    terminal.set_selection(Some(Selection::Line(p(4, 0))))?;
    assert_eq!(
        terminal.copy_selection()?.as_deref(),
        Some("one 界e\u{301} two")
    );
    terminal.set_selection(Some(range(p(0, 0), p(2, 1), true)))?;
    assert_eq!(terminal.copy_selection()?.as_deref(), Some("one\nnex"));
    terminal.set_selection(Some(Selection::All))?;
    assert_eq!(
        terminal.copy_selection()?.as_deref(),
        Some("one 界e\u{301} two\nnext row")
    );
    let mut wrapped = Terminal::new(5, 3)?;
    wrapped.feed(b"abcdefghij")?;
    wrapped.set_selection(Some(range(p(0, 0), p(4, 1), false)))?;
    assert_eq!(wrapped.copy_selection()?.as_deref(), Some("abcdefghij"));
    Ok(())
}

#[test]
fn selections_stay_owned_across_mutations_and_bounds_fail() -> Result<(), Box<dyn std::error::Error>>
{
    let mut terminal = Terminal::new(12, 3)?;
    terminal.feed(b"alpha beta")?;
    terminal.set_selection(Some(Selection::Word(p(1, 0))))?;
    for selection in [
        Selection::Word(p(12, 0)),
        Selection::Line(p(0, 3)),
        range(p(0, 0), p(12, 0), false),
    ] {
        assert_eq!(
            terminal.set_selection(Some(selection)).unwrap_err(),
            Error(-2)
        );
        assert_eq!(terminal.copy_selection()?.as_deref(), Some("alpha"));
    }
    terminal.resize(6, 4)?;
    assert_eq!(terminal.copy_selection()?.as_deref(), Some("alpha"));
    terminal.feed(b"\x1b[?1049h")?;
    assert_eq!(terminal.copy_selection()?, None);
    terminal.feed(b"\x1b[Halternate")?;
    terminal.set_selection(Some(Selection::All))?;
    assert_eq!(terminal.copy_selection()?.as_deref(), Some("alternate"));
    terminal.feed(b"\x1b[?1049l")?;
    // Ghostty clears the selection on an application screen switch.
    assert_eq!(terminal.copy_selection()?, None);
    terminal.scroll(-1)?;
    terminal.copy_selection()?; // Fresh refs after every mutation, including viewport movement.
    Ok(())
}

#[test]
fn hyperlink_metadata_is_owned_untrusted_and_viewport_scoped(
) -> Result<(), Box<dyn std::error::Error>> {
    let mut terminal = Terminal::new(20, 3)?;
    terminal.feed(b"\x1b]8;id=demo;https://example.org/path?q=1\x1b\\link\x1b]8;;\x1b\\ plain")?;
    let uri = terminal.hyperlink_at(p(0, 0))?.ok_or("missing URI")?;
    assert_eq!(uri, "https://example.org/path?q=1");
    let screen = terminal.render()?;
    assert!(screen.cells[0].has_hyperlink);
    assert!(!screen.cells[5].has_hyperlink);
    assert_eq!(terminal.hyperlink_at(p(5, 0))?, None);
    assert_eq!(terminal.hyperlink_at(p(20, 0)).unwrap_err(), Error(-2));
    assert_eq!(terminal.hyperlink_at(p(0, 3)).unwrap_err(), Error(-2));
    // The wrapper is not an opener or scheme policy. Dangerous schemes must
    // remain visible as untrusted metadata for the host/UI to reject.
    terminal.feed(b"\r\n\x1b]8;;javascript:alert(1)\x1b\\x\x1b]8;;\x1b\\")?;
    assert_eq!(
        terminal.hyperlink_at(p(0, 1))?.as_deref(),
        Some("javascript:alert(1)")
    );
    terminal.feed(b"\x1b[?1049h")?;
    assert_eq!(terminal.hyperlink_at(p(0, 0))?, None);
    terminal.feed(b"\x1b[?1049l")?;
    terminal.resize(30, 4)?;
    assert_eq!(
        terminal.hyperlink_at(p(0, 0))?.as_deref(),
        Some(uri.as_str())
    );
    drop(terminal);
    assert_eq!(uri, "https://example.org/path?q=1");
    Ok(())
}

#[test]
fn selection_copy_is_bounded_without_destroying_terminal() -> Result<(), Box<dyn std::error::Error>>
{
    let mut terminal = Terminal::new(1000, 1000)?;
    let row = "界".repeat(500);
    for _ in 0..1000 {
        terminal.feed(row.as_bytes())?;
    }
    terminal.set_selection(Some(Selection::All))?;
    assert_eq!(terminal.copy_selection().unwrap_err(), Error(-6));
    terminal.set_selection(Some(range(p(0, 0), p(1, 0), false)))?;
    assert_eq!(terminal.copy_selection()?.as_deref(), Some("界"));
    Ok(())
}
