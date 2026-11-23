use shellfox_ghostty_vt::{
    key::{KeyEvent, KeyModifiers, PhysicalKey},
    CursorShape, Error, PresentationFrame, Selection, Terminal, ViewportPoint,
    MAX_PRESENTATION_VIEWS,
};
type TestResult = Result<(), Box<dyn std::error::Error>>;
fn at(x: u16, y: u16) -> ViewportPoint {
    ViewportPoint { x, y }
}
fn rows(frame: &PresentationFrame) -> Vec<String> {
    frame
        .screen
        .cells
        .chunks(usize::from(frame.screen.cols))
        .map(|row| {
            row.iter()
                .map(|c| c.text.as_str())
                .collect::<String>()
                .trim_end()
                .to_owned()
        })
        .collect()
}

#[test]
fn two_observers_scroll_select_and_copy_without_touching_host_presentation_or_vt() -> TestResult {
    let mut terminal = Terminal::new(12, 3)?;
    let stream = b"first\r\nsecond\r\nthird\r\nfourth\r\nlast\x1b[?1h\x1b[?2004h\x1b[6n\x1b[38;";
    terminal.feed(stream)?;
    let mut reference = Terminal::new(12, 3)?;
    reference.feed(stream)?;
    let expected_replies = reference.drain_replies()?;
    terminal.scroll(-1)?;
    terminal.set_selection(Some(Selection::Word(at(1, 0))))?;
    let before = terminal.render()?;
    let host_copy = terminal.copy_selection()?;
    let continuation = terminal.continuation()?;
    let formatted = terminal.format_vt(0xFFF, 1024 * 1024)?;
    let input = terminal.encode_key_event(&KeyEvent::press(PhysicalKey::ArrowUp, KeyModifiers::NONE))?;
    let mut one = terminal.new_presentation_view()?;
    let mut two = terminal.new_presentation_view()?;
    terminal.view_scroll(&mut one, isize::MIN)?;
    terminal.view_select(&mut one, Some(Selection::Word(at(2, 0))))?;
    terminal.view_select(&mut two, Some(Selection::Line(at(2, 1))))?;
    assert_eq!(terminal.view_copy(&mut one)?, Some("first".into()));
    assert_eq!(terminal.view_copy(&mut two)?, Some("fourth".into()));
    assert_eq!(rows(&terminal.view_render(&mut one)?)[0], "first");
    assert_eq!(rows(&terminal.view_render(&mut two)?)[0], "third");
    assert!(!terminal.view_render(&mut one)?.screen.cursor.visible);
    assert!(terminal.view_render(&mut two)?.screen.cursor.visible);
    terminal.view_scroll(&mut two, -1)?;
    assert_eq!(rows(&terminal.view_render(&mut one)?)[0], "first");
    assert_eq!(rows(&terminal.view_render(&mut two)?)[0], "second");
    terminal.view_follow_tail(&mut one)?;
    assert_eq!(rows(&terminal.view_render(&mut one)?)[0], "third");
    let after = terminal.render()?;
    assert_eq!(after.cells, before.cells);
    assert_eq!(after.cursor, before.cursor);
    assert_eq!(terminal.copy_selection()?, host_copy);
    assert_eq!(terminal.continuation()?, continuation);
    assert_eq!(terminal.format_vt(0xFFF, 1024 * 1024)?, formatted);
    assert_eq!(
        terminal.encode_key_event(&KeyEvent::press(PhysicalKey::ArrowUp, KeyModifiers::NONE))?,
        input
    );
    assert_eq!(terminal.drain_replies()?, expected_replies);
    Ok(())
}

