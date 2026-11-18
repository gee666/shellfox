use shellfox_ghostty_vt::{
    HistoryPoint, PresentationBuffer, PresentationSelectionKind, Selection, Terminal, ViewportPoint,
};
type TestResult = Result<(), Box<dyn std::error::Error>>;
fn at(x: u16, y: u16) -> ViewportPoint {
    ViewportPoint { x, y }
}
fn history(x: u16, row: u64) -> HistoryPoint {
    HistoryPoint { x, row }
}

#[test]
fn reverse_drag_keeps_true_direction_even_when_selected_masks_match() -> TestResult {
    let mut terminal = Terminal::new(8, 3)?;
    terminal.feed(b"abcdefgh\r\nijklmnop\r\nqrstuvwx")?;
    let mut reverse = terminal.new_presentation_view()?;
    let mut forward = terminal.new_presentation_view()?;
    terminal.view_select(
        &mut reverse,
        Some(Selection::Range {
            start: at(6, 1),
            end: at(1, 0),
            rectangle: false,
        }),
    )?;
    terminal.view_select(
        &mut forward,
        Some(Selection::Range {
            start: at(1, 0),
            end: at(6, 1),
            rectangle: false,
        }),
    )?;
    let value = terminal
        .view_selection(&mut reverse)?
        .ok_or("missing reverse range")?;
    assert_eq!(value.anchor, history(6, 1));
    assert_eq!(value.focus, history(1, 0));
    assert_eq!(value.kind, PresentationSelectionKind::Range);
    assert_eq!(value.buffer, PresentationBuffer::Primary);
    assert!(!value.rectangle);
    let other = terminal
        .view_selection(&mut forward)?
        .ok_or("missing forward range")?;
    assert_eq!(other.anchor, value.focus);
    assert_eq!(other.focus, value.anchor);
    let mask = |frame: shellfox_ghostty_vt::PresentationFrame| {
        frame
            .screen
            .cells
            .into_iter()
            .map(|c| c.selected)
            .collect::<Vec<_>>()
    };
    assert_eq!(
        mask(terminal.view_render(&mut reverse)?),
        mask(terminal.view_render(&mut forward)?)
    );
    assert_ne!(value, other);
    Ok(())
}

#[test]
fn rectangular_diagonals_are_not_sorted_into_a_fabricated_drag() -> TestResult {
    let mut terminal = Terminal::new(8, 3)?;
    terminal.feed(b"abcdefgh\r\nijklmnop\r\nqrstuvwx")?;
    let mut view = terminal.new_presentation_view()?;
    for (start, end) in [
        (at(6, 0), at(1, 2)),
        (at(1, 2), at(6, 0)),
        (at(6, 2), at(1, 0)),
    ] {
        terminal.view_select(
            &mut view,
            Some(Selection::Range {
                start,
                end,
                rectangle: true,
            }),
        )?;
        let value = terminal
            .view_selection(&mut view)?
            .ok_or("missing rectangle")?;
        assert_eq!(value.anchor, history(start.x, u64::from(start.y)));
        assert_eq!(value.focus, history(end.x, u64::from(end.y)));
        assert!(value.rectangle);
        assert_eq!(value.kind, PresentationSelectionKind::Range);
        assert_eq!(
            terminal.view_copy(&mut view)?,
            Some("bcdefg\njklmno\nrstuvw".into())
        );
    }
    Ok(())
}

