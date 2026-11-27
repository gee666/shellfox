mod geometry;
mod wheel;
use super::{TerminalView, runtime::Pending};
use gpui::{Context, MouseDownEvent, MouseMoveEvent, MouseUpEvent, ScrollWheelEvent, Window};
use shellfox_ghostty_vt::{
    MouseAction, MouseButton, MouseEvent, MouseGeometry, Selection, ViewportPoint,
};
use shellfox_runtime::Command;
pub(super) use wheel::Wheel;

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
        self.mouse_tracking && self.may_report(mods)
    }

    /// The wheel also reaches alternate-screen apps, as cursor keys.
    fn wheel_reporting(&self, mods: gpui::Modifiers) -> bool {
        (self.mouse_tracking || self.alternate_scroll) && self.may_report(mods)
    }

    fn may_report(&self, mods: gpui::Modifiers) -> bool {
        let bypass = if cfg!(target_os = "macos") {
            mods.alt
        } else {
            mods.shift
        };
        (self.lease.is_some() || self.peer_can_write()) && !bypass
    }

    fn held_buttons(&self) -> u8 {
        self.outbox.buttons() | self.peer_buttons()
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
        let Some(bounds) = self.bounds else {
            return false;
        };
        let (x, cell_width) =
            crate::renderer::mouse_axis(f32::from(position.x - bounds.left()), self.cell_width);
        let (y, cell_height) =
            crate::renderer::mouse_axis(f32::from(position.y - bounds.top()), self.line_height);
        let event = MouseEvent {
            action,
            button,
            modifiers: shellfox_ghostty_vt::Modifiers {
                shift: mods.shift,
                control: mods.control,
                alt: mods.alt,
                super_key: mods.platform,
            },
            x,
            y,
            any_button_pressed: self.held_buttons() != 0,
        };
        let geometry = MouseGeometry {
            cell_width,
            cell_height,
        };
        let Some(lease) = self.lease else {
            return self.peer_mouse(event, geometry);
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
        let context_click = event.button == gpui::MouseButton::Right
            || cfg!(target_os = "macos")
                && event.button == gpui::MouseButton::Left
                && event.modifiers.control;
        if self.link_pointer_down(point, event, context_click) {
            cx.stop_propagation();
            return;
        }
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
        } else if cfg!(target_os = "linux") && event.button == gpui::MouseButton::Middle {
            self.paste_primary(cx);
        } else if cfg!(target_os = "macos") && context_click {
            let index = usize::from(point.y) * usize::from(self.screen.cols) + usize::from(point.x);
            if !self
                .screen
                .cells
                .get(index)
                .is_some_and(|cell| cell.selected)
            {
                self.select(Some(Selection::Word(point)), cx);
            }
        } else if event.button == gpui::MouseButton::Left {
            self.begin_pointer_selection(point, event.click_count, event.modifiers, cx);
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
        self.update_link_hover(None, cx);
        if self.file_link_motion(event.position) {
            cx.stop_propagation();
            return;
        }
        if self.move_scrollbar_drag(event) {
            cx.stop_propagation();
            cx.notify();
        } else if self.selection_drag.active() || self.held_buttons() != 0 {
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
        if self.move_scrollbar_drag(event) {
            cx.stop_propagation();
            cx.notify();
            return;
        }
        if self.file_link_motion(event.position) {
            cx.stop_propagation();
            return;
        }
        if event.pressed_button.is_none() {
            self.update_link_hover(self.cell_at(event.position, false), cx);
        }
        if event.pressed_button.is_none()
            && (self.held_buttons() != 0 || self.selection_drag.active())
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
        } else if (self.held_buttons() != 0 || self.cell_at(event.position, false).is_some())
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
                    .filter(|button| self.held_buttons() & Self::button_mask(*button) != 0)
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
        if self.link_pointer_up(event, cx) {
            cx.stop_propagation();
            return;
        }
        if event.button == gpui::MouseButton::Left && self.end_scrollbar_drag() {
            cx.stop_propagation();
            cx.notify();
            return;
        }
        let bit = Self::button_mask(event.button);
        if self.held_buttons() & bit != 0 {
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
            if self.selection_drag.moved {
                self.publish_primary(cx);
            }
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
        let on_track = self.scrollbar_track_contains(event.position);
        // The padding below the last row still scrolls the terminal.
        if self.failed || self.closing || !on_track && self.cell_at(event.position, true).is_none()
        {
            return;
        }
        let reporting = !on_track && self.wheel_reporting(event.modifiers);
        let steps = self
            .wheel
            .steps(event.delta, self.line_height, event.modifiers, reporting);
        if reporting {
            for (steps, positive, negative, vertical) in [
                (steps.y, MouseButton::WheelUp, MouseButton::WheelDown, true),
                (
                    steps.x,
                    MouseButton::WheelLeft,
                    MouseButton::WheelRight,
                    false,
                ),
            ] {
                for sent in 0..steps.unsigned_abs() {
                    if !self.send_mouse(
                        MouseAction::Press,
                        Some(if steps > 0 { positive } else { negative }),
                        event.position,
                        event.modifiers,
                    ) {
                        let unsent = (steps.unsigned_abs() - sent) as i32 * steps.signum();
                        self.wheel.refund(vertical, unsent);
                        break;
                    }
                }
            }
        } else if steps.y != 0 {
            self.scroll_lines(-(steps.y as isize));
        }
        cx.stop_propagation();
        cx.notify();
    }

    pub(super) fn scroll_lines(&mut self, delta: isize) -> bool {
        if self.failed || self.closing {
            return false;
        }
        if delta == 0 {
            return true;
        }
        let result = self.apply_presentation(
            shellfox_runtime::PresentationAction::Scroll { delta },
            |terminal| terminal.scroll(delta),
        );
        if let Err(error) = result {
            self.reject(format!("Scroll rejected: {error}"));
            return false;
        }
        self.selection_drag.finish();
        self.link_press = None;
        true
    }
}
