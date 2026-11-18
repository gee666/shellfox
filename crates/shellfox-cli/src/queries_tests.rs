use super::*;

fn render(bytes: &[u8], historical: bool, split: usize) -> Vec<u8> {
    let mut filter = QueryFilter::default();
    bytes
        .chunks(split)
        .flat_map(|chunk| filter.push(chunk, historical))
        .collect()
}
fn assert_fragmented(input: &[u8], expected: &[u8], historical: bool) {
    for split in 1..=input.len().max(1) {
        assert_eq!(
            render(input, historical, split),
            expected,
            "split={split} replay={historical}"
        );
    }
}

#[test]
fn host_queries_never_reach_another_responder() {
    // Same cases as src/cli/terminal-queries.test.ts, plus denied pixel reports.
    for query in [
        b"\x1b[c".as_slice(),
        b"\x1b[0c",
        b"\x1b[>0c",
        b"\x1b[5n",
        b"\x1b[6n",
        b"\x1b[?6n",
        b"\x1b[1$p",
        b"\x1b[?25$p",
        b"\x1bP$qm\x1b\\",
        b"\x1b[18t",
        b"\x1b[14t",
        b"\x1b[16t",
    ] {
        let mut bytes = b"before".to_vec();
        bytes.extend_from_slice(query);
        bytes.extend_from_slice(b"after");
        assert_fragmented(&bytes, b"beforeafter", false);
        assert_fragmented(&bytes, b"beforeafter", true);
    }
}

#[test]
fn clipboard_queries_and_writes_are_denied_both_live_and_replay() {
    for query in [
        b"\x1b]52;c;?\x07".as_slice(),
        b"\x1b]52;c;c2VjcmV0\x1b\\",
        b"\x9d52;p;?\x9c",
        b"\xc2\x9d52;c;?\xc2\x9c",
    ] {
        assert_fragmented(query, b"", false);
        assert_fragmented(query, b"", true);
    }
}

#[test]
fn live_and_historical_color_queries_keep_setters_and_color_slot_indices() {
    let palette = b"\x1b]4;1;rgb:ff/00/00;2;?\x07";
    assert_fragmented(palette, b"\x1b]4;1;rgb:ff/00/00\x07", true);
    assert_fragmented(palette, b"\x1b]4;1;rgb:ff/00/00\x07", false);
    assert_fragmented(
        b"\x1b]10;?;rgb:00/00/00;rgb:ff/ff/ff\x1b\\",
        b"\x1b]11;rgb:00/00/00\x1b\\\x1b]12;rgb:ff/ff/ff\x1b\\",
        true,
    );
    assert_fragmented(b"\x9d4;1;?\x9c", b"", true);
    assert_fragmented(b"\xc2\x9d10;?\xc2\x9c", b"", true);
    let mut filter = QueryFilter::default();
    assert_eq!(filter.push(b"first\x1b]10;", true), b"first");
    assert_eq!(filter.push(b"?\x07live", false), b"live");
}

#[test]
fn query_selectors_allow_leading_zeros_and_ignore_embedded_c0() {
    for bytes in [
        b"\x1b]052;c;?\x07".as_slice(),
        b"\x1b]5\x7f2;c;?\x07",
        b"\x1b]010;?\x07",
        b"\x1b]11;?\x07",
        b"\x1b]12;?\x07",
        b"\x1b]4;1;?\x07",
    ] {
        assert_fragmented(bytes, b"", false);
        assert_fragmented(bytes, b"", true);
    }
}

#[test]
fn user_visible_text_and_non_query_sequences_remain_byte_exact() {
    let bytes = "雪🙂\r\n\x1b[?1049h\x1b[38;2;23;45;67mcolor\x1b[2;3Hcursor\x1b[?25l\x1b[2J\x1b]4;1;rgb:ff/00/00\x07\x1b]0;title\x1b\\\x1bP1;2qimage\x1b\\\x1b[?1049l".as_bytes();
    assert_fragmented(bytes, bytes, false);
    assert_fragmented(bytes, bytes, true);
    // Raw response-shaped data is not globally treated as a query or input.
    let responses = b"\x1b[?61;6;7;22c\x1b[12;34R\x1b[0n";
    assert_fragmented(responses, responses, false);
    assert_fragmented(
        b"\x1b[?61;6;7;22c\x1b[12;34R\x1b[0n",
        b"\x1b[?61;6;7;22c\x1b[12;34R\x1b[0n",
        true,
    );
}

