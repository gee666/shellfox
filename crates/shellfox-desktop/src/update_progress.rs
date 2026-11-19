use crate::theme::Palette;
use gpui::{Animation, AnimationExt, AnyElement, Div, div, prelude::*, px, relative};
use std::time::Duration;

pub(super) fn progress(received: u64, total: u64, palette: Palette) -> AnyElement {
    let colors = palette.progress_colors();
    let segment = div().absolute().top_0().bottom_0().bg(gpui::rgb(colors.value))
        .overflow_hidden().child(edge_rows(colors.value_edge));
    let value = if total == 0 {
        segment
            .w(relative(0.20))
            .with_animation(
                "update-indeterminate",
                Animation::new(Duration::from_millis(2200)).repeat(),
                |segment, time| segment.left(relative((1. - (time * 2. - 1.).abs()) * 0.8)),
            )
            .into_any_element()
    } else {
        segment
            .left_0()
            .w(relative((received.min(total) as f64 / total as f64) as f32))
            .when(received > 0, |segment| segment.min_w(px(2.)))
            .into_any_element()
    };
    div()
        .relative()
        .flex_1()
        .min_w_0()
        // The Chromium widget paints a three-pixel track in its five-pixel box.
        .h(px(3.))
        .rounded_full()
        .bg(gpui::rgb(colors.track))
        .overflow_hidden()
        .child(edge_rows(colors.track_edge))
        .child(value)
        .into_any_element()
}

fn edge_rows(color: u32) -> Div {
    let edge = || div().absolute().left(px(1.)).right(px(1.)).h(px(1.)).bg(gpui::rgb(color));
    // The rim is already composited in each sample; a second border would
    // cover the fill again at this height.
    div().absolute().inset_0().child(edge().top_0()).child(edge().bottom_0())
}

#[cfg(test)]
mod tests {
    use crate::theme::Palette;

    #[test]
    fn native_progress_resolves_scheme_and_composites_the_actual_accent() {
        for (accent, expected) in [
            ("#ec4899", (0x4d4d4d, 0xd25794)),
            ("#f35546", (0x4d4d4d, 0xd76155)),
            ("#4575ef", (0xe0e0e0, 0x4b75e0)),
        ] {
            let paint = Palette::new(accent, "#111016").progress_colors();
            assert_eq!((paint.track, paint.value), expected);
        }
        let dark = Palette::new("#ec4899", "#111016").progress_colors();
        let light = Palette::new("#4575ef", "#111016").progress_colors();
        assert_eq!((dark.track_edge, dark.value_edge), (0x616162, 0x8f667b));
        assert_eq!((light.track_edge, light.value_edge), (0xa6a6a7, 0x778abc));
    }
}
