//! U+2500..257F. Each character is four arms (up, right, down, left), each
//! none, light, heavy or double. Arms stop where they meet the crossing
//! strokes, so corners and junctions overlap instead of leaving gaps.
use super::Painter;

const N: u8 = 0;
const L: u8 = 1;
const H: u8 = 2;
const D: u8 = 3;

/// Arms for the plain junction characters; `None` marks dashes, arcs and diagonals.
fn arms(ch: char) -> Option<[u8; 4]> {
    let arms = match ch as u32 {
        0x2500 => [N, L, N, L],
        0x2501 => [N, H, N, H],
        0x2502 => [L, N, L, N],
        0x2503 => [H, N, H, N],
        0x250C => [N, L, L, N],
        0x250D => [N, H, L, N],
        0x250E => [N, L, H, N],
        0x250F => [N, H, H, N],
        0x2510 => [N, N, L, L],
        0x2511 => [N, N, L, H],
        0x2512 => [N, N, H, L],
        0x2513 => [N, N, H, H],
        0x2514 => [L, L, N, N],
        0x2515 => [L, H, N, N],
        0x2516 => [H, L, N, N],
        0x2517 => [H, H, N, N],
        0x2518 => [L, N, N, L],
        0x2519 => [L, N, N, H],
        0x251A => [H, N, N, L],
        0x251B => [H, N, N, H],
        0x251C => [L, L, L, N],
        0x251D => [L, H, L, N],
        0x251E => [H, L, L, N],
        0x251F => [L, L, H, N],
        0x2520 => [H, L, H, N],
        0x2521 => [H, H, L, N],
        0x2522 => [L, H, H, N],
        0x2523 => [H, H, H, N],
        0x2524 => [L, N, L, L],
        0x2525 => [L, N, L, H],
        0x2526 => [H, N, L, L],
        0x2527 => [L, N, H, L],
        0x2528 => [H, N, H, L],
        0x2529 => [H, N, L, H],
        0x252A => [L, N, H, H],
        0x252B => [H, N, H, H],
        0x252C => [N, L, L, L],
        0x252D => [N, L, L, H],
        0x252E => [N, H, L, L],
        0x252F => [N, H, L, H],
        0x2530 => [N, L, H, L],
        0x2531 => [N, L, H, H],
        0x2532 => [N, H, H, L],
        0x2533 => [N, H, H, H],
        0x2534 => [L, L, N, L],
        0x2535 => [L, L, N, H],
        0x2536 => [L, H, N, L],
        0x2537 => [L, H, N, H],
        0x2538 => [H, L, N, L],
        0x2539 => [H, L, N, H],
        0x253A => [H, H, N, L],
        0x253B => [H, H, N, H],
        0x253C => [L, L, L, L],
        0x253D => [L, L, L, H],
        0x253E => [L, H, L, L],
        0x253F => [L, H, L, H],
        0x2540 => [H, L, L, L],
        0x2541 => [L, L, H, L],
        0x2542 => [H, L, H, L],
        0x2543 => [H, L, L, H],
        0x2544 => [H, H, L, L],
        0x2545 => [L, L, H, H],
        0x2546 => [L, H, H, L],
        0x2547 => [H, H, L, H],
        0x2548 => [L, H, H, H],
        0x2549 => [H, L, H, H],
        0x254A => [H, H, H, L],
        0x254B => [H, H, H, H],
        0x2550 => [N, D, N, D],
        0x2551 => [D, N, D, N],
        0x2552 => [N, D, L, N],
        0x2553 => [N, L, D, N],
        0x2554 => [N, D, D, N],
        0x2555 => [N, N, L, D],
        0x2556 => [N, N, D, L],
        0x2557 => [N, N, D, D],
        0x2558 => [L, D, N, N],
        0x2559 => [D, L, N, N],
        0x255A => [D, D, N, N],
        0x255B => [L, N, N, D],
        0x255C => [D, N, N, L],
        0x255D => [D, N, N, D],
        0x255E => [L, D, L, N],
        0x255F => [D, L, D, N],
        0x2560 => [D, D, D, N],
        0x2561 => [L, N, L, D],
        0x2562 => [D, N, D, L],
        0x2563 => [D, N, D, D],
        0x2564 => [N, D, L, D],
        0x2565 => [N, L, D, L],
        0x2566 => [N, D, D, D],
        0x2567 => [L, D, N, D],
        0x2568 => [D, L, N, L],
        0x2569 => [D, D, N, D],
        0x256A => [L, D, L, D],
        0x256B => [D, L, D, L],
        0x256C => [D, D, D, D],
        0x2574 => [N, N, N, L],
        0x2575 => [L, N, N, N],
        0x2576 => [N, L, N, N],
        0x2577 => [N, N, L, N],
        0x2578 => [N, N, N, H],
        0x2579 => [H, N, N, N],
        0x257A => [N, H, N, N],
        0x257B => [N, N, H, N],
        0x257C => [N, H, N, L],
        0x257D => [L, N, H, N],
        0x257E => [N, L, N, H],
        0x257F => [H, N, L, N],
        _ => return None,
    };
    Some(arms)
}

