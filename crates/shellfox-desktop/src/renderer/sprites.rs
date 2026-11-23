//! Box drawing, block elements, powerline separators and sextants drawn as
//! cell geometry instead of font glyphs, as Ghostty does. Strokes are snapped
//! to device pixels so neighbouring cells join exactly.
use gpui::{Bounds, Hsla, PathBuilder, Pixels, Window, fill, point, px, size};

mod box_drawing;

/// One cell in device pixels: origin plus integer width and height.
#[derive(Clone, Copy)]
pub(super) struct Cell {
    x: f32,
    y: f32,
    pub w: i32,
    pub h: i32,
    scale: f32,
}

impl Cell {
    fn new(origin: gpui::Point<Pixels>, width: f32, height: f32, scale: f32) -> Self {
        Self {
            x: (f32::from(origin.x) * scale).round(),
            y: (f32::from(origin.y) * scale).round(),
            w: (width * scale).round().max(1.) as i32,
            h: (height * scale).round().max(1.) as i32,
            scale,
        }
    }

    /// Light stroke width: about an eighth of the cell, at least one pixel.
    pub fn light(&self) -> i32 {
        ((self.w as f32 / 8.).round() as i32).max(1)
    }

    fn point(&self, x: f32, y: f32) -> gpui::Point<Pixels> {
        point(px((self.x + x) / self.scale), px((self.y + y) / self.scale))
    }
}

/// A painter that collects device-pixel rectangles and paths for one cell.
pub(super) struct Painter<'a> {
    pub cell: Cell,
    color: Hsla,
    window: &'a mut Window,
}

impl Painter<'_> {
    pub fn rect(&mut self, x0: i32, y0: i32, x1: i32, y1: i32) {
        self.rect_color(x0, y0, x1, y1, self.color);
    }

    fn rect_color(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, color: Hsla) {
        let (x0, x1) = (x0.clamp(0, self.cell.w), x1.clamp(0, self.cell.w));
        let (y0, y1) = (y0.clamp(0, self.cell.h), y1.clamp(0, self.cell.h));
        if x1 <= x0 || y1 <= y0 {
            return;
        }
        let origin = self.cell.point(x0 as f32, y0 as f32);
        let scale = self.cell.scale;
        self.window.paint_quad(fill(
            Bounds::new(
                origin,
                size(px((x1 - x0) as f32 / scale), px((y1 - y0) as f32 / scale)),
            ),
            color,
        ));
    }

    /// A polyline or polygon in device pixels; `width` of `None` fills it.
    pub fn path(&mut self, points: &[(f32, f32)], width: Option<f32>, curve: Option<(f32, f32)>) {
        let mut builder = match width {
            Some(width) => PathBuilder::stroke(px(width / self.cell.scale)),
            None => PathBuilder::fill(),
        };
        let Some((&(x, y), rest)) = points.split_first() else {
            return;
        };
        builder.move_to(self.cell.point(x, y));
        for (index, &(x, y)) in rest.iter().enumerate() {
            match curve {
                // A single quadratic segment between the first two points.
                Some((cx, cy)) if index == 0 => {
                    builder.curve_to(self.cell.point(x, y), self.cell.point(cx, cy))
                }
                _ => builder.line_to(self.cell.point(x, y)),
            }
        }
        if width.is_none() {
            builder.close();
        }
        if let Ok(path) = builder.build() {
            self.window.paint_path(path, self.color);
        }
    }
}

/// Paints `ch` into the cell and returns true, or returns false for text the
/// font should draw.
pub(super) fn paint(
    ch: char,
    origin: gpui::Point<Pixels>,
    width: f32,
    height: f32,
    color: Hsla,
    window: &mut Window,
) -> bool {
    if !handles(ch) {
        return false;
    }
    let cell = Cell::new(origin, width, height, window.scale_factor());
    let mut painter = Painter {
        cell,
        color,
        window,
    };
    match ch as u32 {
        0x2500..=0x257F => box_drawing::paint(ch, &mut painter),
        0x2580..=0x259F => block(ch, &mut painter),
        0xE0B0..=0xE0B7 => powerline(ch, &mut painter),
        _ => sextant(ch, &mut painter),
    }
    true
}

pub(super) fn handles(ch: char) -> bool {
    matches!(ch as u32, 0x2500..=0x259F | 0xE0B0..=0xE0B7 | 0x1FB00..=0x1FB3B)
}