#[test]
fn tracked_viewport_and_selection_follow_reflow_and_new_output() -> TestResult {
    let mut terminal = Terminal::new(12, 3)?;
    terminal.feed(b"one two\r\nthree\r\nfour\r\nfive")?;
    let mut view = terminal.new_presentation_view()?;
    terminal.view_scroll(&mut view, isize::MIN)?;
    terminal.view_select(&mut view, Some(Selection::Word(at(4, 0))))?;
    assert_eq!(terminal.view_copy(&mut view)?, Some("two".into()));
    terminal.resize(6, 4)?;
    assert_eq!(terminal.view_copy(&mut view)?, Some("two".into()));
    assert_eq!(rows(&terminal.view_render(&mut view)?)[0], "one tw");
    terminal.feed(b"\r\nsix\r\nseven\r\neight")?;
    assert_eq!(rows(&terminal.view_render(&mut view)?)[0], "one tw");
    assert_eq!(terminal.view_copy(&mut view)?, Some("two".into()));
    terminal.view_follow_tail(&mut view)?;
    assert!(rows(&terminal.view_render(&mut view)?)
        .iter()
        .any(|s| s == "eight"));
    Ok(())
}

#[test]
fn native_unicode_word_logical_line_and_wide_grapheme_selection() -> TestResult {
    let mut terminal = Terminal::new(8, 4)?;
    terminal.feed("cafe\u{301} 界!\r\nabcdefghijk\r\nlast".as_bytes())?;
    let mut view = terminal.new_presentation_view()?;
    terminal.view_scroll(&mut view, isize::MIN)?;
    terminal.view_select(&mut view, Some(Selection::Word(at(2, 0))))?;
    assert_eq!(terminal.view_copy(&mut view)?, Some("cafe\u{301}".into()));
    terminal.view_select(&mut view, Some(Selection::Line(at(2, 1))))?;
    assert_eq!(terminal.view_copy(&mut view)?, Some("abcdefghijk".into()));
    terminal.view_select(
        &mut view,
        Some(Selection::Range {
            start: at(5, 0),
            end: at(6, 0),
            rectangle: false,
        }),
    )?;
    assert_eq!(terminal.view_copy(&mut view)?, Some("界".into()));
    let frame = terminal.view_render(&mut view)?;
    assert_eq!(frame.screen.cells[5].width, 2);
    assert_eq!(frame.screen.cells[6].width, 0);
    assert!(frame.screen.cells[5].selected && frame.screen.cells[6].selected);
    assert_eq!(frame.screen.cells[3].text, "e\u{301}");
    Ok(())
}

#[test]
fn rectangular_and_reversed_ranges_use_ghostty_formatter() -> TestResult {
    let mut terminal = Terminal::new(8, 3)?;
    terminal.feed(b"abcdef\r\nghijkl\r\nmnopqr")?;
    let mut view = terminal.new_presentation_view()?;
    terminal.view_select(
        &mut view,
        Some(Selection::Range {
            start: at(3, 1),
            end: at(1, 0),
            rectangle: true,
        }),
    )?;
    assert_eq!(terminal.view_copy(&mut view)?, Some("bcd\nhij".into()));
    terminal.view_select(&mut view, None)?;
    assert_eq!(terminal.view_copy(&mut view)?, None);
    Ok(())
}

#[test]
fn each_buffer_retains_its_own_view_anchors_and_selection() -> TestResult {
    let mut terminal = Terminal::new(12, 3)?;
    terminal.feed(b"first\r\nsecond\r\nthird\r\nfourth")?;
    let mut view = terminal.new_presentation_view()?;
    terminal.view_scroll(&mut view, -1)?;
    terminal.view_select(&mut view, Some(Selection::Word(at(1, 0))))?;
    terminal.feed(b"\x1b[?1049h\x1b[Halt")?;
    assert_eq!(terminal.view_copy(&mut view)?, None);
    terminal.view_select(&mut view, Some(Selection::Word(at(1, 0))))?;
    assert_eq!(terminal.view_copy(&mut view)?, Some("alt".into()));
    terminal.feed(b"\x1b[?1049l")?;
    assert_eq!(terminal.view_copy(&mut view)?, Some("first".into()));
    assert_eq!(rows(&terminal.view_render(&mut view)?)[0], "first");
    Ok(())
}

