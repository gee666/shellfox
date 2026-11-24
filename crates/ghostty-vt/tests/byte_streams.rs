//! Arbitrary output must never panic or abort the VT, whatever a TUI sends.
//! Covers a recorded c3270 session and seeded random streams built from the
//! sequences full-screen 3270 emulators use.
use shellfox_ghostty_vt::{
    terminal_links, Modifiers, MouseAction, MouseButton, MouseEvent, MouseGeometry, Selection,
    Terminal, ViewportPoint, FORMAT_EXTRAS_ALL, MAX_FORMAT_BYTES,
};

const C3270: &[u8] = include_bytes!("fixtures/c3270-80x24.raw");

/// xorshift64*, so every failure has a reproducible seed.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

#[rustfmt::skip]
const SEQUENCES: &[&str] = &[
    "\x1b[?1049h", "\x1b[?1049l", "\x1b[?47h", "\x1b[?47l", "\x1b[?1047h", "\x1b[?1048h",
    "\x1b[H\x1b[2J", "\x1b[J", "\x1b[K", "\x1b[1K", "\x1b[2K", "\x1b[3J", "\x1b[22;0;0t",
    "\x1b[23;0;0t", "\x1b[8;27;132t", "\x1b[8;43;80t", "\x1b[18t", "\x1b[14t", "\x1b[1;24r",
    "\x1b[r", "\x1b[5;40r", "\x1b[?6h", "\x1b[?6l", "\x1b[?7l", "\x1b[?7h", "\x1b[?3h", "\x1b[?3l",
    "\x1b[?40h", "\x1b[?5h", "\x1b[?5l", "\x1b(0lqqk\x1b(B", "\x1b(0x\x1b(B",
    "\x1b(0mqqjtunwv`afg~\x1b(B", "\x1b)0\x0e", "\x0f", "\x1b[4h", "\x1b[4l", "\x1b[?1h\x1b=",
    "\x1b[?1l\x1b>", "\x1b[?1034h", "\x1b[?1000h", "\x1b[?1002h", "\x1b[?1003h", "\x1b[?1006h",
    "\x1b[?1015h", "\x1b[?1005h", "\x1b[?1000l", "\x1b[?2004h", "\x1b[?2004l", "\x1b[?1004h",
    "\x1b[?25l", "\x1b[?25h", "\x1b[?12h", "\x1b[?12l", "\x1b[2 q", "\x1b[5 q", "\x1b[0m",
    "\x1b[1;4;7m", "\x1b[5m", "\x1b[8m", "\x1b[9m", "\x1b[2m", "\x1b[3m", "\x1b[4:3m",
    "\x1b[58;5;196m", "\x1b[21m", "\x1b[27m", "\x1b[38;5;196m", "\x1b[48;5;21m",
    "\x1b[38;2;255;128;0m", "\x1b[48;2;1;2;3m", "\x1b[38;5;300m", "\x1b[38;2;999;999;999m",
    "\x1b[38:2::1:2:3m", "\x1b[39;49m", "\x1b[92m", "\x1b[1;1H", "\x1b[24;80H", "\x1b[27;132H",
    "\x1b[43;80H", "\x1b[999;999H", "\x1b[0;0H", "\x1b[65535;65535H", "\x1b[3d", "\x1b[200G",
    "\x1b[10A", "\x1b[10B", "\x1b[300C", "\x1b[300D", "\x1b[5@", "\x1b[5P", "\x1b[5X", "\x1b[3L",
    "\x1b[3M", "\x1b[2S", "\x1b[2T", "\x1b[3b", "\x1b[Z", "\x1b[3I", "\x1bH", "\x1b[3g", "\x1b[g",
    "\t", "\r\n", "\n", "\x08", "\x07", "\x1b7", "\x1b8", "\x1b[s", "\x1b[u", "\x1bM", "\x1bD",
    "\x1bE", "\x1bc", "\x1b[!p", "\x1b#8", "\x1b#3", "\x1b#6", "\x1b]0;c3270 - host\x07",
    "\x1b]2;title\x1b\\", "\x1b]1;icon\x07", "\x1b]7;file:///tmp\x07",
    "\x1b]8;;https://example.com\x07link\x1b]8;;\x07", "\x1b]52;c;aGVsbG8=\x07",
    "\x1b]4;1;rgb:ff/00/00\x07", "\x1b]10;?\x07", "\x1b]11;?\x07", "\x1b]104\x07",
    "\x1b]133;A\x07", "\x1b]9;notify\x07", "\x1b]777;notify;a;b\x07", "\x1b[c", "\x1b[>c",
    "\x1b[5n", "\x1b[6n", "\x1b[?6n", "\x1b[>0q", "\x1b[?u", "\x1b[>1u", "\x1b[=1u", "\x1b[<u",
    "\x1b[?2026h", "\x1b[?2026l", "\x1b[?996n", "\x1bP+q544e\x1b\\", "\x1bP$qm\x1b\\",
    "\x1bPq#0;2;0;0;0#0~~\x1b\\", "\x1b_Gi=1;AAAA\x1b\\", "\x1b[200~", "\x1b[201~", "\x1b[",
    "\x1b]", "\x1bP", "\x1b", "\u{9b}31m", "\u{90}", "\u{9c}",
    "\u{2500}\u{2502}\u{250c}\u{2510}\u{2514}\u{2518}\u{253c}\u{2550}\u{256c}\u{2571}\u{2573}",
    "\u{2580}\u{2588}\u{2591}\u{2592}\u{2593}\u{259f}\u{25a0}\u{1fb00}\u{1fb3b}",
    "\u{e0b0}\u{e0b2}\u{e0b4}\u{e0b7}\u{e000}\u{f8ff}\u{f0000}\u{10fffd}",
    "\u{2336}\u{2337}\u{2339}\u{233b}\u{2349}\u{234b}\u{2352}\u{235d}\u{2374}\u{2375}\u{237a}",
    "\u{a2}\u{ac}\u{a6}\u{2190}\u{2192}\u{2260}\u{2264}\u{2265}\u{2228}\u{2227}\u{2207}",
    "e\u{301}", "\u{301}", "\u{300}\u{301}\u{302}\u{303}", "\u{200d}", "\u{fe0f}", "\u{fe0e}",
    "\u{200b}", "\u{feff}", "\u{ad}", "\u{1f468}\u{200d}\u{1f469}\u{200d}\u{1f467}",
    "\u{1f1fa}\u{1f1f8}", "界", "ｱ", "\u{3000}", "\u{fffd}", "\u{1100}\u{1161}\u{11a8}", "\x00",
    "\x7f", "\x05", "\x0b", "\x0c", "\x1a", "\x18",
];

