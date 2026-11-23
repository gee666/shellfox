use gpui::{Bounds, Context, Pixels, Task, Window, fill, point, px, size};
use shellfox_runtime::{CursorShape, CursorVisual, Identity, PresentationLease};
use std::time::{Duration, Instant};

// xterm blinks the cursor on a 1 s cycle, visible for the first half.
const BLINK_INTERVAL: Duration = Duration::from_millis(500);
// The counter restarts the blink after a local selection change.
type CursorKey = (Option<(Identity, PresentationLease)>, CursorVisual, u64);

fn phase_at(elapsed: Duration) -> bool {
    elapsed.as_millis() % 1000 < 500
}
fn until_boundary(elapsed: Duration) -> Duration {
    BLINK_INTERVAL - Duration::from_nanos((elapsed.as_nanos() % BLINK_INTERVAL.as_nanos()) as u64)
}

#[derive(Default)]
struct Blink {
    key: Option<CursorKey>,
    started: Option<Instant>,
    task: Option<Task<()>>,
}
impl Blink {
    fn update(&mut self, key: CursorKey, cx: &mut Context<Self>) -> bool {
        let (_, cursor, _) = key;
        if self.key != Some(key) {
            self.key = Some(key);
            self.started = Some(Instant::now());
            self.task = None;
            if cursor.visible && cursor.blinking {
                self.task = Some(cx.spawn(async move |clock, cx| {
                    loop {
                        let Ok(delay) = clock.update(cx, |clock, _| {
                            until_boundary(
                                clock
                                    .started
                                    .map_or(Duration::ZERO, |start| start.elapsed()),
                            )
                        }) else {
                            break;
                        };
                        cx.background_executor().timer(delay).await;
                        if clock.update(cx, |_, cx| cx.notify()).is_err() {
                            break;
                        }
                    }
                }));
            }
        }
        !cursor.blinking || self.started.is_none_or(|start| phase_at(start.elapsed()))
    }
}

