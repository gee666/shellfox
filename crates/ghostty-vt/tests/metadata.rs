use shellfox_ghostty_vt::{
    Error, Selection, Terminal, ViewportPoint, FORMAT_EXTRAS_ALL, MAX_FORMAT_BYTES,
    MAX_METADATA_CWD_BYTES, MAX_PENDING_BELLS, MAX_RAW_TITLE_BYTES,
};

fn osc(terminal: &mut Terminal, code: &str, value: &[u8]) -> Result<(), Error> {
    terminal.feed(format!("\x1b]{code};").as_bytes())?;
    terminal.feed(value)?;
    terminal.feed(b"\x1b\\")
}

#[test]
fn fragmented_metadata_is_coalesced_owned_and_taken_once() -> Result<(), Box<dyn std::error::Error>>
{
    let mut terminal = Terminal::new(12, 3)?;
    assert_eq!(terminal.take_metadata()?, None);
    for byte in "\x1b]2;編🙂".as_bytes() {
        terminal.feed(&[*byte])?;
    }
    assert_eq!(terminal.take_metadata()?, None);
    terminal.feed(b"\x1b\\\x1b]7;file://localhost/work/next%20folder\x07\x07")?;
    let first = terminal.take_metadata()?.ok_or("missing metadata")?;
    assert_eq!(first.title.as_deref(), Some("編🙂"));
    assert_eq!(
        first.cwd_uri.as_deref(),
        Some("file://localhost/work/next%20folder")
    );
    assert_eq!(first.bell_count, 1); // The OSC's BEL terminator is not a bell.
    assert!(!first.bells_saturated);
    assert_eq!(first.rejected_updates, 0);
    assert_eq!(terminal.take_metadata()?, None);
    terminal.feed(
        b"\x1b]0;middle\x07\x1b]2;latest\x07\x1b]7;file://remote-host/srv/project\x07\x07\x07",
    )?;
    let latest = terminal.take_metadata()?.ok_or("missing latest")?;
    assert!(latest.version > first.version);
    assert_eq!(latest.title.as_deref(), Some("latest"));
    assert_eq!(
        latest.cwd_uri.as_deref(),
        Some("file://remote-host/srv/project")
    );
    assert_eq!(latest.bell_count, 2);
    drop(terminal);
    assert_eq!(first.title.as_deref(), Some("編🙂"));
    assert_eq!(latest.title.as_deref(), Some("latest"));
    Ok(())
}

#[test]
fn title_uses_legacy_trim_control_and_utf16_limits() -> Result<(), Box<dyn std::error::Error>> {
    let mut terminal = Terminal::new(12, 3)?;
    osc(&mut terminal, "2", " \u{a0} A\u{85}B \u{2000}".as_bytes())?;
    assert_eq!(
        terminal.take_metadata()?.ok_or("title")?.title.as_deref(),
        Some("AB")
    );
    osc(&mut terminal, "2", "🙂".repeat(150).as_bytes())?;
    let title = terminal
        .take_metadata()?
        .ok_or("title")?
        .title
        .ok_or("title value")?;
    assert_eq!(title, "🙂".repeat(100));
    assert_eq!(title.encode_utf16().count(), 200);
    osc(
        &mut terminal,
        "2",
        format!("{}🙂", "a".repeat(199)).as_bytes(),
    )?;
    assert_eq!(
        terminal.take_metadata()?.ok_or("title")?.title.as_deref(),
        Some("a".repeat(199).as_str())
    );
    osc(&mut terminal, "2", "界".repeat(250).as_bytes())?;
    assert_eq!(
        terminal.take_metadata()?.ok_or("title")?.title.as_deref(),
        Some("界".repeat(200).as_str())
    );
    osc(&mut terminal, "2", b" \t\r\n ")?;
    assert_eq!(terminal.take_metadata()?.ok_or("clear")?.title, None);
    Ok(())
}

#[test]
fn duplicate_normalized_titles_do_not_manufacture_versions(
) -> Result<(), Box<dyn std::error::Error>> {
    let mut terminal = Terminal::new(12, 3)?;
    osc(&mut terminal, "2", b"stable")?;
    let first = terminal.take_metadata()?.ok_or("title")?;
    osc(&mut terminal, "2", b" stable ")?;
    assert_eq!(terminal.take_metadata()?, None);
    terminal.feed(b"\x07")?;
    let next = terminal.take_metadata()?.ok_or("bell")?;
    assert_eq!(next.version, first.version + 1);
    assert_eq!(next.title, first.title);
    Ok(())
}

