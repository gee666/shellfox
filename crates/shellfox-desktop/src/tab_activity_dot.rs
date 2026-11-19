use crate::theme::Palette;
use gpui::{
    Animation, AnimationExt, Bounds, Div, Hsla, IntoElement, canvas, div, fill, point, prelude::*,
    px, size,
};
use shellfox_service::activity::AgentState;
use std::time::Duration;

fn status_color(state: AgentState, palette: Palette) -> Hsla {
    palette.status_color(match state {
        AgentState::Shell => 0x60a5fa,
        AgentState::Running | AgentState::Busy => 0x4ade80,
        AgentState::Error => 0xfc8397,
        AgentState::Settled => 0x96734e,
    })
}

pub(crate) fn status_dot(state: AgentState, palette: Palette) -> Div {
    let color = status_color(state, palette);
    let slot = div().size(px(8.)).flex_shrink_0();
    if state == AgentState::Busy {
        slot.child(circle(color, 0.).with_animation(
            "agent-activity-pulse",
            Animation::new(Duration::from_millis(1200)).repeat(),
            move |_, phase| circle(color, phase),
        ))
    } else {
        slot.child(circle(color, 0.))
    }
}

fn circle(color: Hsla, phase: f32) -> impl IntoElement {
    let delta = pulse(phase);
    let diameter = px(8. * (1. - 0.15 * delta));
    let color = color.opacity(1. - 0.55 * delta);
    canvas(
        |_, _, _| (),
        move |bounds, (), window, _| {
            let offset = (bounds.size.width - diameter) / 2.;
            let circle = Bounds::new(
                bounds.origin + point(offset, offset),
                size(diameter, diameter),
            );
            window.paint_quad(fill(circle, color).corner_radii(diameter / 2.));
        },
    )
    .size_full()
}

fn pulse(phase: f32) -> f32 {
    let progress = 1. - (2. * phase - 1.).abs();
    if progress <= 0. || progress >= 1. {
        return progress.clamp(0., 1.);
    }
    // Chromium's ease-in-out is cubic-bezier(.42, 0, .58, 1), for each half.
    let (mut low, mut high) = (0., 1.);
    for _ in 0..16 {
        let t = (low + high) / 2.;
        let s = 1. - t;
        let x = 3. * s * s * t * 0.42 + 3. * s * t * t * 0.58 + t * t * t;
        if x < progress {
            low = t;
        } else {
            high = t;
        }
    }
    let t = (low + high) / 2.;
    t * t * (3. - 2. * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_and_busy_are_green_even_with_a_pink_accent() {
        let palette = Palette::new("#ec4899", "#111016");
        for (state, expected) in [
            (AgentState::Shell, 0x60a5fa),
            (AgentState::Running, 0x4ade80),
            (AgentState::Busy, 0x4ade80),
            (AgentState::Error, 0xfc8397),
        ] {
            assert_eq!(status_color(state, palette), gpui::rgb(expected).into());
        }
    }

    #[test]
    fn pulse_matches_css_easing_and_returns_to_the_original_size() {
        for (phase, expected) in [
            (0., 0.),
            (0.125, 0.12916),
            (0.25, 0.5),
            (0.5, 1.),
            (0.75, 0.5),
            (1., 0.),
        ] {
            assert!((super::pulse(phase) - expected).abs() < 0.0001);
        }
    }
}
