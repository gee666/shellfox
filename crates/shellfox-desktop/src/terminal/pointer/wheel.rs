use gpui::{Modifiers, ScrollDelta, px};

/// Collects wheel and touchpad deltas into whole terminal lines.
#[derive(Debug, Default)]
pub(in crate::terminal) struct Wheel {
    x: f32,
    y: f32,
}

/// Whole lines from one event. Positive `y` is up, towards history.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Steps {
    pub x: i32,
    pub y: i32,
}

impl Wheel {
    /// `report` means the steps go to the application rather than scrollback.
    pub(super) fn steps(
        &mut self,
        delta: ScrollDelta,
        line_height: f32,
        mods: Modifiers,
        report: bool,
    ) -> Steps {
        let delta = delta.pixel_delta(px(line_height));
        let mut x = f32::from(delta.x) / line_height;
        let mut y = f32::from(delta.y) / line_height;
        if !report {
            // Every platform turns Shift+wheel into horizontal scrolling, yet
            // Shift is the bypass for local scrolling and history is vertical.
            if mods.shift && y == 0. {
                y = x;
            }
            x = 0.;
            if mods.alt {
                y *= 5.;
            }
        }
        if !x.is_finite() || !y.is_finite() {
            return Steps { x: 0, y: 0 };
        }
        self.x = (self.x + x).clamp(-64., 64.);
        self.y = (self.y + y).clamp(-64., 64.);
        let steps = Steps {
            x: self.x.trunc() as i32,
            y: self.y.trunc() as i32,
        };
        self.x -= steps.x as f32;
        self.y -= steps.y as f32;
        steps
    }

    /// Keeps steps the application did not accept for the next event.
    pub(super) fn refund(&mut self, vertical: bool, lines: i32) {
        let axis = if vertical { &mut self.y } else { &mut self.x };
        *axis = (*axis + lines as f32).clamp(-64., 64.);
    }
}

#[cfg(test)]
mod tests;