fn chunk(rng: &mut Rng, out: &mut Vec<u8>) {
    match rng.below(10) {
        0..=5 => out.extend_from_slice(SEQUENCES[rng.below(SEQUENCES.len())].as_bytes()),
        6 => out.extend((0..rng.below(16)).map(|_| rng.next() as u8)),
        7 => {
            let ch = char::from_u32(rng.below(0x11_0000) as u32).unwrap_or('\u{fffd}');
            out.extend_from_slice(ch.to_string().as_bytes());
        }
        8 => {
            out.extend_from_slice(format!("\x1b[{};{}H", rng.below(140), rng.below(140)).as_bytes())
        }
        _ => out.extend((0..rng.below(200)).map(|_| b' ' + rng.below(95) as u8)),
    }
}

/// Exercises every read the desktop and host make after output.
fn observe(terminal: &mut Terminal, rng: &mut Rng) {
    let _ = terminal.drain_replies();
    let _ = terminal.take_metadata();
    let _ = terminal.cursor_visual();
    if let Ok(screen) = terminal.render() {
        assert_eq!(
            screen.cells.len(),
            usize::from(screen.cols) * usize::from(screen.rows)
        );
    }
    let Ok(mut view) = terminal.new_presentation_view() else {
        return;
    };
    let _ = terminal.view_scroll(&mut view, rng.below(41) as isize - 20);
    let frame = terminal.view_render(&mut view);
    let (cols, rows) = match &frame {
        Ok(frame) => (frame.screen.cols.max(1), frame.screen.rows.max(1)),
        Err(_) => (1, 1),
    };
    let point = |rng: &mut Rng| ViewportPoint {
        x: rng.below(usize::from(cols) + 2) as u16,
        y: rng.below(usize::from(rows) + 2) as u16,
    };
    let selection = match rng.below(4) {
        0 => Selection::Range {
            start: point(rng),
            end: point(rng),
            rectangle: rng.below(2) == 0,
        },
        1 => Selection::Word(point(rng)),
        2 => Selection::Line(point(rng)),
        _ => Selection::All,
    };
    let _ = terminal.view_select(&mut view, Some(selection));
    let _ = terminal.view_selection(&mut view);
    let _ = terminal.view_copy(&mut view);
    let _ = terminal.view_hyperlink(&mut view, point(rng));
    if let Ok(frame) = terminal.view_render(&mut view) {
        assert_eq!(
            frame.screen.cells.len(),
            usize::from(frame.screen.cols) * usize::from(frame.screen.rows)
        );
        assert_eq!(frame.row_metadata.len(), usize::from(frame.screen.rows));
        let _ = terminal_links::link_at(&frame.screen, &frame.row_metadata, point(rng), true);
    }
    let _ = terminal.set_selection(Some(Selection::Word(point(rng))));
    let _ = terminal.copy_selection();
    let _ = terminal.hyperlink_at(point(rng));
    let _ = terminal.file_link_at(point(rng));
    let _ = terminal.mouse_tracking();
    let _ = terminal.encode_mouse(
        &MouseEvent {
            action: MouseAction::Press,
            button: Some(MouseButton::Left),
            modifiers: Modifiers::default(),
            x: rng.below(2000) as f32 - 10.,
            y: rng.below(2000) as f32 - 10.,
            any_button_pressed: true,
        },
        MouseGeometry {
            cell_width: 8,
            cell_height: 16,
        },
    );
    let _ = terminal.encode_paste("paste\r\nline\x1b[201~");
    let _ = terminal.encode_focus(rng.below(2) == 0);
    let _ = terminal.format_vt(FORMAT_EXTRAS_ALL, MAX_FORMAT_BYTES);
    let _ = terminal.recovery_info();
    let _ = terminal.scroll(rng.below(21) as isize - 10);
}