#[test]
fn raw_and_utf8_c1_controls_do_not_confuse_utf8_continuations() {
    assert_fragmented(b"a\x9b6nb\x90$qm\x9cc", b"abc", false);
    assert_fragmented(b"a\xc2\x9b6nb\xc2\x90$qm\xc2\x9cc", b"abc", false);
    // These contain 0x9b / 0x9c / 0x9d as UTF-8 continuation bytes, not controls.
    let text = "雪ܜݝ🙂\u{69b}6n\u{69c}$qm\u{69d}52;c;?".as_bytes();
    assert_fragmented(text, text, true);
    assert_fragmented(text, text, false);
    let invalid = b"\xff\xe2\x9bXsafe";
    assert_fragmented(invalid, invalid, false);
}

#[test]
fn remote_input_modes_and_window_resize_do_not_reconfigure_caller() {
    assert_fragmented(
        b"before\x1b[?9001h\x1b[8;45;117t\x1b[4;800;1200tafter",
        b"beforeafter",
        false,
    );
    assert_fragmented(b"\x1b[?25;9001;1004h", b"\x1b[?25;1004h", false);
    assert_fragmented(b"\x9b?9001l", b"", true);
}

#[test]
fn restart_and_cancel_do_not_leak_queries_or_stale_state() {
    assert_fragmented(b"\x1b[\x1b[6n", b"", false);
    assert_fragmented(b"\x1b[\x07c", b"\x07", false);
    assert_fragmented(b"\x1b]0;unfinished\x18READY", b"\x18READY", false);
    assert_fragmented(b"\x1bP$qunfinished\x1aREADY", b"\x1aREADY", false);
    assert_fragmented(b"\x1b]0;unfinished\x1b[6nREADY", b"READY", false);
    let mut filter = QueryFilter::default();
    assert!(filter.push(b"\x1b]0;old partial", false).is_empty());
    filter.reset();
    assert_eq!(filter.push(b"new screen", true), b"new screen");
}

#[test]
fn bounded_strings_stream_unknown_payloads_but_never_known_denied_payloads() {
    let payload = vec![b'm'; 10000];
    for prefix in [b"\x1bP$q".as_slice(), b"\x1b]52;c;"] {
        let mut bytes = prefix.to_vec();
        bytes.extend_from_slice(&payload);
        bytes.extend_from_slice(b"\x1b\\after");
        for split in [1, 1024, bytes.len()] {
            assert_eq!(render(&bytes, false, split), b"after");
        }
    }
    let mut title = b"\x1b]0;".to_vec();
    title.extend_from_slice(&payload);
    title.extend_from_slice(b"\x1b\\after");
    for split in [1, 1024, title.len()] {
        assert_eq!(render(&title, true, split), title);
    }
    let mut restart = b"\x1b]0;".to_vec();
    restart.extend_from_slice(&payload);
    restart.extend_from_slice(b"\x1b[6nOK");
    let mut expected = b"\x1b]0;".to_vec();
    expected.extend_from_slice(&payload);
    expected.extend_from_slice(b"\x18OK");
    assert_eq!(render(&restart, false, 1), expected);
    for (prefix, suffix) in [
        (b"\x1b]".as_slice(), b"52;c;?\x07after".as_slice()),
        (b"\x1bP", b"$qm\x1b\\after"),
        (b"\x1b]10;", b";?\x07after"),
    ] {
        let mut bytes = prefix.to_vec();
        bytes.extend_from_slice(&vec![b'0'; 5000]);
        bytes.extend_from_slice(suffix);
        assert_eq!(render(&bytes, false, 1), b"after");
    }
    let mut overlong = b"\x1b[".to_vec();
    overlong.extend_from_slice(&vec![b'0'; 5000]);
    overlong.extend_from_slice(b"cOK");
    assert_eq!(render(&overlong, false, 1), b"OK");
}