/// Stroke positions along one axis of a cell `size` long.
#[derive(Clone, Copy)]
struct Axis {
    size: i32,
    light: i32,
}

impl Axis {
    fn width(self, style: u8) -> i32 {
        if style == H { self.light * 2 } else { self.light }
    }

    /// Start of a centered single stroke.
    fn single(self, style: u8) -> i32 {
        (self.size - self.width(style)) / 2
    }

    /// Start of the first of two double strokes; the second starts 2 light widths later.
    fn double(self) -> i32 {
        (self.size - 3 * self.light) / 2
    }
}

/// Where an arm toward the low end (left or up) of `along` stops, given the
/// perpendicular arms on its two sides. `side` says which perpendicular arm
/// shares a side with the stroke: 0 for the first double stroke or a single.
fn low_end(along: Axis, style: u8, perpendicular: [u8; 2], opposite: u8, side: usize) -> i32 {
    let double = perpendicular.contains(&D);
    let single = perpendicular.iter().copied().filter(|&s| s == L || s == H).max();
    let (d, light) = (along.double(), along.light);
    if style == D {
        return if double {
            if perpendicular[side] != N { d + light } else { d + 3 * light }
        } else if let Some(single) = single {
            along.single(single) + along.width(single)
        } else {
            along.size / 2
        };
    }
    if opposite != N {
        along.size / 2
    } else if double {
        d + light
    } else if let Some(single) = single {
        along.single(single) + along.width(single)
    } else {
        (along.size + along.width(style)) / 2
    }
}

/// Mirror of `low_end` for the arm toward the high end (right or down).
fn high_start(along: Axis, style: u8, perpendicular: [u8; 2], opposite: u8, side: usize) -> i32 {
    let double = perpendicular.contains(&D);
    let single = perpendicular.iter().copied().filter(|&s| s == L || s == H).max();
    let (d, light) = (along.double(), along.light);
    if style == D {
        return if double {
            if perpendicular[side] != N { d + 2 * light } else { d }
        } else if let Some(single) = single {
            along.single(single)
        } else {
            along.size / 2
        };
    }
    if opposite != N {
        along.size / 2
    } else if double {
        d + 2 * light
    } else if let Some(single) = single {
        along.single(single)
    } else {
        (along.size - along.width(style)) / 2
    }
}

/// Strokes across `across` for one arm: one for light/heavy, two for double.
fn strokes(across: Axis, style: u8) -> Vec<(i32, i32)> {
    if style == D {
        let d = across.double();
        vec![(d, d + across.light), (d + 2 * across.light, d + 3 * across.light)]
    } else {
        let s = across.single(style);
        vec![(s, s + across.width(style))]
    }
}

pub(super) fn paint(ch: char, p: &mut Painter) {
    let light = p.cell.light();
    let horizontal = Axis { size: p.cell.w, light };
    let vertical = Axis { size: p.cell.h, light };
    let Some([up, right, down, left]) = arms(ch) else {
        return special(ch, p);
    };
    if left != N {
        for (side, (y0, y1)) in strokes(vertical, left).into_iter().enumerate() {
            p.rect(0, y0, low_end(horizontal, left, [up, down], right, side), y1);
        }
    }
    if right != N {
        for (side, (y0, y1)) in strokes(vertical, right).into_iter().enumerate() {
            p.rect(high_start(horizontal, right, [up, down], left, side), y0, horizontal.size, y1);
        }
    }
    if up != N {
        for (side, (x0, x1)) in strokes(horizontal, up).into_iter().enumerate() {
            p.rect(x0, 0, x1, low_end(vertical, up, [left, right], down, side));
        }
    }
    if down != N {
        for (side, (x0, x1)) in strokes(horizontal, down).into_iter().enumerate() {
            p.rect(x0, high_start(vertical, down, [left, right], up, side), x1, vertical.size);
        }
    }
}

