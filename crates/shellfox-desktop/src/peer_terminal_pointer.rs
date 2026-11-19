use super::*;
use gpui::{MouseDownEvent, MouseMoveEvent, MouseUpEvent, Point, ScrollWheelEvent};
use shellfox_ghostty_vt::{Selection, ViewportPoint};
impl PeerTerminalView {
    fn cell(&self, position: Point<Pixels>, clamp: bool) -> Option<(u16, u16)> {
        let bounds = self.bounds?;
        let frame = self.screen.as_ref()?;
        let x = f32::from(position.x - bounds.left());
        let y = f32::from(position.y - bounds.top());
        if !x.is_finite() || !y.is_finite() || frame.cols == 0 || frame.rows == 0 {
            return None;
        }
        if !clamp
            && (x < 0.
                || y < 0.
                || x >= f32::from(frame.cols) * self.cell_width
                || y >= f32::from(frame.rows) * self.line_height)
        {
            return None;
        }
        Some((
            (x / self.cell_width)
                .floor()
                .clamp(0., f32::from(frame.cols - 1)) as u16,
            (y / self.line_height)
                .floor()
                .clamp(0., f32::from(frame.rows - 1)) as u16,
        ))
    }
    fn reports_mouse(&self, modifiers: gpui::Modifiers) -> bool {
        let bypass = if cfg!(target_os = "macos") {
            modifiers.alt
        } else {
            modifiers.shift
        };
        !self.retired
            && !self.frozen
            && !self.input_failed
            && self.ready
            && !bypass
            && self.owner.mouse_tracking().unwrap_or(false)
    }
    fn mouse_button(button: gpui::MouseButton) -> (m::MouseButton, u8) {
        match button {
            gpui::MouseButton::Left => (m::MouseButton::Left, 1),
            gpui::MouseButton::Middle => (m::MouseButton::Middle, 2),
            gpui::MouseButton::Right => (m::MouseButton::Right, 4),
            gpui::MouseButton::Navigate(gpui::NavigationDirection::Back) => {
                (m::MouseButton::Back, 8)
            }
            gpui::MouseButton::Navigate(gpui::NavigationDirection::Forward) => {
                (m::MouseButton::Forward, 16)
            }
        }
    }
    fn mouse(
        &mut self,
        action: m::MouseAction,
        button: Option<m::MouseButton>,
        position: Point<Pixels>,
        modifiers: gpui::Modifiers,
        cx: &mut Context<Self>,
    ) {
        if self.retired || self.frozen || self.input_failed || !self.ready || self.input.pending_count() >= 64
        {
            return;
        }
        let Some(bounds) = self.bounds else {
            return;
        };
        let (x, cell_width) = renderer::mouse_axis(
            f32::from(position.x - bounds.left()), self.cell_width,
        );
        let (y, cell_height) = renderer::mouse_axis(
            f32::from(position.y - bounds.top()), self.line_height,
        );
        let event = m::MouseEvent {
            action,
            button,
            modifiers: m::Modifiers {
                shift: modifiers.shift,
                ctrl: modifiers.control,
                alt: modifiers.alt,
                super_key: modifiers.platform,
            },
            x_px: f64::from(x),
            y_px: f64::from(y),
            any_button_pressed: self.buttons != 0,
        };
        match self.owner.admit_mouse(
            event,
            cell_width,
            cell_height,
            self.hints.clone(),
        ) {
            Ok(input) => {
                self.input.push(input);
                self.send_next(cx);
            }
            Err(error) => {
                self.error = Some(format!("Mouse not admitted: {error:?}"));
                cx.notify();
            }
        }
    }
    pub(super) fn copy(&mut self, cx: &mut Context<Self>) {
        match self.owner.copy_selection() {
            Ok(Some(text)) => cx.write_to_clipboard(gpui::ClipboardItem::new_string(text)),
            Ok(None) => {}
            Err(error) => self.error = Some(format!("Copy failed: {error:?}")),
        }
        cx.notify();
    }
    fn select_cells(
        &mut self,
        start: (u16, u16),
        end: (u16, u16),
        kind: u32,
        cx: &mut Context<Self>,
    ) {
        let start = ViewportPoint {
            x: start.0,
            y: start.1,
        };
        let end = ViewportPoint { x: end.0, y: end.1 };
        let selection = match kind {
            2 => Selection::Word(start),
            3 => Selection::Line(start),
            _ => Selection::Range {
                start,
                end,
                rectangle: false,
            },
        };
        if let Err(error) = self.owner.set_selection(Some(selection)) {
            self.error = Some(format!("Selection failed: {error:?}"));
        }
        cx.notify();
    }
    pub(super) fn down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.retired || self.frozen {
            return;
        }
        let Some(point) = self.cell(event.position, false) else {
            return;
        };
        self.focus(window);
        if event.button == gpui::MouseButton::Left
            && (event.modifiers.control || event.modifiers.platform)
            && let Ok(Some(uri)) = self.owner.hyperlink_at(ViewportPoint {
                x: point.0,
                y: point.1,
            })
            && url::Url::parse(&uri)
                .is_ok_and(|url| matches!(url.scheme(), "http" | "https" | "mailto"))
        {
            cx.open_url(&uri);
            cx.stop_propagation();
            return;
        }
        let (button, mask) = Self::mouse_button(event.button);
        self.buttons |= mask;
        if self.reports_mouse(event.modifiers) {
            self.mouse(
                m::MouseAction::Press,
                Some(button),
                event.position,
                event.modifiers,
                cx,
            );
        } else if event.button == gpui::MouseButton::Left {
            self.selection = (event.click_count <= 1).then_some(point);
            self.select_cells(
                point,
                point,
                match event.click_count {
                    2 => 2,
                    3 => 3,
                    _ => 1,
                },
                cx,
            );
        } else if event.button == gpui::MouseButton::Middle
            && let Some(text) = cx.read_from_clipboard().and_then(|item| item.text())
        {
            self.admit(m::Input::Paste { text }, cx);
        }
        cx.stop_propagation();
    }
    pub(super) fn up(&mut self, event: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        let (button, mask) = Self::mouse_button(event.button);
        self.buttons &= !mask;
        if self.reports_mouse(event.modifiers) {
            self.mouse(
                m::MouseAction::Release,
                Some(button),
                event.position,
                event.modifiers,
                cx,
            );
        } else if let (Some(start), Some(end)) =
            (self.selection.take(), self.cell(event.position, true))
        {
            self.select_cells(start, end, 1, cx);
        }
    }
    pub(super) fn motion(
        &mut self,
        event: &MouseMoveEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.reports_mouse(event.modifiers) {
            self.mouse(
                m::MouseAction::Motion,
                event
                    .pressed_button
                    .map(|button| Self::mouse_button(button).0),
                event.position,
                event.modifiers,
                cx,
            );
        } else if let (Some(start), Some(end)) = (self.selection, self.cell(event.position, true)) {
            self.select_cells(start, end, 1, cx);
        }
    }
    pub(super) fn wheel(
        &mut self,
        event: &ScrollWheelEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.retired || self.frozen || self.cell(event.position, false).is_none() {
            return;
        }
        let delta = event.delta.pixel_delta(px(self.line_height));
        self.wheel.0 = (self.wheel.0 + f32::from(delta.x) / self.line_height).clamp(-64., 64.);
        self.wheel.1 = (self.wheel.1 + f32::from(delta.y) / self.line_height).clamp(-64., 64.);
        let x = self.wheel.0.trunc() as i32;
        let y = self.wheel.1.trunc() as i32;
        self.wheel.0 -= x as f32;
        self.wheel.1 -= y as f32;
        if self.reports_mouse(event.modifiers) {
            for (steps, positive, negative) in [
                (y, m::MouseButton::WheelUp, m::MouseButton::WheelDown),
                (x, m::MouseButton::WheelLeft, m::MouseButton::WheelRight),
            ] {
                for _ in 0..steps.unsigned_abs() {
                    self.mouse(
                        m::MouseAction::Press,
                        Some(if steps > 0 { positive } else { negative }),
                        event.position,
                        event.modifiers,
                        cx,
                    );
                }
            }
        } else if y != 0
            && let Err(error) = self.owner.scroll(-y)
        {
            self.error = Some(format!("Scroll failed: {error:?}"));
        }
        cx.stop_propagation();
        cx.notify();
    }
}
