//! Real Ghostty callback coverage, independent of native ConPTY interception.
use shellfox_ghostty_vt::{Rgb, Terminal, TerminalColors};

#[test]
fn live_osc_palette_foreground_background_and_cursor_replies_are_host_owned_once() {
    let mut terminal = Terminal::new(80, 24).unwrap();
    let mut palette = [Rgb { r: 0, g: 0, b: 0 }; 256];
    palette[1] = Rgb { r: 3, g: 5, b: 7 };
    terminal
        .set_colors(&TerminalColors {
            foreground: Rgb { r: 1, g: 2, b: 3 },
            background: Rgb { r: 4, g: 5, b: 6 },
            cursor: Some(Rgb { r: 7, g: 8, b: 9 }),
            palette: Some(palette),
        })
        .unwrap();
    // BEL and ST, with every byte separately fed, are both callback-owned.
    let queries = b"\x1b]4;1;?\x1b\\\x1b]10;?\x07\x1b]11;?\x1b\\\x1b]12;?\x07";
    for byte in queries {
        terminal.feed(&[*byte]).unwrap();
    }
    let replies = terminal.drain_replies().unwrap();
    assert_eq!(replies,
        b"\x1b]4;1;rgb:0303/0505/0707\x1b\\\x1b]10;rgb:0101/0202/0303\x07\x1b]11;rgb:0404/0505/0606\x1b\\\x1b]12;rgb:0707/0808/0909\x07");
    terminal.render().unwrap();
    terminal.render().unwrap();
    assert!(terminal.drain_replies().unwrap().is_empty());
}
#[test]
fn cursor_query_without_explicit_cursor_color_uses_authoritative_foreground() {
    let mut terminal = Terminal::new(80, 24).unwrap();
    terminal
        .set_colors(&TerminalColors {
            foreground: Rgb {
                r: 10,
                g: 20,
                b: 30,
            },
            background: Rgb { r: 0, g: 0, b: 0 },
            cursor: None,
            palette: None,
        })
        .unwrap();
    terminal.feed(b"\x1b]12;?\x1b\\").unwrap();
    assert_eq!(
        terminal.drain_replies().unwrap(),
        b"\x1b]12;rgb:0a0a/1414/1e1e\x1b\\"
    );
}