#[test]
fn valid_then_invalid_cwd_in_one_feed_retains_last_valid() -> Result<(), Box<dyn std::error::Error>>
{
    let mut terminal = Terminal::new(12, 3)?;
    terminal
        .feed(b"\x1b]7;file://foreign-host/a%20b\x07\x1b]7;https://example.org/not-a-cwd\x07")?;
    let metadata = terminal.take_metadata()?.ok_or("metadata")?;
    assert_eq!(
        metadata.cwd_uri.as_deref(),
        Some("file://foreign-host/a%20b")
    );
    assert_eq!(metadata.rejected_updates, 1);
    Ok(())
}

#[test]
fn cwd_rejects_controls_credentials_query_fragment_ports_and_invalid_encoding(
) -> Result<(), Box<dyn std::error::Error>> {
    let mut terminal = Terminal::new(12, 3)?;
    osc(&mut terminal, "7", b"file:///safe")?;
    terminal.take_metadata()?;
    for bad in [
        "https://example.org/work",
        "file://user@host/work",
        "file://user%40host/work",
        "file://host:22/work",
        "file://host%3A22/work",
        "file://[:::]/work",
        "file://[xyz]/work",
        "file://[1:2:3:4:5:6:7:8:9]/work",
        "file:///work?secret=1",
        "file:///work#fragment",
        "file:///work%00bad",
        "file:///work%0abad",
        "file:///work%7Fbad",
        "file:///work%C2%85bad",
        "file:///work%ffbad",
        "file:///work%C0%AFbad",
        "file:///work%ED%A0%80bad",
        "file:///work%F4%90%80%80bad",
        "file:///work%",
        "file:///work%2",
        "file:///work%GG",
        "file:///work space",
        "file:///work\\bad",
        "/bare/path",
        "file://host",
    ] {
        osc(&mut terminal, "7", bad.as_bytes())?;
        let metadata = terminal.take_metadata()?.ok_or("missing rejection")?;
        assert_eq!(metadata.cwd_uri.as_deref(), Some("file:///safe"), "{bad}");
        assert_eq!(metadata.rejected_updates, 1, "{bad}");
    }
    for good in [
        "file://remote-host/a%3Fb%23c",
        "FILE://localhost/C:/work",
        "file://[::1]/work",
        "file://[::ffff:192.0.2.1]/work",
        "file://[2001:db8:0:0:0:0:0:1]/work",
        "file:///work/%F0%9F%99%82",
        "file:///界",
        "file://host_name/work",
        "file://rémote/work",
    ] {
        osc(&mut terminal, "7", good.as_bytes())?;
        let metadata = terminal.take_metadata()?.ok_or("missing valid cwd")?;
        assert_eq!(metadata.cwd_uri.as_deref(), Some(good));
        assert_eq!(metadata.rejected_updates, 0);
    }
    Ok(())
}

#[test]
fn invalid_and_oversized_metadata_never_poison_or_drain_replies(
) -> Result<(), Box<dyn std::error::Error>> {
    let mut terminal = Terminal::new(12, 3)?;
    osc(&mut terminal, "2", b"good")?;
    osc(&mut terminal, "7", b"file:///good")?;
    terminal.take_metadata()?;
    osc(&mut terminal, "2", &vec![b'x'; MAX_RAW_TITLE_BYTES + 1])?;
    let large_cwd = format!("file:///{}", "x".repeat(MAX_METADATA_CWD_BYTES));
    osc(&mut terminal, "7", large_cwd.as_bytes())?;
    // vt-1.0.1 ignores oversized OSC strings before invoking public callbacks.
    // Such native rejection is not observable as a wrapper rejection counter.
    assert_eq!(terminal.take_metadata()?, None);
    terminal.feed(b"\x07abc\x1b[6n")?;
    let metadata = terminal.take_metadata()?.ok_or("missing bell snapshot")?;
    assert_eq!(metadata.title.as_deref(), Some("good"));
    assert_eq!(metadata.cwd_uri.as_deref(), Some("file:///good"));
    assert_eq!(metadata.rejected_updates, 0);
    assert_eq!(metadata.bell_count, 1);
    assert_eq!(terminal.drain_replies()?, b"\x1b[1;4R");
    terminal.feed(b"d")?;
    assert_eq!(terminal.render()?.cells[3].text, "d");
    Ok(())
}

#[test]
fn bell_storm_is_bounded_and_does_not_lose_device_replies() -> Result<(), Box<dyn std::error::Error>>
{
    let mut terminal = Terminal::new(12, 3)?;
    terminal.feed(&vec![7; MAX_PENDING_BELLS as usize + 10])?;
    terminal.feed(b"\x1b[5n")?;
    let bells = terminal.take_metadata()?.ok_or("bells")?;
    assert_eq!(bells.bell_count, MAX_PENDING_BELLS);
    assert!(bells.bells_saturated);
    assert_eq!(terminal.drain_replies()?, b"\x1b[0n");
    assert_eq!(terminal.take_metadata()?, None);
    terminal.feed(b"\x07")?;
    let next = terminal.take_metadata()?.ok_or("next bell")?;
    assert_eq!(next.bell_count, 1);
    assert!(!next.bells_saturated);
    assert!(next.version > bells.version);
    Ok(())
}

