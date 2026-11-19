mod cursor;
mod loading;
pub(crate) use loading::terminal_loading;
mod cells;
pub(crate) use cells::{paint_cells, CellStyle, CellOverlay};
mod surface;
mod theme;
use anyhow::{Context as _, Result, ensure};
use gpui::{Font, Window, font, px, rgb};
use shellfox_ghostty_vt::{Cell, Rgb};
pub use surface::surface;
pub use theme::TerminalTheme;

pub const FONT_SIZE: f32 = 14.0;
pub const HOST_INSET: f32 = 8.0;

pub(crate) fn terminal_baseline_offset(window: &Window) -> gpui::Pixels {
    // At Windows DPI 96, Chrome places centered terminal/menu text one
    // device pixel below GPUI. Do not move cells, selections, or cursor boxes.
    if cfg!(target_os = "windows") { px(1. / window.scale_factor()) } else { px(0.) }
}
// xterm's FitAddon always reserves the 8px overview ruler when scrollback is on.
pub const SCROLLBAR_WIDTH: f32 = 8.0;
pub const HOST_RIGHT_INSET: f32 = 2.0 + SCROLLBAR_WIDTH;

pub fn choose_font(window: &Window, preferred: &str, font_size: f32) -> Result<(Font, f32, f32)> {
    ensure!(
        font_size.is_finite() && (8.0..=40.0).contains(&font_size),
        "font size must be between 8 and 40 pixels"
    );
    let names = window.text_system().all_font_names();
    let family = std::iter::once(preferred)
        .filter(|family| !family.is_empty() && *family != "monospace")
        .chain(["Cascadia Code", "Consolas", "DejaVu Sans Mono", "Menlo", "Liberation Mono"])
        .find(|candidate| names.iter().any(|name| name.eq_ignore_ascii_case(candidate)))
        .context("no supported monospace font found; install Cascadia Code, Consolas, Menlo or DejaVu Sans Mono")?;
    let font = font(family.to_owned());
    let id = window.text_system().resolve_font(&font);
    // xterm measures W with zero letter spacing and does not round each cell.
    let text = window.text_system();
    let width = f32::from(text.advance(id, px(font_size), 'W')?.width);
    let ascent = f32::from(text.ascent(id, px(font_size)));
    let descent = f32::from(text.descent(id, px(font_size)));
    let height = terminal_line_height(ascent, descent, window.scale_factor());
    ensure!(
        width.is_finite() && width > 0.0 && height.is_finite() && height > 0.0,
        "font has invalid cell metrics"
    );
    Ok((font, width, height))
}

fn terminal_line_height(ascent: f32, descent: f32, scale: f32) -> f32 {
    // Canvas TextMetrics rounds the font bounds to CSS pixels. xterm rounds
    // their sum up in device pixels and uses lineHeight: 1.
    ((ascent.round() + descent.abs().round()) * scale).ceil() / scale
}

/// Ghostty's mouse encoder takes integer cell dimensions. Scale its input
/// coordinates into that same grid so they still select the painted cell.
pub(crate) fn mouse_axis(position: f32, cell: f32) -> (f32, u32) {
    let encoder_cell = cell.round().max(1.) as u32;
    (position / cell * encoder_cell as f32, encoder_cell)
}

fn color(value: Rgb) -> gpui::Hsla {
    rgb((u32::from(value.r) << 16) | (u32::from(value.g) << 8) | u32::from(value.b)).into()
}

fn resolved_colors(cell: &Cell) -> (Rgb, Rgb) {
    if cell.inverse {
        (cell.background, cell.foreground)
    } else {
        (cell.foreground, cell.background)
    }
}

fn paints_glyph(cell: &Cell) -> bool {
    !cell.invisible && cell.width != 0 && !cell.text.is_empty() && cell.text != " "
}

/// Dimensions here are the canvas only, not the window or the status line.
pub fn grid_size(width: f32, height: f32, cell_width: f32, line_height: f32) -> (u16, u16) {
    if ![width, height, cell_width, line_height]
        .into_iter()
        .all(f32::is_finite)
        || cell_width <= 0.
        || line_height <= 0.
    {
        return (2, 1);
    }
    let cols = (width.floor() / cell_width).floor().clamp(2.0, 500.0) as u16;
    let row_limit = (shellfox_runtime::MAX_SCREEN_CELLS / usize::from(cols)).min(250);
    let rows = (height.floor() / line_height).floor().clamp(1.0, row_limit as f32) as u16;
    (cols, rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invisible_cells_never_paint_and_inverse_swaps_resolved_colors() {
        let mut terminal = shellfox_ghostty_vt::Terminal::new(10, 2).unwrap();
        terminal.feed(b"\x1b[31;44;7mA\x1b[8mB").unwrap();
        let screen = terminal.render().unwrap();
        let first = &screen.cells[0];
        assert!(first.inverse);
        assert_eq!(resolved_colors(first), (first.background, first.foreground));
        assert!(paints_glyph(first));
        assert!(screen.cells[1].invisible);
        assert!(!paints_glyph(&screen.cells[1]));
    }

    #[test]
    fn xterm_rounds_font_bounds_before_applying_device_scale() {
        assert_eq!(terminal_line_height(12.7, -3.1, 1.), 16.);
        assert_eq!(terminal_line_height(11.7, 3.1, 1.25), 15.2);
    }

    #[test]
    fn fractional_cells_keep_mouse_reporting_on_the_painted_column() {
        let width = 8.2;
        let (x, encoded_width) = mouse_axis(width * 110. + 1., width);
        assert_eq!((x / encoded_width as f32).floor(), 110.);
    }

    #[test]
    fn grid_uses_canvas_size_not_window_chrome() {
        assert_eq!(grid_size(800., 456., 10., 19.), (80, 24));
        // 1200px client, 224px sidebar, 420px Files panel, xterm host + ruler.
        assert_eq!(grid_size(556. - HOST_INSET - HOST_RIGHT_INSET, 714., 8.19, 16.), (65, 44));
        assert_eq!(grid_size(600., 380., 10., 19.), (60, 20));
        assert_eq!(grid_size(0., 0., 10., 19.), (2, 1));
        assert_eq!(grid_size(f32::NAN, 456., 10., 19.), (2, 1));
        assert_eq!(grid_size(800., 456., 0., 19.), (2, 1));
        assert_eq!(grid_size(100000., 100000., 10., 19.), (500, 250));
    }

    #[test]
    fn theme_uses_derived_ansi16_and_standard_extended_colors() {
        let theme = TerminalTheme::default().with_ansi_palette([0x123456; 16]);
        let palette = theme.colors().palette.unwrap();
        assert_eq!(palette[0], Rgb::from([0x12, 0x34, 0x56]));
        assert_eq!(palette[15], palette[0]);
        assert_eq!(palette[16], Rgb::from([0, 0, 0]));
        assert_eq!(palette[21], Rgb::from([0, 0, 255]));
        assert_eq!(palette[231], Rgb::from([255, 255, 255]));
        assert_eq!(palette[232], Rgb::from([8, 8, 8]));
        assert_eq!(palette[255], Rgb::from([238, 238, 238]));
        assert!(
            !TerminalTheme::default()
                .with_ansi_palette([0x1000000; 16])
                .valid()
        );
    }

    #[test]
    fn defaults_match_shellfox_ui() {
        assert_eq!(TerminalTheme::default().background, 0x111016);
        assert_eq!(TerminalTheme::default().foreground, 0xe5e4eb);
        assert_eq!(TerminalTheme::default().accent, 0xec4899);
    }
}
