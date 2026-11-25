//! Replays keep the receiver's own theme: no host palette, defaults stay default.
use super::*;
use shellfox_ghostty_vt::{ColorSource, Rgb, TerminalColors};

fn themed(fg: Rgb, bg: Rgb, red: Rgb) -> Terminal {
    let mut terminal = Terminal::new(30, 3).unwrap();
    let mut palette = [Rgb { r: 9, g: 9, b: 9 }; 256];
    palette[1] = red;
    terminal
        .set_colors(&TerminalColors {
            foreground: fg,
            background: bg,
            cursor: None,
            palette: Some(palette),
        })
        .unwrap();
    terminal
}

#[test]
fn formatted_replays_send_no_host_palette_and_keep_color_sources() {
    let host = |c: u8| Rgb { r: c, g: c, b: c };
    let mut source = themed(host(1), host(2), host(3));
    source
        .feed(b"plain\x1b[31mred\x1b[38;5;200;44mmix\x1b[0m\x1b[38;2;7;8;9mrgb")
        .unwrap();
    let client_red = Rgb { r: 250, g: 1, b: 1 };
    for replay in [
        remote_v1_ansi(&mut source).unwrap(),
        active_buffer_ansi(&mut source).unwrap(),
    ] {
        assert!(
            !replay.windows(3).any(|w| w == b"\x1b]4"),
            "OSC 4 in the replay"
        );
        let mut client = themed(
            Rgb {
                r: 200,
                g: 201,
                b: 202,
            },
            Rgb {
                r: 20,
                g: 21,
                b: 22,
            },
            client_red,
        );
        client.feed(&replay).unwrap();
        let cells = client.render().unwrap().cells;
        assert_eq!(cells[0].foreground_source, ColorSource::Default);
        assert_eq!(cells[0].background_source, ColorSource::Default);
        assert_eq!(
            cells[0].foreground,
            Rgb {
                r: 200,
                g: 201,
                b: 202
            }
        );
        assert_eq!(
            cells[0].background,
            Rgb {
                r: 20,
                g: 21,
                b: 22
            }
        );
        assert_eq!(cells[5].foreground_source, ColorSource::Palette(1));
        assert_eq!(cells[5].foreground, client_red);
        assert_eq!(cells[8].foreground_source, ColorSource::Palette(200));
        assert_eq!(cells[8].background_source, ColorSource::Palette(4));
        assert_eq!(cells[11].foreground_source, ColorSource::Rgb);
        assert_eq!(cells[11].foreground, Rgb { r: 7, g: 8, b: 9 });
    }
}