#[test]
fn rejected_metadata_storm_is_coalesced_and_counter_bounded(
) -> Result<(), Box<dyn std::error::Error>> {
    let mut terminal = Terminal::new(12, 3)?;
    let stream =
        b"\x1b]7;https://a/\x07\x1b]7;https://b/\x07".repeat(MAX_PENDING_BELLS as usize / 2 + 8);
    for chunk in stream.chunks(1024 * 1024) {
        terminal.feed(chunk)?;
    }
    let metadata = terminal.take_metadata()?.ok_or("rejections")?;
    assert_eq!(metadata.rejected_updates, MAX_PENDING_BELLS);
    assert_eq!(metadata.bell_count, 0);
    assert_eq!(metadata.cwd_uri, None);
    assert_eq!(terminal.take_metadata()?, None);
    terminal.feed(b"\x1b[5n")?;
    assert_eq!(terminal.drain_replies()?, b"\x1b[0n");
    Ok(())
}

#[test]
fn full_reset_clears_metadata_soft_reset_keeps_it() -> Result<(), Box<dyn std::error::Error>> {
    let mut terminal = Terminal::new(12, 3)?;
    osc(&mut terminal, "2", b"shell")?;
    osc(&mut terminal, "7", b"file:///work")?;
    let before = terminal.take_metadata()?.ok_or("metadata")?;
    terminal.feed(b"\x1b[!p")?;
    assert_eq!(terminal.take_metadata()?, None);
    terminal.feed(b"\x07\x1bc")?;
    let cleared = terminal.take_metadata()?.ok_or("reset")?;
    assert!(cleared.version > before.version);
    assert_eq!(cleared.title, None);
    assert_eq!(cleared.cwd_uri, None);
    assert_eq!(cleared.bell_count, 1);
    Ok(())
}

#[test]
fn empty_values_clear_and_untyped_pwd_paths_are_not_promoted(
) -> Result<(), Box<dyn std::error::Error>> {
    let mut terminal = Terminal::new(12, 3)?;
    osc(&mut terminal, "2", b"shell")?;
    osc(&mut terminal, "7", b"file:///work")?;
    terminal.take_metadata()?;
    osc(&mut terminal, "1337", b"CurrentDir=/untrusted/bare/path")?;
    let rejected = terminal.take_metadata()?.ok_or("bare PWD")?;
    assert_eq!(rejected.cwd_uri.as_deref(), Some("file:///work"));
    assert_eq!(rejected.rejected_updates, 1);
    terminal.feed(b"\x1b]2;\x07\x1b]7;\x07")?;
    let cleared = terminal.take_metadata()?.ok_or("clear")?;
    assert_eq!(cleared.title, None);
    assert_eq!(cleared.cwd_uri, None);
    Ok(())
}

#[test]
fn presentation_and_inspection_never_drain_or_duplicate_metadata(
) -> Result<(), Box<dyn std::error::Error>> {
    let mut terminal = Terminal::new(12, 3)?;
    let mut view = terminal.new_presentation_view()?;
    terminal.feed(b"abc\x1b]2;live\x07\x1b]7;file:///work\x07\x07")?;
    terminal.view_select(
        &mut view,
        Some(Selection::Word(ViewportPoint { x: 1, y: 0 })),
    )?;
    terminal.view_render(&mut view)?;
    terminal.view_copy(&mut view)?;
    terminal.cursor_visual()?;
    terminal.render()?;
    terminal.resize(14, 4)?;
    terminal.format_vt(FORMAT_EXTRAS_ALL, MAX_FORMAT_BYTES)?;
    terminal.continuation()?;
    let metadata = terminal.take_metadata()?.ok_or("live metadata")?;
    assert_eq!(metadata.title.as_deref(), Some("live"));
    assert_eq!(metadata.bell_count, 1);
    terminal.view_render(&mut view)?;
    terminal.render()?;
    assert_eq!(terminal.take_metadata()?, None);
    assert!(terminal.drain_replies()?.is_empty());
    // Clipboard writes stay disabled, and titles cannot be queried back into PTY input.
    terminal.feed(b"\x1b]52;c;c2VjcmV0\x07\x1b[21t")?;
    assert_eq!(terminal.take_metadata()?, None);
    assert!(terminal.drain_replies()?.is_empty());
    Ok(())
}