#[test]
fn offscreen_endpoints_remain_absolute_while_view_offset_changes() -> TestResult {
    let mut terminal = Terminal::new(8, 3)?;
    terminal.feed(b"zero\r\none\r\ntwo\r\nthree\r\nfour")?;
    let mut view = terminal.new_presentation_view()?;
    terminal.view_scroll(&mut view, isize::MIN)?;
    terminal.view_select(
        &mut view,
        Some(Selection::Range {
            start: at(1, 0),
            end: at(2, 1),
            rectangle: false,
        }),
    )?;
    let initial = terminal
        .view_selection(&mut view)?
        .ok_or("missing historical range")?;
    assert_eq!(initial.anchor, history(1, 0));
    assert_eq!(initial.focus, history(2, 1));
    assert_eq!(initial.view_top_row, 0);
    assert_eq!(initial.total_rows, 5);
    terminal.view_follow_tail(&mut view)?;
    let offscreen = terminal
        .view_selection(&mut view)?
        .ok_or("offscreen is not absent")?;
    assert_eq!(offscreen.anchor, initial.anchor);
    assert_eq!(offscreen.focus, initial.focus);
    assert_eq!(offscreen.view_top_row, 2);
    assert!(offscreen.focus.row < offscreen.view_top_row);
    assert!(terminal
        .view_render(&mut view)?
        .screen
        .cells
        .iter()
        .all(|cell| !cell.selected));
    terminal.view_scroll(&mut view, -1)?;
    assert_eq!(
        terminal
            .view_selection(&mut view)?
            .ok_or("missing range")?
            .view_top_row,
        1
    );
    Ok(())
}

#[test]
fn reverse_endpoints_follow_reflow_and_unicode_cells_not_utf16_offsets() -> TestResult {
    let mut terminal = Terminal::new(12, 3)?;
    terminal.feed(b"abcdefghijkl\r\nnext\r\nlast")?;
    let mut view = terminal.new_presentation_view()?;
    terminal.view_select(
        &mut view,
        Some(Selection::Range {
            start: at(10, 0),
            end: at(2, 0),
            rectangle: false,
        }),
    )?;
    terminal.resize(6, 4)?;
    let value = terminal
        .view_selection(&mut view)?
        .ok_or("reflow lost range")?;
    assert_eq!(value.anchor, history(4, 1));
    assert_eq!(value.focus, history(2, 0));
    assert_eq!(terminal.view_copy(&mut view)?, Some("cdefghijk".into()));
    let mut terminal = Terminal::new(12, 3)?;
    terminal.feed("a界e\u{301}Z".as_bytes())?;
    let mut view = terminal.new_presentation_view()?;
    terminal.view_select(
        &mut view,
        Some(Selection::Range {
            start: at(3, 0),
            end: at(1, 0),
            rectangle: false,
        }),
    )?;
    terminal.resize(3, 4)?;
    let value = terminal
        .view_selection(&mut view)?
        .ok_or("reflow lost grapheme range")?;
    assert_eq!(value.anchor, history(0, 1));
    assert_eq!(value.focus, history(1, 0));
    assert_eq!(terminal.view_copy(&mut view)?, Some("界e\u{301}".into()));
    Ok(())
}

#[test]
fn word_line_all_have_explicit_normalized_not_drag_semantics() -> TestResult {
    let mut terminal = Terminal::new(8, 4)?;
    terminal.feed(b"one two\r\nwrappedline012345\r\nlast")?;
    let mut view = terminal.new_presentation_view()?;
    terminal.view_scroll(&mut view, isize::MIN)?;
    for (selection, kind, copy) in [
        (
            Selection::Word(at(5, 0)),
            PresentationSelectionKind::Word,
            "two",
        ),
        (
            Selection::Line(at(1, 1)),
            PresentationSelectionKind::Line,
            "wrappedline012345",
        ),
        (
            Selection::All,
            PresentationSelectionKind::All,
            "one two\nwrappedline012345\nlast",
        ),
    ] {
        terminal.view_select(&mut view, Some(selection))?;
        let value = terminal
            .view_selection(&mut view)?
            .ok_or("missing normalized range")?;
        assert_eq!(value.kind, kind);
        assert!(!value.rectangle);
        assert!((value.anchor.row, value.anchor.x) <= (value.focus.row, value.focus.x));
        assert_eq!(terminal.view_copy(&mut view)?.as_deref(), Some(copy));
    }
    Ok(())
}

