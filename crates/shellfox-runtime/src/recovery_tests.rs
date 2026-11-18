use super::*;
use shellfox_ghostty_vt::{Key, KeyEvent, Screen};
use std::process::Command;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn feed(terminal: &mut Terminal, bytes: &[u8]) -> TestResult {
    for chunk in bytes.chunks(64 * 1024) {
        terminal.feed(chunk)?;
    }
    Ok(())
}
fn comparable(screen: Screen) -> (Vec<shellfox_ghostty_vt::Cell>, shellfox_ghostty_vt::Cursor) {
    let mut cells = screen.cells;
    for cell in &mut cells {
        if cell.text.is_empty() && cell.width != 0 {
            cell.text.push(' ');
        }
    }
    (cells, screen.cursor)
}
fn text(terminal: &mut Terminal) -> Result<String, shellfox_ghostty_vt::Error> {
    terminal.set_selection(Some(shellfox_ghostty_vt::Selection::All))?;
    let text = terminal.copy_selection()?.unwrap_or_default();
    terminal.set_selection(None)?;
    Ok(text)
}
fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8] = b"0123456789abcdef";
    bytes
        .iter()
        .flat_map(|byte| {
            [
                char::from(DIGITS[usize::from(byte >> 4)]),
                char::from(DIGITS[usize::from(byte & 15)]),
            ]
        })
        .collect()
}
// Original differential fixture using the installed legacy xterm public API.
// No xterm implementation or upstream test code is copied into Rust.
const XTERM: &str = r#"
const assert = require('node:assert/strict');
const {Terminal} = require('@xterm/headless');
const watchdog = setTimeout(()=>{console.error('xterm differential timeout');process.exit(2);},10000);watchdog.unref();
const [cols, rows, original, recovery, suffix] = process.argv.slice(1);
const create = () => new Terminal({cols:+cols, rows:+rows, scrollback:3000, allowProposedApi:true, logLevel:'off'});
const write = (term, data) => new Promise(resolve => term.write(Buffer.from(data,'hex'), resolve));
function state(term) {
  const buffer = term.buffer.active, lines = [];
  for (let y=0; y<buffer.length; y++) {
    const line = buffer.getLine(y), cells=[];
    for (let x=0; x<term.cols; x++) {
      const c=line.getCell(x);
      cells.push([c.getChars() || (c.getWidth() ? ' ' : ''), c.getWidth(),
        !!c.isBold(),!!c.isItalic(),!!c.isUnderline(),!!c.isInverse(),!!c.isInvisible(),!!c.isStrikethrough()]);
    }
    lines.push(cells);
  }
  return {lines, type:buffer.type, cursor:[Math.min(buffer.cursorX,term.cols-1),buffer.cursorY],
    application:term.modes.applicationCursorKeysMode, bracketed:term.modes.bracketedPasteMode};
}
(async()=>{
  const source=create(), target=create(), replies=[];let clipboard=0;
  target.onData(data=>replies.push(data));
  target.parser.registerOscHandler(52,()=>{clipboard++;return true;});
  try {
    await write(source,original);
    // Deliberately dirty the receiving terminal at the same dimensions.
    await write(target,Buffer.from('STALE\r\nOLD\x1b[?2004h\x1b]52;c;YQ==').toString('hex'));
    await write(target,recovery);
    assert.deepEqual(replies,[], 'recovery must not contain completed queries');
    assert.equal(clipboard,0,'recovery must not contain completed clipboard requests');
    await write(source,suffix); await write(target,suffix);
    assert.deepEqual(state(target),state(source));
  } finally {source.dispose();target.dispose();}
})().catch(error=>{console.error(error);process.exitCode=1;});
"#;
fn legacy_xterm(
    cols: u16,
    rows: u16,
    original: &[u8],
    recovery: &[u8],
    suffix: &[u8],
) -> TestResult {
    let project = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let output = Command::new("node")
        .current_dir(project)
        .args([
            "-e",
            XTERM,
            &cols.to_string(),
            &rows.to_string(),
            &hex(original),
            &hex(recovery),
            &hex(suffix),
        ])
        .output()?;
    assert!(
        output.status.success(),
        "legacy xterm differential failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}

#[test]
fn history_styles_cursor_and_live_suffix_roundtrip_without_refeeding_source() -> TestResult {
    let original =
        b"first\r\nsecond\r\nthird\r\nfourth\r\n\x1b[38;2;1;2;3mstyled\x1b[0m\x1b[2;3H\x1b[1;3;4m";
    let suffix = b"NEW\x1b[0m\r\nlast";
    let mut source = Terminal::new(16, 4)?;
    feed(&mut source, original)?;
    let before = comparable(source.render()?);
    let recovery = active_buffer_ansi(&mut source)?;
    assert_eq!(comparable(source.render()?), before);
    assert!(source.drain_replies()?.is_empty());
    let mut target = Terminal::new(16, 4)?;
    target.feed(b"stale\r\nprevious\x1b[?2004h")?;
    feed(&mut target, &recovery)?;
    assert!(target.drain_replies()?.is_empty());
    assert_eq!(comparable(target.render()?), before);
    assert_eq!(text(&mut target)?, text(&mut source)?);
    source.feed(suffix)?;
    target.feed(suffix)?;
    assert_eq!(comparable(target.render()?), comparable(source.render()?));
    assert_eq!(text(&mut target)?, text(&mut source)?);
    legacy_xterm(16, 4, original, &recovery, suffix)
}

#[test]
fn alternate_active_buffer_and_supported_input_modes_roundtrip() -> TestResult {
    let original =
        b"primary\x1b[?1049h\x1b[Halt\x1b[?1h\x1b[?2004h\x1b[?1000h\x1b[?1006h\x1b[?25l\x1b[2;4H";
    let mut source = Terminal::new(16, 4)?;
    source.feed(original)?;
    let recovery = active_buffer_ansi(&mut source)?;
    let mut target = Terminal::new(16, 4)?;
    feed(&mut target, &recovery)?;
    assert_eq!(comparable(target.render()?), comparable(source.render()?));
    let up = KeyEvent {
        key: Key::ArrowUp,
        ..Default::default()
    };
    assert_eq!(target.encode_key(&up)?, source.encode_key(&up)?);
    assert_eq!(
        target.encode_paste("one\ntwo")?,
        source.encode_paste("one\ntwo")?
    );
    assert_eq!(target.mouse_tracking()?, source.mouse_tracking()?);
    assert!(target.drain_replies()?.is_empty());
    legacy_xterm(16, 4, original, &recovery, b"Z")
}

#[test]
fn unfinished_utf8_and_escape_suffix_are_last_and_resume_at_the_barrier() -> TestResult {
    for (original, suffix) in [
        (b"before\xe9\x9b".as_slice(), b"\xaa!".as_slice()),
        (b"\x1b[31mred\x1b[".as_slice(), b"2;2H!".as_slice()),
        (b"ab\x1b[38;2;1;".as_slice(), b"2;3mZ".as_slice()),
    ] {
        let mut source = Terminal::new(16, 4)?;
        feed(&mut source, original)?;
        let pending = source.continuation()?;
        assert!(!pending.is_empty());
        let recovery = active_buffer_ansi(&mut source)?;
        assert!(recovery.ends_with(&pending));
        let mut target = Terminal::new(16, 4)?;
        feed(&mut target, &recovery)?;
        assert!(target.drain_replies()?.is_empty());
        source.feed(suffix)?;
        target.feed(suffix)?;
        assert_eq!(comparable(target.render()?), comparable(source.render()?));
        legacy_xterm(16, 4, original, &recovery, suffix)?;
    }
    Ok(())
}

#[test]
fn same_grid_repaint_and_pending_wrap_survive_later_print() -> TestResult {
    let original = b"\x1b[3;1H1234567890123456";
    let suffix = b"X";
    let mut source = Terminal::new(16, 4)?;
    source.feed(original)?;
    let recovery = active_buffer_ansi(&mut source)?;
    let mut target = Terminal::new(16, 4)?;
    target.feed(b"\x1b[2J\x1b[Hstale\x1b[4;1Hobsolete")?;
    feed(&mut target, &recovery)?;
    source.feed(suffix)?;
    target.feed(suffix)?;
    assert_eq!(comparable(target.render()?), comparable(source.render()?));
    legacy_xterm(16, 4, original, &recovery, suffix)
}

#[test]
fn scrolling_region_and_origin_preserve_later_output() -> TestResult {
    let original = b"\x1b[2;4r\x1b[?6h\x1b[1;1Hregion\x1b[2;4H";
    let suffix = b"\r\nline\r\nlast";
    let mut source = Terminal::new(16, 4)?;
    source.feed(original)?;
    let recovery = active_buffer_ansi(&mut source)?;
    let mut target = Terminal::new(16, 4)?;
    feed(&mut target, &recovery)?;
    source.feed(suffix)?;
    target.feed(suffix)?;
    assert_eq!(comparable(target.render()?), comparable(source.render()?));
    legacy_xterm(16, 4, original, &recovery, suffix)
}

#[test]
fn resized_state_is_recovered_at_the_new_grid_boundary() -> TestResult {
    let mut source = Terminal::new(8, 4)?;
    source.feed(b"one\r\ntwo\r\nthree\r\nfour\r\nfive")?;
    source.resize(16, 3)?;
    source.feed(b"\x1b[3;1Hnew-grid")?;
    let recovery = active_buffer_ansi(&mut source)?;
    let mut target = Terminal::new(16, 3)?;
    feed(&mut target, &recovery)?;
    assert_eq!(comparable(target.render()?), comparable(source.render()?));
    assert_eq!(text(&mut target)?, text(&mut source)?);
    Ok(())
}

#[test]
fn recovery_does_not_consume_or_regenerate_authoritative_query_replies() -> TestResult {
    let mut source = Terminal::new(16, 4)?;
    source.feed(b"abc\x1b[6n")?;
    let recovery = active_buffer_ansi(&mut source)?;
    assert_eq!(source.drain_replies()?, b"\x1b[1;4R");
    let mut target = Terminal::new(16, 4)?;
    feed(&mut target, &recovery)?;
    assert!(target.drain_replies()?.is_empty());
    assert!(source.drain_replies()?.is_empty());
    Ok(())
}

#[test]
fn completed_clipboard_and_directory_effects_are_not_replayed() -> TestResult {
    let original = b"text\x1b]52;c;Y29weQ==\x07\x1b]7;file:///untrusted\x07";
    let mut source = Terminal::new(16, 4)?;
    source.feed(original)?;
    let recovery = active_buffer_ansi(&mut source)?;
    assert!(!recovery.windows(4).any(|bytes| bytes == b"\x1b]52"));
    assert!(!recovery.windows(4).any(|bytes| bytes == b"\x1b]7;"));
    legacy_xterm(16, 4, original, &recovery, b"!")
}

#[test]
fn insufficient_budget_refuses_history_instead_of_returning_a_partial_prefix() -> TestResult {
    let mut source = Terminal::new(16, 4)?;
    let output = b"\x1b[38;2;1;2;3mhistory\x1b[0m\r\n".repeat(100);
    feed(&mut source, &output)?;
    let before = text(&mut source)?;
    for budget in [0, RESET.len() - 1, 128] {
        let error = bounded_active_buffer_ansi(&mut source, budget).unwrap_err();
        assert!(!error.message.is_empty());
    }
    assert_eq!(text(&mut source)?, before);
    assert!(source.drain_replies()?.is_empty());
    assert!(active_buffer_ansi(&mut source)?.len() <= MAX_RECOVERY_BYTES);
    source.feed(b"still-live")?;
    Ok(())
}

#[test]
fn unavailable_continuation_refuses_until_parser_ground_is_reached() -> TestResult {
    let mut source = Terminal::new(16, 4)?;
    source.feed(b"\x1b]0;")?;
    feed(
        &mut source,
        &vec![b'x'; shellfox_ghostty_vt::CONTINUATION_MAX_BYTES + 1],
    )?;
    assert!(source.continuation().is_err());
    assert!(active_buffer_ansi(&mut source).is_err());
    source.feed(b"\x07")?;
    assert!(source.continuation()?.is_empty());
    assert!(active_buffer_ansi(&mut source).is_ok());
    Ok(())
}

#[test]
fn keyboard_stack_is_not_a_full_state_contract() -> TestResult {
    let mut source = Terminal::new(16, 4)?;
    source.feed(b"\x1b[>1u\x1b[>3u")?;
    let recovery = active_buffer_ansi(&mut source)?;
    let mut target = Terminal::new(16, 4)?;
    feed(&mut target, &recovery)?;
    source.feed(b"\x1b[<u")?;
    target.feed(b"\x1b[<u")?;
    let escape = KeyEvent {
        key: Key::Escape,
        ..Default::default()
    };
    assert_ne!(source.encode_key(&escape)?, target.encode_key(&escape)?);
    Ok(())
}

#[test]
fn formatter_cannot_preserve_saved_cursor_and_inactive_primary_buffer() -> TestResult {
    // This is an executable reason not to advertise the bytes as FullState.
    for (original, suffix) in [
        (b"\x1b[2;2H\x1b7\x1b[3;5H".as_slice(), b"\x1b8X".as_slice()),
        (
            b"primary\x1b[?1049h\x1b[Halt".as_slice(),
            b"\x1b[?1049lX".as_slice(),
        ),
    ] {
        let mut source = Terminal::new(16, 4)?;
        source.feed(original)?;
        let recovery = active_buffer_ansi(&mut source)?;
        let mut target = Terminal::new(16, 4)?;
        feed(&mut target, &recovery)?;
        source.feed(suffix)?;
        target.feed(suffix)?;
        assert_ne!(comparable(target.render()?), comparable(source.render()?));
    }
    Ok(())
}