const MODEL_SIZES: &[(u16, u16)] = &[
    (80, 24),
    (80, 25),
    (80, 32),
    (80, 43),
    (132, 27),
    (160, 62),
    (2, 1),
    (500, 250),
];

#[test]
fn recorded_c3270_session_replays_at_every_model_size() -> Result<(), Box<dyn std::error::Error>> {
    let mut rng = Rng(0x3270);
    for &(cols, rows) in MODEL_SIZES {
        let mut terminal = Terminal::new(cols, rows)?;
        for piece in C3270.chunks(97) {
            terminal.feed(piece)?;
            observe(&mut terminal, &mut rng);
        }
        for &(cols, rows) in MODEL_SIZES {
            terminal.resize(cols, rows)?;
            terminal.feed(C3270)?;
            observe(&mut terminal, &mut rng);
        }
    }
    Ok(())
}

#[test]
fn random_tui_streams_never_panic() -> Result<(), Box<dyn std::error::Error>> {
    let seeds: u64 = std::env::var("SHELLFOX_VT_FUZZ_SEEDS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(20);
    for seed in 1..=seeds {
        let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        let &(cols, rows) = &MODEL_SIZES[rng.below(MODEL_SIZES.len())];
        let mut terminal = Terminal::new(cols, rows)?;
        for _ in 0..200 {
            let mut bytes = Vec::new();
            for _ in 0..rng.below(40) + 1 {
                chunk(&mut rng, &mut bytes);
            }
            // A failed feed is a reported error, never a panic; keep going like the host.
            if terminal.feed(&bytes).is_err() {
                break;
            }
            if rng.below(12) == 0 {
                let &(cols, rows) = &MODEL_SIZES[rng.below(MODEL_SIZES.len())];
                terminal.resize(cols, rows)?;
            }
            if rng.below(3) == 0 {
                observe(&mut terminal, &mut rng);
            }
        }
        observe(&mut terminal, &mut rng);
    }
    Ok(())
}