#[test]
fn view_lifetimes_wrong_source_and_capacity_are_checked() -> TestResult {
    let mut terminal = Terminal::new(8, 3)?;
    let mut views = Vec::new();
    for _ in 0..MAX_PRESENTATION_VIEWS {
        views.push(terminal.new_presentation_view()?);
    }
    assert!(matches!(terminal.new_presentation_view(), Err(Error(-6))));
    let mut other = Terminal::new(8, 3)?;
    assert!(other.view_scroll(&mut views[0], -1).is_err());
    terminal.view_select(&mut views[0], Some(Selection::All))?;
    let last = views.pop();
    drop(last);
    let replacement = terminal.new_presentation_view()?;
    drop(terminal);
    assert!(other.view_render(&mut views[0]).is_err());
    drop(views);
    drop(replacement);
    Ok(())
}

#[test]
fn cursor_visual_honors_decscusr_blink_color_and_view_local_focus() -> TestResult {
    let mut terminal = Terminal::new(12, 3)?;
    assert_eq!(terminal.cursor_visual()?.shape, CursorShape::Block);
    for (sequence, shape, blinking) in [
        (1, CursorShape::Block, true),
        (2, CursorShape::Block, false),
        (3, CursorShape::Underline, true),
        (4, CursorShape::Underline, false),
        (5, CursorShape::Bar, true),
        (6, CursorShape::Bar, false),
    ] {
        terminal.feed(format!("\x1b[{sequence} q").as_bytes())?;
        let visual = terminal.cursor_visual()?;
        assert_eq!(visual.shape, shape);
        assert_eq!(visual.blinking, blinking);
        let inactive = visual.for_focus(false);
        assert_eq!(inactive.shape, CursorShape::HollowBlock);
        assert!(!inactive.blinking);
        assert_eq!(terminal.cursor_visual()?, visual);
    }
    terminal.feed(b"\x1b]12;#123456\x07\x1b[?25l")?;
    let visual = terminal.cursor_visual()?;
    assert_eq!(
        visual.color,
        shellfox_ghostty_vt::Rgb {
            r: 0x12,
            g: 0x34,
            b: 0x56
        }
    );
    assert!(!visual.visible);
    Ok(())
}

#[test]
fn wide_cursor_and_cursor_reads_do_not_lose_subsequent_render_updates() -> TestResult {
    let mut terminal = Terminal::new(8, 3)?;
    terminal.render()?;
    terminal.feed("界\x1b[1;2H".as_bytes())?;
    let cursor = terminal.cursor_visual()?;
    assert!(cursor.wide_tail);
    assert_eq!(cursor.width, 2);
    assert_eq!(terminal.render()?.cells[0].text, "界");
    terminal.feed(b"\x1b[2;1Hchanged")?;
    terminal.cursor_visual()?;
    assert_eq!(terminal.render()?.cells[8].text, "c");
    Ok(())
}

#[test]
fn row_metadata_distinguishes_soft_wrap_and_hard_breaks_after_reflow() -> TestResult {
    let mut terminal = Terminal::new(6, 4)?;
    terminal.feed(b"abcdefgh\r\nhard")?;
    let mut view = terminal.new_presentation_view()?;
    let frame = terminal.view_render(&mut view)?;
    assert!(frame.row_metadata[0].soft_wrapped);
    assert!(frame.row_metadata[1].wrap_continuation);
    assert!(!frame.row_metadata[2].wrap_continuation);
    terminal.resize(12, 4)?;
    let frame = terminal.view_render(&mut view)?;
    assert!(!frame.row_metadata[0].soft_wrapped);
    assert!(!frame.row_metadata[1].wrap_continuation);
    Ok(())
}

#[test]
fn independent_grid_colors_styles_and_graphemes_match_shared_render_state() -> TestResult {
    let mut terminal = Terminal::new(16, 4)?;
    terminal.feed(
        "\x1b[1;2;3;4;7;8;9;38;2;1;2;3;48;2;4;5;6m界e\u{301}\x1b[0m\r\n\x1b[48;5;42m\x1b[K"
            .as_bytes(),
    )?;
    let expected = terminal.render()?;
    let mut view = terminal.new_presentation_view()?;
    let actual = terminal.view_render(&mut view)?;
    assert_eq!(actual.screen.cells, expected.cells);
    assert_eq!(actual.screen.cursor, expected.cursor);
    Ok(())
}

