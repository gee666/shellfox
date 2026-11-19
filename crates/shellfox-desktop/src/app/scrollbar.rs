use super::TerminalView;
use crate::renderer;
use gpui::{div, prelude::*, px, AnyElement, Context, MouseButton, Pixels, Point, Window};
use std::time::{Duration, Instant};

#[derive(Default)]
pub(super) struct Scrollbar {
    pub hovered: bool,
    drag: Option<Drag>,
    fade: Option<Fade>,
    wake_until: Option<Instant>,
    previous_offset: Option<u64>,
}

struct Fade {
    from: f32,
    visible: bool,
    since: Instant,
}

impl Fade {
    fn opacity(&self, now: Instant) -> (f32, bool) {
        let duration = if self.visible { 0.1 } else { 0.8 };
        let portion = (now.saturating_duration_since(self.since).as_secs_f32() / duration).min(1.);
        let to = if self.visible { 1. } else { 0. };
        (self.from + (to - self.from) * portion, portion < 1.)
    }
}

impl Scrollbar {
    fn opacity(&mut self, offset: u64, now: Instant, window: &Window) -> (f32, bool) {
        if self
            .previous_offset
            .replace(offset)
            .is_some_and(|old| old != offset)
        {
            self.wake_until = Some(now + Duration::from_millis(500));
        }
        let waiting =
            !self.hovered && self.drag.is_none() && self.wake_until.is_some_and(|end| end > now);
        let visible = waiting || self.hovered || self.drag.is_some();
        let (opacity, animating) = self
            .fade
            .as_ref()
            .map_or((0., false), |fade| fade.opacity(now));
        let changed = self
            .fade
            .as_ref()
            .map_or(visible, |fade| fade.visible != visible);
        if changed {
            self.fade = Some(Fade {
                from: opacity,
                visible,
                since: now,
            });
        }
        if waiting || animating || changed {
            window.request_animation_frame();
        }
        (opacity, visible)
    }
}

#[derive(Clone, Copy)]
struct Drag {
    origin: Point<Pixels>,
    initial_row: u64,
    submitted_row: u64,
}

#[derive(Clone, Copy)]
struct Geometry {
    height: f32,
    thumb: f32,
    top: f32,
    max_row: u64,
}

impl Geometry {
    fn new(visible_rows: u16, total_rows: u64, top_row: u64, line_height: f32) -> Option<Self> {
        let height = f32::from(visible_rows) * line_height;
        let max_row = total_rows.saturating_sub(u64::from(visible_rows));
        if height <= 20. || max_row == 0 {
            return None;
        }
        // xterm's scrollbar has no buttons, and a thumb at least 20px high.
        let thumb = (height * f32::from(visible_rows) / total_rows as f32)
            .floor()
            .max(20.);
        let top = (top_row.min(max_row) as f32 * (height - thumb) / max_row as f32).round();
        Some(Self {
            height,
            thumb,
            top,
            max_row,
        })
    }

    fn row(self, thumb_top: f32) -> u64 {
        (thumb_top / (self.height - self.thumb) * self.max_row as f32)
            .round()
            .clamp(0., self.max_row as f32) as u64
    }
}

impl TerminalView {
    fn viewport_rows(&self) -> Option<(u64, u64)> {
        match &self.screen {
            super::SharedScreen::Native(frame) => Some((frame.top_row, frame.total_rows)),
            super::SharedScreen::Peer(frame) => {
                Some((frame.rendered.top_row, frame.rendered.total_rows))
            }
            _ => None,
        }
    }

    fn scrollbar_geometry(&self) -> Option<Geometry> {
        let (top, total) = self.viewport_rows()?;
        Geometry::new(self.screen.rows, total, top, self.line_height)
    }

    pub(super) fn scrollbar_track_contains(&self, position: Point<Pixels>) -> bool {
        let (Some(bounds), Some(geometry)) = (self.bounds, self.scrollbar_geometry()) else {
            return false;
        };
        position.x >= bounds.right()
            && position.x < bounds.right() + px(renderer::SCROLLBAR_WIDTH)
            && position.y >= bounds.top()
            && position.y < bounds.top() + px(geometry.height)
    }

