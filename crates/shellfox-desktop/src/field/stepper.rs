//! Up/Down stepping for plain numeric fields. There are no spin buttons.
use super::Field;
use gpui::{Context, Window};

#[derive(Clone, Copy)]
pub(crate) enum NumberKind {
    History,
    Hours,
    Byte,
    Hue,
    Percent,
}

impl NumberKind {
    pub(crate) fn range(self) -> (f64, f64) {
        match self {
            Self::History => (1., 100.),
            Self::Hours => (1., 87_600.),
            Self::Byte => (0., 255.),
            Self::Hue => (0., 360.),
            Self::Percent => (0., 100.),
        }
    }

    fn step(self, value: &str, direction: f64) -> Option<String> {
        let (min, max) = self.range();
        let current = value
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|value| value.is_finite());
        let next = match current {
            None => min,
            Some(value) if value < min && direction < 0. || value > max && direction > 0. => {
                return None;
            }
            // Only hours accept fractions; other kinds step to the next whole number.
            Some(value) if !matches!(self, Self::Hours) && value.fract() != 0. => {
                if direction > 0. {
                    value.ceil()
                } else {
                    value.floor()
                }
            }
            Some(value) => value + direction,
        }
        .clamp(min, max);
        (Some(next) != current).then(|| next.to_string())
    }
}

impl Field {
    pub(super) fn step(&mut self, increase: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.read_only || self.marked.is_some() {
            return;
        }
        let Some(value) = self
            .number_kind
            .and_then(|kind| kind.step(&self.text, if increase { 1. } else { -1. }))
        else {
            return;
        };
        self.focus.focus(window);
        if self
            .replace(Some(0..self.text.encode_utf16().count()), &value)
            .is_some()
        {
            cx.notify();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::NumberKind::*;

    #[test]
    fn numbers_step_inside_their_ranges() {
        assert_eq!(History.step("10", 1.).as_deref(), Some("11"));
        assert_eq!(History.step("100", 1.), None);
        assert_eq!(History.step("1", -1.), None);
        assert_eq!(Hours.step("1.5", 1.).as_deref(), Some("2.5"));
        assert_eq!(Hours.step("", -1.).as_deref(), Some("1"));
        assert_eq!(Hours.step("87599.5", 1.).as_deref(), Some("87600"));
        assert_eq!(Byte.step("0", -1.), None);
        assert_eq!(Byte.step("254", 1.).as_deref(), Some("255"));
        assert_eq!(Byte.step("255", 1.), None);
        assert_eq!(Hue.step("359", 1.).as_deref(), Some("360"));
        assert_eq!(Percent.step("", 1.).as_deref(), Some("0"));
        assert_eq!(Percent.step("1", -1.).as_deref(), Some("0"));
    }
}