#[test]
fn each_buffer_reports_its_own_context_and_original_direction() -> TestResult {
    let mut terminal = Terminal::new(8, 3)?;
    terminal.feed(b"primary")?;
    let mut view = terminal.new_presentation_view()?;
    terminal.view_select(
        &mut view,
        Some(Selection::Range {
            start: at(6, 0),
            end: at(0, 0),
            rectangle: false,
        }),
    )?;
    let primary = terminal
        .view_selection(&mut view)?
        .ok_or("missing primary")?;
    terminal.feed(b"\x1b[?1049h\x1b[Halt")?;
    assert!(terminal.view_selection(&mut view)?.is_none());
    terminal.view_select(
        &mut view,
        Some(Selection::Range {
            start: at(2, 0),
            end: at(0, 0),
            rectangle: true,
        }),
    )?;
    let alternate = terminal
        .view_selection(&mut view)?
        .ok_or("missing alternate")?;
    assert_eq!(alternate.buffer, PresentationBuffer::Alternate);
    assert!(alternate.rectangle);
    assert_eq!(alternate.anchor, history(2, 0));
    assert_eq!(alternate.focus, history(0, 0));
    terminal.feed(b"\x1b[?1049l")?;
    assert_eq!(terminal.view_selection(&mut view)?, Some(primary));
    Ok(())
}

#[test]
fn discarded_endpoints_are_absent_not_clamped_to_a_visible_cell() -> TestResult {
    let mut terminal = Terminal::new(8, 3)?;
    terminal.feed(b"original\r\nsecond\r\nthird\r\nfourth")?;
    let mut view = terminal.new_presentation_view()?;
    terminal.view_scroll(&mut view, isize::MIN)?;
    terminal.view_select(
        &mut view,
        Some(Selection::Range {
            start: at(7, 0),
            end: at(1, 1),
            rectangle: false,
        }),
    )?;
    assert!(terminal.view_selection(&mut view)?.is_some());
    terminal.feed(&b"x\r\n".repeat(300_000))?;
    assert!(terminal.view_selection(&mut view)?.is_none());
    assert!(terminal.view_selection(&mut view)?.is_none());
    assert!(terminal.view_copy(&mut view)?.is_none());
    Ok(())
}

#[test]
fn query_is_read_only_and_rejects_cross_terminal_handles_and_poison() -> TestResult {
    let mut terminal = Terminal::new(8, 3)?;
    terminal.feed(b"first\r\nsecond\r\nthird\r\nfourth\x1b[6n\x1b[38;")?;
    terminal.scroll(-1)?;
    terminal.set_selection(Some(Selection::Word(at(1, 0))))?;
    let before = terminal.render()?;
    let host = terminal.copy_selection()?;
    let continuation = terminal.continuation()?;
    let formatted = terminal.format_vt(0xFFF, 1024 * 1024)?;
    let mut view = terminal.new_presentation_view()?;
    terminal.view_select(
        &mut view,
        Some(Selection::Range {
            start: at(5, 1),
            end: at(1, 0),
            rectangle: true,
        }),
    )?;
    let value = terminal.view_selection(&mut view)?;
    for _ in 0..3 {
        assert_eq!(terminal.view_selection(&mut view)?, value);
    }
    assert_eq!(terminal.render()?.cells, before.cells);
    assert_eq!(terminal.copy_selection()?, host);
    assert_eq!(terminal.continuation()?, continuation);
    assert_eq!(terminal.format_vt(0xFFF, 1024 * 1024)?, formatted);
    assert_eq!(terminal.drain_replies()?, b"\x1b[3;7R");
    let mut other = Terminal::new(8, 3)?;
    assert!(other.view_selection(&mut view).is_err());
    terminal.feed(b"2;3m")?;
    terminal.feed(&b"\x1b[6n".repeat(120_000))?;
    assert!(terminal.feed(&b"\x1b[6n".repeat(120_000)).is_err());
    assert!(terminal.view_selection(&mut view).is_err());
    Ok(())
}