#[test]
fn historical_hyperlinks_resolve_against_the_independent_view() -> TestResult {
    let mut terminal = Terminal::new(12, 3)?;
    terminal
        .feed(b"\x1b]8;;https://example.test/old\x1b\\old\x1b]8;;\x1b\\\r\ntwo\r\nthree\r\nfour")?;
    let mut old = terminal.new_presentation_view()?;
    let mut live = terminal.new_presentation_view()?;
    terminal.view_scroll(&mut old, isize::MIN)?;
    assert_eq!(
        terminal.view_hyperlink(&mut old, at(0, 0))?,
        Some("https://example.test/old".into())
    );
    assert_eq!(terminal.view_hyperlink(&mut live, at(0, 0))?, None);
    assert!(terminal.view_render(&mut old)?.screen.cells[0].has_hyperlink);
    Ok(())
}

#[test]
fn pruned_selection_is_cleared_and_viewport_clamps_without_dangling_refs() -> TestResult {
    let mut terminal = Terminal::new(8, 3)?;
    terminal.feed(b"original\r\nsecond\r\nthird\r\nfourth")?;
    let mut view = terminal.new_presentation_view()?;
    terminal.view_scroll(&mut view, isize::MIN)?;
    terminal.view_select(&mut view, Some(Selection::Word(at(1, 0))))?;
    assert_eq!(terminal.view_copy(&mut view)?, Some("original".into()));
    terminal.feed(&b"x\r\n".repeat(300_000))?;
    assert_eq!(terminal.view_copy(&mut view)?, None);
    assert_eq!(terminal.view_render(&mut view)?.top_row, 0);
    Ok(())
}

#[test]
fn independent_views_bounded_render_workload() -> TestResult {
    let mut terminal = Terminal::new(80, 24)?;
    terminal.feed(&b"history line with bounded cells\r\n".repeat(500))?;
    let mut views = Vec::new();
    for index in 0..MAX_PRESENTATION_VIEWS {
        let mut view = terminal.new_presentation_view()?;
        terminal.view_scroll(&mut view, -(index as isize) * 5)?;
        views.push(view);
    }
    let start = std::time::Instant::now();
    let mut cells = 0;
    for _ in 0..10 {
        for view in &mut views {
            cells += terminal.view_render(view)?.screen.cells.len();
        }
    }
    assert_eq!(cells, 80 * 24 * 10 * MAX_PRESENTATION_VIEWS);
    println!(
        "presentation workload: 170 frames, {cells} cells, {:?}",
        start.elapsed()
    );
    Ok(())
}

#[test]
fn invalid_coordinates_frame_bounds_and_poison_fail_closed() -> TestResult {
    let mut terminal = Terminal::new(8, 3)?;
    let mut view = terminal.new_presentation_view()?;
    assert!(terminal
        .view_select(&mut view, Some(Selection::Word(at(8, 0))))
        .is_err());
    terminal.feed(&b"\x1b[6n".repeat(120_000))?;
    assert!(terminal.feed(&b"\x1b[6n".repeat(120_000)).is_err());
    assert!(terminal.view_render(&mut view).is_err());
    assert!(terminal.view_copy(&mut view).is_err());
    assert!(terminal.view_scroll(&mut view, -1).is_err());
    assert!(terminal.cursor_visual().is_err());
    let mut large = Terminal::new(768, 216)?;
    let mut view = large.new_presentation_view()?;
    assert!(matches!(
        large.view_render_with_limit(&mut view, 32768),
        Err(Error(-6))
    ));
    assert_eq!(large.view_render(&mut view)?.screen.cells.len(), 768 * 216);
    assert!(large.view_render_with_limit(&mut view, 0).is_err());
    assert!(large
        .view_render_with_limit(&mut view, shellfox_ghostty_vt::HARD_PRESENTATION_CELLS + 1)
        .is_err());
    Ok(())
}
