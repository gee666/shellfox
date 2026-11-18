use shellfox_ghostty_vt::{
    Error, RecoveryInfo, Rgb, Terminal, CONTINUATION_MAX_BYTES, FORMAT_EXTRAS_ALL, MAX_FORMAT_BYTES,
};

#[test]
fn continuation_is_owned_exact_unfinished_utf8_and_csi() -> Result<(), Box<dyn std::error::Error>> {
    let mut source = Terminal::new(12, 3)?;
    assert!(source.continuation()?.is_empty());
    source.feed(&[0xf0, 0x9f, 0x99])?;
    let suffix = source.continuation()?;
    assert_eq!(suffix, [0xf0, 0x9f, 0x99]);
    assert_eq!(source.continuation()?, suffix); // Read does not consume parser state.
    let mut replay = Terminal::new(12, 3)?;
    replay.feed(&suffix)?;
    source.feed(&[0x82])?;
    replay.feed(&[0x82])?;
    assert_eq!(source.render()?.cells, replay.render()?.cells);
    assert_eq!(source.render()?.cells[0].text, "🙂");
    assert!(source.continuation()?.is_empty());
    source.feed(b"\x1b[38;2;1;2;")?;
    let csi = source.continuation()?;
    assert_eq!(csi, b"\x1b[38;2;1;2;");
    replay.feed(&csi)?;
    source.feed(b"3mX")?;
    replay.feed(b"3mX")?;
    let screen = source.render()?;
    assert_eq!(screen.cells, replay.render()?.cells);
    assert_eq!(screen.cells[2].foreground, Rgb { r: 1, g: 2, b: 3 });
    assert!(source.continuation()?.is_empty());
    drop(source);
    assert_eq!(suffix, [0xf0, 0x9f, 0x99]);
    Ok(())
}

#[test]
fn continuation_overflow_is_unavailable_until_ground_not_empty_success(
) -> Result<(), Box<dyn std::error::Error>> {
    let mut source = Terminal::new(12, 3)?;
    source.feed(b"\x1b]2;")?;
    source.feed(&vec![b'x'; CONTINUATION_MAX_BYTES + 1])?;
    assert_eq!(source.continuation().unwrap_err(), Error(-2));
    // This limit is not reply loss and does not poison ordinary operations.
    source.recovery_info()?;
    source.feed(b"\x07")?;
    assert!(source.continuation()?.is_empty());
    source.feed(b"\x1b[")?;
    assert_eq!(source.continuation()?, b"\x1b[");
    Ok(())
}

#[test]
fn recovery_info_uses_active_screen_public_state() -> Result<(), Box<dyn std::error::Error>> {
    let mut source = Terminal::new(5, 2)?;
    assert_eq!(
        source.recovery_info()?,
        RecoveryInfo {
            cols: 5,
            rows: 2,
            alternate: false,
            pending_wrap: false,
            scrollback_rows: 0
        }
    );
    source.feed(b"abcde")?;
    assert!(source.recovery_info()?.pending_wrap);
    source.feed(b"fghijklmno")?;
    assert_eq!(source.recovery_info()?.scrollback_rows, 1);
    source.feed(b"\x1b[?1049h")?;
    let alternate = source.recovery_info()?;
    assert!(alternate.alternate);
    assert_eq!(alternate.scrollback_rows, 0);
    source.resize(8, 3)?;
    let resized = source.recovery_info()?;
    assert_eq!((resized.cols, resized.rows), (8, 3));
    source.feed(b"\x1b[?1049l")?;
    assert!(!source.recovery_info()?.alternate);
    Ok(())
}

#[test]
fn vt_format_replays_active_cells_styles_and_cursor() -> Result<(), Box<dyn std::error::Error>> {
    let mut source = Terminal::new(12, 3)?;
    source.feed("A界e\u{301}\x1b[38;2;1;2;3mX\x1b[0m\x1b[2;3H".as_bytes())?;
    let before = source.render()?;
    let vt = source.format_vt(FORMAT_EXTRAS_ALL, MAX_FORMAT_BYTES)?;
    let mut replay = Terminal::new(12, 3)?;
    replay.feed(&vt)?;
    let after = replay.render()?;
    assert_eq!(before.cells, after.cells);
    assert_eq!(before.cursor, after.cursor);
    assert!(replay.drain_replies()?.is_empty());
    drop(source);
    assert!(!vt.is_empty());
    Ok(())
}

#[test]
fn recovery_inspection_does_not_mutate_source_or_drain_replies(
) -> Result<(), Box<dyn std::error::Error>> {
    let mut source = Terminal::new(12, 3)?;
    source.feed(b"abc\x1b[6n\x1b[31")?;
    source.set_selection(Some(shellfox_ghostty_vt::Selection::Range {
        start: shellfox_ghostty_vt::ViewportPoint { x: 0, y: 0 },
        end: shellfox_ghostty_vt::ViewportPoint { x: 1, y: 0 },
        rectangle: false,
    }))?;
    let selected = source.copy_selection()?;
    let before = source.render()?;
    let info = source.recovery_info()?;
    let continuation = source.continuation()?;
    let first = source.format_vt(FORMAT_EXTRAS_ALL, MAX_FORMAT_BYTES)?;
    assert_eq!(
        source.format_vt(FORMAT_EXTRAS_ALL, MAX_FORMAT_BYTES)?,
        first
    );
    assert_eq!(source.continuation()?, continuation);
    assert_eq!(source.recovery_info()?, info);
    assert_eq!(source.copy_selection()?, selected);
    let after = source.render()?;
    assert_eq!(before.cells, after.cells);
    assert_eq!(before.cursor, after.cursor);
    assert_eq!(source.drain_replies()?, b"\x1b[1;4R");
    assert!(source.drain_replies()?.is_empty());
    source.feed(b"mX")?;
    assert!(source.continuation()?.is_empty());
    assert_eq!(source.render()?.cells[3].text, "X");
    Ok(())
}

#[test]
fn formatter_bounds_and_extras_fail_without_truncation_or_poison(
) -> Result<(), Box<dyn std::error::Error>> {
    let mut source = Terminal::new(12, 3)?;
    source.feed(b"abc")?;
    assert_eq!(source.format_vt(0, 0).unwrap_err(), Error(-6));
    assert_eq!(source.format_vt(0, 1).unwrap_err(), Error(-6));
    assert_eq!(
        source.format_vt(0, MAX_FORMAT_BYTES + 1).unwrap_err(),
        Error(-6)
    );
    assert_eq!(
        source.format_vt(0x1000, MAX_FORMAT_BYTES).unwrap_err(),
        Error(-2)
    );
    let vt = source.format_vt(0, MAX_FORMAT_BYTES)?;
    assert_eq!(source.format_vt(0, vt.len())?, vt);
    assert_eq!(source.format_vt(0, vt.len() - 1).unwrap_err(), Error(-6));
    // Trim=false retains empty trailing rows, unwrap=false retains screen rows.
    let mut replay = Terminal::new(12, 3)?;
    replay.feed(&vt)?;
    assert_eq!(source.render()?.cells, replay.render()?.cells);
    assert!(source.drain_replies()?.is_empty());
    Ok(())
}