fn block(ch: char, p: &mut Painter) {
    let (w, h) = (p.cell.w, p.cell.h);
    let eighth_h = |n: i32| (h as f32 * n as f32 / 8.).round() as i32;
    let eighth_w = |n: i32| (w as f32 * n as f32 / 8.).round() as i32;
    let (hw, hh) = (w / 2, h / 2);
    let quadrants = |p: &mut Painter, [ul, ur, ll, lr]: [bool; 4]| {
        for (on, x0, y0, x1, y1) in [
            (ul, 0, 0, hw, hh),
            (ur, hw, 0, w, hh),
            (ll, 0, hh, hw, h),
            (lr, hw, hh, w, h),
        ] {
            if on {
                p.rect(x0, y0, x1, y1);
            }
        }
    };
    match ch as u32 {
        0x2580 => p.rect(0, 0, w, hh),
        n @ 0x2581..=0x2588 => p.rect(0, h - eighth_h(n as i32 - 0x2580), w, h),
        n @ 0x2589..=0x258F => p.rect(0, 0, eighth_w(0x2590 - n as i32), h),
        0x2590 => p.rect(hw, 0, w, h),
        n @ 0x2591..=0x2593 => {
            let alpha = (n - 0x2590) as f32 * 0.25;
            let color = Hsla {
                a: p.color.a * alpha,
                ..p.color
            };
            p.rect_color(0, 0, w, h, color);
        }
        0x2594 => p.rect(0, 0, w, eighth_h(1)),
        0x2595 => p.rect(w - eighth_w(1), 0, w, h),
        0x2596 => quadrants(p, [false, false, true, false]),
        0x2597 => quadrants(p, [false, false, false, true]),
        0x2598 => quadrants(p, [true, false, false, false]),
        0x2599 => quadrants(p, [true, false, true, true]),
        0x259A => quadrants(p, [true, false, false, true]),
        0x259B => quadrants(p, [true, true, true, false]),
        0x259C => quadrants(p, [true, true, false, true]),
        0x259D => quadrants(p, [false, true, false, false]),
        0x259E => quadrants(p, [false, true, true, false]),
        _ => quadrants(p, [false, true, true, true]),
    }
}

fn powerline(ch: char, p: &mut Painter) {
    let (w, h) = (p.cell.w as f32, p.cell.h as f32);
    let line = p.cell.light() as f32;
    let half = line / 2.;
    match ch as u32 {
        0xE0B0 => p.path(&[(0., 0.), (w, h / 2.), (0., h)], None, None),
        0xE0B2 => p.path(&[(w, 0.), (0., h / 2.), (w, h)], None, None),
        0xE0B1 => p.path(
            &[(half, half), (w - half, h / 2.), (half, h - half)],
            Some(line),
            None,
        ),
        0xE0B3 => p.path(
            &[(w - half, half), (half, h / 2.), (w - half, h - half)],
            Some(line),
            None,
        ),
        // Half discs: one quadratic per quarter keeps the shape to two paths.
        0xE0B4 => {
            p.path(&[(0., 0.), (w, h / 2.), (0., h / 2.)], None, Some((w, 0.)));
            p.path(&[(0., h), (w, h / 2.), (0., h / 2.)], None, Some((w, h)));
        }
        0xE0B6 => {
            p.path(&[(w, 0.), (0., h / 2.), (w, h / 2.)], None, Some((0., 0.)));
            p.path(&[(w, h), (0., h / 2.), (w, h / 2.)], None, Some((0., h)));
        }
        0xE0B5 => {
            p.path(
                &[(0., half), (w - half, h / 2.)],
                Some(line),
                Some((w - half, half)),
            );
            p.path(
                &[(0., h - half), (w - half, h / 2.)],
                Some(line),
                Some((w - half, h - half)),
            );
        }
        _ => {
            p.path(&[(w, half), (half, h / 2.)], Some(line), Some((half, half)));
            p.path(
                &[(w, h - half), (half, h / 2.)],
                Some(line),
                Some((half, h - half)),
            );
        }
    }
}

/// U+1FB00..1FB3B: a 2x3 grid, skipping the two patterns that are ▌ and ▐.
fn sextant(ch: char, p: &mut Painter) {
    let mut bits = ch as u32 - 0x1FB00 + 1;
    if bits >= 21 {
        bits += 1;
    }
    if bits >= 42 {
        bits += 1;
    }
    let (w, h) = (p.cell.w, p.cell.h);
    let rows = [0, h / 3, h - h / 3, h];
    for row in 0..3 {
        for col in 0..2 {
            if bits & (1 << (row * 2 + col)) != 0 {
                let (x0, x1) = if col == 0 { (0, w / 2) } else { (w / 2, w) };
                p.rect(x0, rows[row as usize], x1, rows[row as usize + 1]);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handled_ranges_cover_boxes_blocks_powerline_and_sextants() {
        for ch in [
            '─',
            '╬',
            '╭',
            '╳',
            '▀',
            '█',
            '░',
            '▟',
            '\u{E0B0}',
            '\u{E0B7}',
            '\u{1FB00}',
            '\u{1FB3B}',
        ] {
            assert!(handles(ch), "{ch:?}");
        }
        for ch in ['a', ' ', '■', '\u{E0B8}', '\u{1FB3C}'] {
            assert!(!handles(ch), "{ch:?}");
        }
    }

    #[test]
    fn cells_snap_to_device_pixels_and_light_lines_scale() {
        let cell = Cell::new(point(px(10.2), px(5.)), 8.8, 18.4, 1.25);
        assert_eq!((cell.x, cell.y, cell.w, cell.h), (13., 6., 11, 23));
        assert_eq!(cell.light(), 1);
        assert_eq!(
            Cell::new(point(px(0.), px(0.)), 26. / 3., 56. / 3., 1.5).light(),
            2
        );
    }
}