/// The state is owned by the painted canvas, so hidden/dropped views cancel their blink task.
pub(super) fn phase(key: CursorKey, window: &mut Window, cx: &mut gpui::App) -> bool {
    let clock = window.use_keyed_state("terminal-cursor-blink", cx, |_, _| Blink::default());
    clock.update(cx, |clock, cx| clock.update(key, cx))
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct CursorPaint {
    pub column: u16,
    pub row: u16,
    pub width: u8,
    pub shape: CursorShape,
    pub color: shellfox_ghostty_vt::Rgb,
    phase_visible: bool,
}
impl CursorPaint {
    pub fn new(visual: CursorVisual, cols: u16, rows: u16, phase_visible: bool) -> Option<Self> {
        if !visual.visible
            || visual.x >= cols
            || visual.y >= rows
            || !(1..=2).contains(&visual.width)
            || visual.wide_tail && visual.x == 0
        {
            return None;
        }
        let column = visual.x - u16::from(visual.wide_tail);
        if column + u16::from(visual.width) > cols {
            return None;
        }
        Some(Self {
            column,
            row: visual.y,
            width: visual.width,
            shape: visual.shape,
            color: visual.color,
            phase_visible: !visual.blinking || phase_visible,
        })
    }
    pub fn covers_glyph(self, col: usize, row: usize) -> bool {
        self.shape == CursorShape::Block
            && row == usize::from(self.row)
            && (usize::from(self.column)..usize::from(self.column) + usize::from(self.width))
                .contains(&col)
    }
    pub fn glyph_color(self, accent: gpui::Hsla) -> gpui::Hsla {
        if self.phase_visible {
            accent
        } else {
            super::color(self.color)
        }
    }
    pub fn paint(
        self,
        canvas: Bounds<Pixels>,
        cell_width: f32,
        line_height: f32,
        background: gpui::Hsla,
        window: &mut Window,
    ) {
        if !self.phase_visible && self.shape != CursorShape::Block {
            return;
        }
        let mut bounds = Bounds::new(
            canvas.origin
                + point(
                    px(f32::from(self.column) * cell_width),
                    px(f32::from(self.row) * line_height),
                ),
            size(px(f32::from(self.width) * cell_width), px(line_height)),
        );
        match self.shape {
            CursorShape::Bar => bounds.size.width = px(1.).min(bounds.size.width),
            CursorShape::Underline => {
                let thickness = px(1.).min(bounds.size.height);
                bounds.origin.y += bounds.size.height - thickness;
                bounds.size.height = thickness;
            }
            CursorShape::HollowBlock => {
                paint_inset_outline(bounds, super::color(self.color), window);
                return;
            }
            CursorShape::Block => {}
        }
        window.paint_quad(fill(
            bounds,
            if self.phase_visible {
                super::color(self.color)
            } else {
                background
            },
        ));
    }
}

fn paint_inset_outline(bounds: Bounds<Pixels>, color: gpui::Hsla, window: &mut Window) {
    // xterm's square CSS outline is inset by one pixel. Paint edges on the
    // device pixel grid rather than blending a rounded-border distance field.
    let scale = window.scale_factor();
    let snap = |value: Pixels| px((f32::from(value) * scale).round() / scale);
    let left = snap(bounds.left());
    let top = snap(bounds.top());
    let right = snap(bounds.right());
    let bottom = snap(bounds.bottom());
    let width = right - left;
    let height = bottom - top;
    let edge = snap(px(1.)).min(width).min(height);
    if edge <= px(0.) {
        return;
    }
    for (x, y, w, h) in [
        (left, top, width, edge),
        (left, bottom - edge, width, edge),
        (left, top + edge, edge, height - edge * 2.),
        (right - edge, top + edge, edge, height - edge * 2.),
    ] {
        if h > px(0.) {
            window.paint_quad(fill(Bounds::new(point(x, y), size(w, h)), color));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn visual(shape: CursorShape) -> CursorVisual {
        CursorVisual {
            x: 3,
            y: 1,
            visible: true,
            shape,
            blinking: false,
            color: shellfox_ghostty_vt::Rgb::from([0x12, 0x34, 0x56]),
            width: 1,
            wide_tail: false,
        }
    }
    #[test]
    fn shipped_dom_timing_is_one_second_step_end_with_fifty_percent_duty() {
        for (millis, on) in [
            (0, true),
            (499, true),
            (500, false),
            (999, false),
            (1000, true),
            (1499, true),
            (1500, false),
        ] {
            assert_eq!(phase_at(Duration::from_millis(millis)), on);
        }
        assert_eq!(
            until_boundary(Duration::from_millis(501)),
            Duration::from_millis(499)
        );
        assert_eq!(
            until_boundary(Duration::from_millis(1000)),
            Duration::from_millis(500)
        );
    }
    #[test]
    fn exact_authoritative_shapes_colors_widths_and_wide_tail_are_preserved() {
        for shape in [
            CursorShape::Block,
            CursorShape::Bar,
            CursorShape::Underline,
            CursorShape::HollowBlock,
        ] {
            let mut source = visual(shape);
            source.width = 2;
            source.wide_tail = true;
            let cursor = CursorPaint::new(source, 5, 2, true).unwrap();
            assert_eq!(cursor.column, 2);
            assert_eq!(cursor.row, 1);
            assert_eq!(cursor.width, 2);
            assert_eq!(cursor.color, source.color);
            assert_eq!(cursor.shape, shape);
            assert_eq!(cursor.covers_glyph(2, 1), shape == CursorShape::Block);
            assert_eq!(cursor.covers_glyph(3, 1), shape == CursorShape::Block);
            assert!(!cursor.covers_glyph(4, 1));
        }
    }
    #[test]
    fn invisible_or_invalid_visuals_never_guess_a_cursor() {
        let mut source = visual(CursorShape::Block);
        source.visible = false;
        assert!(CursorPaint::new(source, 5, 2, true).is_none());
        source.visible = true;
        source.width = 0;
        assert!(CursorPaint::new(source, 5, 2, true).is_none());
        source.width = 2;
        source.x = 4;
        assert!(CursorPaint::new(source, 5, 2, true).is_none());
        source.x = 0;
        source.wide_tail = true;
        assert!(CursorPaint::new(source, 5, 2, true).is_none());
    }
    #[test]
    fn blink_phase_never_hides_a_steady_or_unfocused_cursor() {
        let mut source = visual(CursorShape::Block);
        source.blinking = true;
        assert!(!CursorPaint::new(source, 5, 2, false).unwrap().phase_visible);
        assert!(CursorPaint::new(source, 5, 2, true).is_some());
        let inactive = source.for_focus(false);
        assert_eq!(
            CursorPaint::new(inactive, 5, 2, false).unwrap().shape,
            CursorShape::HollowBlock
        );
    }
}
