use shellfox_service::activity::*;

#[test]
fn output_transitions_only_and_restarts_quiet_deadline() {
    let mut activity = TerminalActivity::new();
    assert_eq!(activity.output(b"x", 0), Some(true));
    assert_eq!(activity.output(b"x", 0), None);
    assert_eq!(activity.tick(2499), None);
    assert_eq!(activity.output(b"x", 2499), None);
    assert_eq!(activity.tick(4998), None);
    assert_eq!(activity.tick(4999), Some(false));
    assert_eq!(activity.tick(10000), None);
    assert_eq!(activity.output(b"x", 10000), Some(true));
    assert_eq!(activity.stop(), Some(false));
    assert_eq!(activity.stop(), None);
}

#[test]
fn synchronous_echo_is_ignored_without_extending_busy_deadline() {
    let mut activity = TerminalActivity::new();
    activity.input(0);
    assert_eq!(activity.output(b"echo", 0), None);
    assert_eq!(activity.output(b"echo", 249), None);
    assert_eq!(activity.output(b"work", 250), Some(true));
    activity.input(2650);
    assert_eq!(activity.output(b"echo", 2650), None);
    assert_eq!(activity.tick(2750), Some(false));
}

#[test]
fn split_idle_sequences_are_ignored_but_split_erase_is_work() {
    let mut activity = TerminalActivity::new();
    for bytes in [
        b"\x1b[?2026h\x1b[35;1H\x1b[?25l\x1b[?2026l".as_slice(),
        b"\x1b[?20",
        b"26h\x1b[3",
        b"5;1H\x1b[A\x1b[?25h",
    ] {
        assert_eq!(activity.output(bytes, 0), None);
    }
    assert!(!activity.is_busy());
    assert_eq!(activity.output(b"\x1b[?2026h\x1b[2", 0), None);
    assert_eq!(activity.output(b"KHi", 0), Some(true));
}

#[test]
fn text_erase_colors_title_and_unknown_sequences_count_as_work() {
    for bytes in [
        b"hello".as_slice(),
        b"\x1b[2J",
        b"\x1b[K",
        b"\x1b[?1049h",
        b"\x1b[31m",
        b"\x1b]0;title\x07",
        b"\x1b[?2026h\x1b[1;1Hx",
    ] {
        let mut activity = TerminalActivity::new();
        assert_eq!(activity.output(bytes, 0), Some(true));
        assert_eq!(activity.stop(), Some(false));
    }
}

#[test]
fn idle_output_does_not_extend_work_and_tabs_are_independent() {
    let mut first = TerminalActivity::new();
    let mut second = TerminalActivity::new();
    first.input(0);
    assert_eq!(first.output(b"echo", 0), None);
    assert_eq!(second.output(b"work", 0), Some(true));
    assert_eq!(first.output(b"work", 250), Some(true));
    assert_eq!(
        second.output(b"\x1b[?2026h\x1b[35;1H\x1b[?25l\x1b[?2026l", 2000),
        None
    );
    assert_eq!(second.tick(2500), Some(false));
    assert!(first.is_busy());
    assert_eq!(first.stop(), Some(false));
}

#[test]
fn passive_focus_and_hover_do_not_resume_or_count_as_input() {
    for bytes in [
        b"\x1b[I".as_slice(),
        b"\x1b[O",
        b"\x1b[O\x1b[I",
        b"\x1b[<35;12;8M",
        b"\x1b[<51;13;8M",
        b"\x1b[<35;14;8M\x1b[I",
        b"\x1b[<35;12;8M\x1b[MCAB",
    ] {
        assert!(!meaningful_input(bytes, true));
    }
    for bytes in [
        b"x".as_slice(),
        b"\x03",
        b"\x1b[<0;12;8M",
        b"\x1b[<64;12;8M",
        b"\x1b[<32;12;8M",
        b"\x1b[<35;12;8Mtyped",
        b"\x1b[MCABtyped",
    ] {
        assert!(meaningful_input(bytes, true));
        assert!(!meaningful_input(bytes, false));
    }
    assert!(!meaningful_input(b"", true));
    assert!(!meaningful_input(b"\x1b[1;1R", false));
}

#[test]
fn late_queued_input_preserves_newer_settlement_and_legacy_clients_work() {
    let current = Some("2026-01-02T00:00:00.000Z");
    assert!(!should_unsettle(current, Some(None)));
    assert!(!should_unsettle(
        current,
        Some(Some("2026-01-01T00:00:00.000Z"))
    ));
    assert!(should_unsettle(current, Some(current)));
    assert!(should_unsettle(current, None));
    assert!(!should_unsettle(None, None));
    assert!(!should_unsettle(None, Some(None)));
}

#[test]
fn partial_escape_carry_is_bounded_and_complete_long_cursor_still_idle() {
    let mut activity = TerminalActivity::new();
    let mut cursor = b"\x1b[".to_vec();
    cursor.extend([b'1'; 10000]);
    cursor.push(b'H');
    assert_eq!(activity.output(&cursor, 0), None);
    assert_eq!(activity.output(b"\x1b[12345678901234567", 0), Some(true));
    assert_eq!(activity.stop(), Some(false));
    assert_eq!(activity.output(b"\x1b[", 0), None);
    activity.stop();
    assert_eq!(activity.output(b"H", 0), Some(true));
}
