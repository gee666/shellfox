use super::*;

fn b64(bytes: &[u8]) -> String {
    STANDARD.encode(bytes)
}
fn row(ok: &str, exe: &str, argv: &[u8], marker: &[u8], name: &str) -> String {
    format!(
        "R\t{ok}\t{}\t{}\t{}\t{}",
        b64(exe.as_bytes()),
        b64(argv),
        b64(marker),
        b64(name.as_bytes())
    )
}
fn listing(rows: &[String]) -> Vec<u8> {
    let mut text = vec![HEADER.to_owned()];
    text.extend_from_slice(rows);
    text.push("END".into());
    (text.join("\n") + "\n").into_bytes()
}
const TAB: &[u8] = b"SHELLFOX_TERMINAL_MARKER=7de47674-b4ab-4f61-98de-878b0a05da02";

#[test]
fn rows_keep_the_marker_command_line_and_name() {
    // The shell script ends grep's marker entry with NUL; Python doesn't.
    let shell_marker = [TAB, b"\0"].concat();
    let output = listing(&[
        row(
            "1",
            "/usr/bin/node",
            b"node\0/pkg/cli.js\0\x001",
            TAB,
            "node",
        ),
        row(
            "0",
            "/tmp/claude",
            b"claude\0\x001",
            &shell_marker,
            "claude",
        ),
    ]);
    let parsed = parse_guest_processes(&output).unwrap();
    assert!(parsed.complete);
    let [node, claude] = &parsed.processes[..] else {
        panic!("two rows")
    };
    assert_eq!(node.marker, "7de47674-b4ab-4f61-98de-878b0a05da02");
    assert!(node.accessible);
    assert_eq!(node.executable.as_deref(), Some("/usr/bin/node"));
    assert_eq!(node.process_name.as_deref(), Some("node"));
    assert_eq!(
        node.argv.as_deref(),
        Some(&["node".to_owned(), "/pkg/cli.js".to_owned()][..])
    );
    assert!(!claude.accessible);
    assert_eq!(claude.marker, node.marker);
}

#[test]
fn unreadable_processes_make_the_listing_incomplete() {
    let output = listing(&["E".into()]);
    let parsed = parse_guest_processes(&output).unwrap();
    assert!(!parsed.complete && parsed.processes.is_empty());
}

#[test]
fn a_partial_or_failed_command_line_is_unknown() {
    // `head -c` stops one byte over the limit.
    let too_long = [vec![b'a'; MAX_FIELD + 1], b"\x001".to_vec()].concat();
    for argv in [&b"node\0\0\x000"[..], b"node\0", &too_long] {
        let parsed = parse_guest_processes(&listing(&[row("1", "/n", argv, TAB, "n")])).unwrap();
        assert!(parsed.processes[0].argv.is_none());
    }
}

#[test]
fn non_utf8_fields_are_not_exact_evidence() {
    let parsed =
        parse_guest_processes(&listing(&[row("1", "/n", b"\xff\0\0\x001", TAB, "n")])).unwrap();
    assert!(!parsed.processes[0].accessible);
}

#[test]
fn rows_without_exactly_one_marker_carry_no_tab() {
    let twice = [TAB, b"\0", TAB].concat();
    for marker in [&b"OTHER=1"[..], &twice, b"SHELLFOX_TERMINAL_MARKER="] {
        let parsed = parse_guest_processes(&listing(&[row("1", "/n", b"", marker, "n")])).unwrap();
        assert!(parsed.processes.is_empty() && parsed.complete);
    }
    let huge = format!("R\t1\t-\t-\t{}\t-", "A".repeat(MAX_MARKER_ENCODED + 4));
    let parsed = parse_guest_processes(&listing(&[huge])).unwrap();
    assert!(parsed.processes.is_empty() && !parsed.complete);
}

#[test]
fn truncated_or_malformed_listings_are_rejected() {
    let good = row("1", "/n", b"", TAB, "n");
    let cases: Vec<Vec<u8>> = vec![
        format!("{HEADER}\n{good}\n").into_bytes(),
        format!("OTHER\n{good}\nEND\n").into_bytes(),
        listing(&[format!("{good}\textra")]),
        listing(&["R\t2\t-\t-\t-\t-".into()]),
        listing(&["R\t1\t!!\t-\t-\t-".into()]),
        listing(&[format!("R\t1\t{}\t-\t-\t-", "A".repeat(MAX_ENCODED + 4))]),
        vec![0xff; 4],
    ];
    for output in cases {
        assert!(parse_guest_processes(&output).is_err());
    }
    let rows = vec![good; MAX_ROWS + 1];
    assert!(parse_guest_processes(&listing(&rows)).is_err());
}
