mod geometry;
use super::{TerminalView, runtime::Pending};
use gpui::{Context, MouseDownEvent, MouseMoveEvent, MouseUpEvent, ScrollWheelEvent, Window, px};
use shellfox_ghostty_vt::{
    MouseAction, MouseButton, MouseEvent, MouseGeometry, Selection, ViewportPoint,
};
use shellfox_runtime::Command;

impl TerminalView {
    fn cell_at(&self, position: gpui::Point<gpui::Pixels>, clamp: bool) -> Option<ViewportPoint> {
        // Until the first runtime frame arrives, use the requested grid.
        let grid = if self.screen.cols == 0 {
            self.grid
        } else {
            (self.screen.cols, self.screen.rows)
        };
        geometry::hit_cell(
            self.bounds?,
            position,
            self.cell_width,
            self.line_height,
            grid,
            clamp,
        )
    }

    fn reporting(&self, mods: gpui::Modifiers) -> bool {
        let bypass = if cfg!(target_os = "macos") {
            mods.alt
        } else {
            mods.shift
        };
        self.lease.is_some() && self.mouse_tracking && !bypass
    }

    fn button_mask(button: gpui::MouseButton) -> u8 {
        match Self::mouse_button(button) {
            MouseButton::Left => 1,
            MouseButton::Middle => 2,
            MouseButton::Right => 4,
            MouseButton::Back => 8,
            MouseButton::Forward => 16,
            _ => 0,
        }
    }

    fn mouse_button(button: gpui::MouseButton) -> MouseButton {
        match button {
            gpui::MouseButton::Left => MouseButton::Left,
            gpui::MouseButton::Middle => MouseButton::Middle,
            gpui::MouseButton::Right => MouseButton::Right,
            gpui::MouseButton::Navigate(gpui::NavigationDirection::Back) => MouseButton::Back,
            gpui::MouseButton::Navigate(gpui::NavigationDirection::Forward) => MouseButton::Forward,
        }
    }

    fn send_mouse(
        &mut self,
        action: MouseAction,
        button: Option<MouseButton>,
        position: gpui::Point<gpui::Pixels>,
        mods: gpui::Modifiers,
    ) -> bool {
        if self.failed || self.closing {
            return false;
        }
        let (Some(bounds), Some(lease)) = (self.bounds, self.lease) else {
            return false;
        };
        let event = MouseEvent {
            action,
            button,
            modifiers: shellfox_ghostty_vt::Modifiers {
                shift: mods.shift,
                control: mods.control,
                alt: mods.alt,
                super_key: mods.platform,
            },
            x: f32::from(position.x - bounds.left()),
            y: f32::from(position.y - bounds.top()),
            any_button_pressed: self.outbox.buttons() != 0,
        };
        let geometry = MouseGeometry {
            cell_width: self.cell_width as u32,
            cell_height: self.line_height as u32,
        };
        self.submit(
            Command::Mouse {
                lease,
                event,
                geometry,
            },
            Pending::Applied,
        )
    }

