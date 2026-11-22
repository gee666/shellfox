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

/// Grows and fades in, then back, over one cycle.
fn pulse(phase: f32) -> f32 {
    gpui::ease_in_out(1. - (2. * phase - 1.).abs())
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
    fn idle_stays_green_after_importing_an_electron_accent() {
        let green: Hsla = gpui::rgb(0x4ade80).into();
        for background in ["#111016", "#ffffff", "#777777", "#00ff00"] {
            let pink = Palette::new("#ec4899", background);
            let blue = Palette::new("#0075ff", background);
            let idle = status_color(AgentState::Running, pink);
            assert_eq!(idle, status_color(AgentState::Running, blue));
            assert_ne!(idle, status_color(AgentState::Error, pink));
            assert!((idle.h - green.h).abs() < 0.01);
        }
    }

    #[test]
    fn pulse_eases_out_and_back_to_the_original_size() {
        for (phase, expected) in [
            (0., 0.),
            (0.125, 0.125),
            (0.25, 0.5),
            (0.5, 1.),
            (0.75, 0.5),
            (1., 0.),
        ] {
            assert!((super::pulse(phase) - expected).abs() < 0.0001);
        }
    }
}