/// Dashes, rounded corners and diagonals.
fn special(ch: char, p: &mut Painter) {
    let (w, h, light) = (p.cell.w, p.cell.h, p.cell.light());
    let horizontal = Axis { size: w, light };
    let vertical = Axis { size: h, light };
    // (count, heavy, vertical) for the dashed lines.
    let dashes = match ch as u32 {
        0x2504 => Some((3, false, false)),
        0x2505 => Some((3, true, false)),
        0x2506 => Some((3, false, true)),
        0x2507 => Some((3, true, true)),
        0x2508 => Some((4, false, false)),
        0x2509 => Some((4, true, false)),
        0x250A => Some((4, false, true)),
        0x250B => Some((4, true, true)),
        0x254C => Some((2, false, false)),
        0x254D => Some((2, true, false)),
        0x254E => Some((2, false, true)),
        0x254F => Some((2, true, true)),
        _ => None,
    };
    if let Some((count, heavy, is_vertical)) = dashes {
        let style = if heavy { H } else { L };
        let length = if is_vertical { h } else { w };
        let gap = (length / (count * 4)).max(1);
        for i in 0..count {
            let start = length * i / count + gap / 2;
            let end = length * (i + 1) / count - (gap - gap / 2);
            if is_vertical {
                let x = horizontal.single(style);
                p.rect(x, start, x + horizontal.width(style), end);
            } else {
                let y = vertical.single(style);
                p.rect(start, y, end, y + vertical.width(style));
            }
        }
        return;
    }
    let (wf, hf, lf) = (w as f32, h as f32, light as f32);
    let radius = (wf / 2.).min(hf / 2.).floor();
    match ch as u32 {
        0x2571 => p.path(&[(wf, 0.), (0., hf)], Some(lf), None),
        0x2572 => p.path(&[(0., 0.), (wf, hf)], Some(lf), None),
        0x2573 => {
            p.path(&[(wf, 0.), (0., hf)], Some(lf), None);
            p.path(&[(0., 0.), (wf, hf)], Some(lf), None);
        }
        _ => rounded(ch, p, horizontal, vertical, radius),
    }
}

/// ╭ ╮ ╯ ╰: straight runs from the two edges, joined by a quarter curve.
fn rounded(ch: char, p: &mut Painter, horizontal: Axis, vertical: Axis, radius: f32) {
    let (x0, y0) = (horizontal.single(L), vertical.single(L));
    let light = horizontal.light;
    let (cx, cy) = (x0 as f32 + light as f32 / 2., y0 as f32 + light as f32 / 2.);
    let r = radius as i32;
    let (down, right) = match ch as u32 {
        0x256D => (true, true),
        0x256E => (true, false),
        0x256F => (false, false),
        _ => (false, true),
    };
    if down {
        p.rect(x0, y0 + r, x0 + light, vertical.size);
    } else {
        p.rect(x0, 0, x0 + light, y0 + light - r);
    }
    if right {
        p.rect(x0 + r, y0, horizontal.size, y0 + light);
    } else {
        p.rect(0, y0, x0 + light - r, y0 + light);
    }
    let rf = r as f32;
    let vertical_end = (cx, if down { cy + rf } else { cy - rf });
    let horizontal_end = (if right { cx + rf } else { cx - rf }, cy);
    p.path(&[vertical_end, horizontal_end], Some(light as f32), Some((cx, cy)));
}

#[cfg(test)]
mod tests {
    use super::*;

    const CELL: Axis = Axis { size: 9, light: 1 };

    #[test]
    fn every_junction_in_the_block_has_arms_or_a_special_shape() {
        let special = [0x2504..=0x250B, 0x254C..=0x254F, 0x256D..=0x2573];
        for code in 0x2500..=0x257F_u32 {
            let ch = char::from_u32(code).unwrap();
            let is_special = special.iter().any(|range| range.contains(&code));
            assert_eq!(arms(ch).is_none(), is_special, "U+{code:04X}");
        }
    }

    #[test]
    fn straight_lines_meet_at_the_center_and_cross_singles_fully() {
        // ─: both halves meet at the middle, no gap.
        assert_eq!(low_end(CELL, L, [N, N], L, 0), high_start(CELL, L, [N, N], L, 0));
        // ┤: the left arm covers the whole vertical stroke.
        assert_eq!(low_end(CELL, L, [L, L], N, 0), CELL.single(L) + 1);
        // ╴: a lone half line reaches just past the center.
        assert_eq!(low_end(CELL, L, [N, N], N, 0), 5);
    }

    #[test]
    fn double_corners_close_inner_and_outer_strokes() {
        let d = CELL.double();
        // ╗: the top stroke runs to the outer edge, the bottom one stops at the inner line.
        assert_eq!(low_end(CELL, D, [N, D], N, 0), d + 3);
        assert_eq!(low_end(CELL, D, [N, D], N, 1), d + 1);
        // ╔: mirrored for the right arm.
        assert_eq!(high_start(CELL, D, [N, D], N, 0), d);
        assert_eq!(high_start(CELL, D, [N, D], N, 1), d + 2);
        // ╬: every stroke stops at the inner line, leaving the center open.
        assert_eq!(low_end(CELL, D, [D, D], D, 0), d + 1);
        assert_eq!(high_start(CELL, D, [D, D], D, 1), d + 2);
    }
}
