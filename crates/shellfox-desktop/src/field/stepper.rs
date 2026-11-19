use super::Field;
use crate::theme::Palette;
use gpui::{Context, Div, Window, canvas, div, point, prelude::*, px, rgb};

#[derive(Clone, Copy)]
pub(crate) enum NumberKind {
    History,
    Hours,
}

impl NumberKind {
    fn step(self, value: &str, direction: f64) -> Option<String> {
        let max = match self { Self::History => 100., Self::Hours => 87_600. };
        let current = value.trim().parse::<f64>().ok().filter(|value| value.is_finite());
        let next = match current {
            None => 1.,
            Some(value) if value < 1. && direction < 0. || value > max && direction > 0. => return None,
            Some(value) if matches!(self, Self::History) && value.fract() != 0. => {
                if direction > 0. { value.ceil() } else { value.floor() }
            }
            Some(value) => value + direction,
        }.clamp(1., max);
        (Some(next) != current).then(|| next.to_string())
    }
}

impl Field {
    pub(super) fn step(&mut self, increase: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.read_only || self.marked.is_some() { return; }
        let Some(value) = self.number_kind.and_then(|kind| kind.step(&self.text, if increase { 1. } else { -1. })) else { return; };
        self.focus.focus(window);
        if self.replace(Some(0..self.text.encode_utf16().count()), &value).is_some() {
            cx.notify();
        }
    }
}

pub(super) const HOVER_GROUP: &str = "settings-number";

pub(super) fn control(palette: Palette, focused: bool, cx: &Context<Field>) -> Div {
    let (background, arrow, highlighted) = if palette.dark {
        (0x2c2c2c, 0x9f9f9f, 0xd1d1d1)
    } else {
        (0xe9e9e9, 0x5f5f5f, 0x242424)
    };
    let mut control = div()
        .absolute().top(px(5.)).right(px(6.)).w(px(15.)).h(px(16.))
        .flex().flex_col().bg(rgb(background))
        .when(!focused, |control| control.opacity(0.).group_hover(HOVER_GROUP, |style| style.opacity(1.)));
    for increase in [true, false] {
        let half = div().id(if increase { "step-up" } else { "step-down" })
            .w_full().h(px(8.)).text_color(rgb(arrow))
            .hover(|style| style.text_color(rgb(highlighted)))
            .on_mouse_down(gpui::MouseButton::Left, cx.listener(move |field, _, window, cx| {
                field.step(increase, window, cx);
                cx.stop_propagation();
            }))
            .child(canvas(|_, _, _| (), move |bounds, _, window, _| {
                let coordinates = if increase { [(7.5, 0.), (12., 6.), (3., 6.)] }
                    else { [(3., 2.), (12., 2.), (7.5, 8.)] };
                let [first, middle, last] = coordinates.map(|(x, y)| bounds.origin + point(px(x), px(y)));
                let mut path = gpui::PathBuilder::fill();
                path.move_to(first);
                path.line_to(middle);
                path.line_to(last);
                path.close();
                if let Ok(path) = path.build() { window.paint_path(path, window.text_style().color); }
            }).size_full());
        control = control.child(half);
    }
    control
}

#[cfg(test)]
mod tests {
    use super::NumberKind::*;

    #[test]
    fn numbers_step_inside_the_same_ranges_as_the_electron_controls() {
        assert_eq!(History.step("10", 1.).as_deref(), Some("11"));
        assert_eq!(History.step("100", 1.), None);
        assert_eq!(History.step("1", -1.), None);
        assert_eq!(Hours.step("1.5", 1.).as_deref(), Some("2.5"));
        assert_eq!(Hours.step("", -1.).as_deref(), Some("1"));
        assert_eq!(Hours.step("87599.5", 1.).as_deref(), Some("87600"));
    }
}