    pub(super) fn pointer_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.failed || self.closing {
            return;
        }
        let Some(point) = self.cell_at(event.position, false) else {
            return;
        };
        self.focus.focus(window);
        self.link_press = None;
        self.selection_drag.finish();
        if event.button == gpui::MouseButton::Middle
            && self.screen.cells.iter().any(|cell| cell.selected)
        {
            self.copy_selection(cx);
        } else if self.reporting(event.modifiers) {
            self.send_mouse(
                MouseAction::Press,
                Some(Self::mouse_button(event.button)),
                event.position,
                event.modifiers,
            );
        } else if event.button == gpui::MouseButton::Left {
            match event.click_count {
                2 => self.select(Some(Selection::Word(point)), cx),
                3.. => self.select(Some(Selection::Line(point)), cx),
                _ => {
                    self.select(None, cx);
                    self.selection_drag.begin(point);
                    self.link_press = Some(point);
                }
            }
        }
        cx.stop_propagation();
        cx.notify();
    }

    pub(crate) fn pointer_move_outside(
        &mut self,
        event: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.selection_drag.active() || self.outbox.buttons() != 0 {
            self.pointer_move(event, window, cx);
        }
    }

    pub(super) fn pointer_move(
        &mut self,
        event: &MouseMoveEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.failed || self.closing {
            return;
        }
        if event.pressed_button.is_none()
            && (self.outbox.buttons() != 0 || self.selection_drag.active())
        {
            self.cancel_pointer(cx);
            cx.stop_propagation();
            return;
        }
        if self.selection_drag.active() {
            if let Some(point) = self.cell_at(event.position, true)
                && let Some(selection) = self.selection_drag.extend(point)
            {
                self.link_press = None;
                self.select(Some(selection), cx);
            }
            cx.stop_propagation();
        } else if (self.outbox.buttons() != 0 || self.cell_at(event.position, false).is_some())
            && self.reporting(event.modifiers)
        {
            if event.pressed_button.is_none() && self.screen.cells.iter().any(|cell| cell.selected)
            {
                return;
            }
            self.send_mouse(
                MouseAction::Motion,
                event
                    .pressed_button
                    .filter(|button| self.outbox.buttons() & Self::button_mask(*button) != 0)
                    .map(Self::mouse_button),
                event.position,
                event.modifiers,
            );
            cx.stop_propagation();
        }
    }

    pub(super) fn pointer_up(
        &mut self,
        event: &MouseUpEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.failed || self.closing {
            return;
        }
        let bit = Self::button_mask(event.button);
        if self.outbox.buttons() & bit != 0 {
            self.send_mouse(
                MouseAction::Release,
                Some(Self::mouse_button(event.button)),
                event.position,
                event.modifiers,
            );
        } else if event.button == gpui::MouseButton::Left && self.selection_drag.active() {
            if let Some(point) = self.cell_at(event.position, true)
                && let Some(selection) = self.selection_drag.extend(point)
            {
                self.select(Some(selection), cx);
            }
            self.selection_drag.finish();
            if !self.selection_drag.moved
                && let Some(point) = self.link_press.take()
                && self.cell_at(event.position, false) == Some(point)
            {
                self.activate_link(point, event.modifiers, cx);
            }
        } else {
            return;
        }
        self.link_press = None;
        cx.stop_propagation();
        cx.notify();
    }

    pub(super) fn pointer_scroll(
        &mut self,
        event: &ScrollWheelEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.failed || self.closing || self.cell_at(event.position, false).is_none() {
            return;
        }
        let delta = event.delta.pixel_delta(px(self.line_height));
        self.wheel_remainder.x =
            (self.wheel_remainder.x + f32::from(delta.x) / self.line_height).clamp(-64., 64.);
        self.wheel_remainder.y =
            (self.wheel_remainder.y + f32::from(delta.y) / self.line_height).clamp(-64., 64.);
        let x = self.wheel_remainder.x.trunc() as i32;
        let y = self.wheel_remainder.y.trunc() as i32;
        self.wheel_remainder.x -= x as f32;
        self.wheel_remainder.y -= y as f32;
        if self.reporting(event.modifiers) {
            for (steps, positive, negative, vertical) in [
                (y, MouseButton::WheelUp, MouseButton::WheelDown, true),
                (x, MouseButton::WheelLeft, MouseButton::WheelRight, false),
            ] {
                for sent in 0..steps.unsigned_abs() {
                    if !self.send_mouse(
                        MouseAction::Press,
                        Some(if steps > 0 { positive } else { negative }),
                        event.position,
                        event.modifiers,
                    ) {
                        let remainder = if vertical {
                            &mut self.wheel_remainder.y
                        } else {
                            &mut self.wheel_remainder.x
                        };
                        *remainder = (*remainder
                            + (steps.unsigned_abs() - sent) as f32 * steps.signum() as f32)
                            .clamp(-64., 64.);
                        break;
                    }
                }
            }
        } else if y != 0 {
            let result = self.apply_presentation(
                shellfox_runtime::PresentationAction::Scroll {
                    delta: -(y as isize),
                },
                |terminal| terminal.scroll(-(y as isize)),
            );
            if let Err(error) = result {
                self.reject(format!("Scroll rejected: {error}"));
            }
            self.selection_drag.finish();
            self.link_press = None;
        }
        cx.stop_propagation();
        cx.notify();
    }
}