    pub(super) fn render_scrollbar(
        &mut self,
        window: &Window,
        cx: &Context<Self>,
    ) -> Option<AnyElement> {
        let (top, total) = self.viewport_rows()?;
        let geometry = Geometry::new(self.screen.rows, total, top, self.line_height)?;
        let (opacity, interactive) = self.scrollbar.opacity(top, Instant::now(), window);
        if opacity <= 0. && !interactive {
            return None;
        }
        let base = self.theme.scrollbar(self.scrollbar.drag.is_some());
        let active = self.theme.scrollbar(true);
        Some(
            div()
                .id("terminal-scrollbar")
                .absolute()
                .top(px(renderer::HOST_INSET))
                .right(px(renderer::HOST_RIGHT_INSET - renderer::SCROLLBAR_WIDTH))
                .w(px(renderer::SCROLLBAR_WIDTH))
                .h(px(geometry.height))
                .opacity(opacity)
                .when(interactive, |bar| {
                    bar.on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|view, event: &gpui::MouseDownEvent, _, cx| {
                            view.start_scrollbar_drag(event.position);
                            cx.stop_propagation();
                            cx.notify();
                        }),
                    )
                })
                .child(
                    div()
                        .id("terminal-scrollbar-thumb")
                        .absolute()
                        .top(px(geometry.top))
                        .w_full()
                        .h(px(geometry.thumb))
                        .bg(base)
                        .hover(move |thumb| thumb.bg(active)),
                )
                .into_any_element(),
        )
    }

    fn start_scrollbar_drag(&mut self, position: Point<Pixels>) {
        let (Some((top, _)), Some(bounds), Some(geometry)) =
            (self.viewport_rows(), self.bounds, self.scrollbar_geometry())
        else {
            return;
        };
        let y = f32::from(position.y - bounds.top());
        let target = if (geometry.top..=geometry.top + geometry.thumb).contains(&y) {
            top
        } else {
            geometry.row(y - geometry.thumb / 2.)
        };
        if self.scroll_lines(target as isize - top as isize) {
            self.scrollbar.drag = Some(Drag {
                origin: position,
                initial_row: target,
                submitted_row: target,
            });
        }
    }

    pub(super) fn move_scrollbar_drag(&mut self, event: &gpui::MouseMoveEvent) -> bool {
        let Some(mut drag) = self.scrollbar.drag else {
            return false;
        };
        if event.pressed_button != Some(MouseButton::Left) {
            self.end_scrollbar_drag();
            return true;
        }
        if let Some(geometry) = self.scrollbar_geometry() {
            let target = if cfg!(target_os = "windows")
                && f32::from(event.position.x - drag.origin.x).abs() > 140.
            {
                drag.initial_row
            } else {
                let top = drag.initial_row as f32 * (geometry.height - geometry.thumb)
                    / geometry.max_row as f32;
                geometry.row(top + f32::from(event.position.y - drag.origin.y))
            };
            if self.scroll_lines(target as isize - drag.submitted_row as isize) {
                drag.submitted_row = target;
                self.scrollbar.drag = Some(drag);
            }
        }
        true
    }

    pub(super) fn end_scrollbar_drag(&mut self) -> bool {
        self.scrollbar.drag.take().is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn thumb_uses_xterm_reveal_and_hide_times() {
        let since = Instant::now();
        let show = Fade {
            from: 0.,
            since,
            visible: true,
        };
        let hide = Fade {
            from: 1.,
            since,
            visible: false,
        };
        assert_eq!(show.opacity(since + Duration::from_millis(50)), (0.5, true));
        assert_eq!(
            show.opacity(since + Duration::from_millis(100)),
            (1., false)
        );
        assert_eq!(
            hide.opacity(since + Duration::from_millis(400)),
            (0.5, true)
        );
        assert_eq!(
            hide.opacity(since + Duration::from_millis(800)),
            (0., false)
        );
    }

    #[test]
    fn slider_has_no_thumb_without_history_and_tracks_bottom() {
        assert!(Geometry::new(25, 25, 0, 16.).is_none());
        let g = Geometry::new(25, 10_000, 9_975, 16.).unwrap();
        assert_eq!(g.thumb, 20.);
        assert_eq!(g.top, 380.);
        assert_eq!(g.row(0.), 0);
        assert_eq!(g.row(380.), 9_975);
        assert_eq!(g.row(700.), 9_975);
    }
}
