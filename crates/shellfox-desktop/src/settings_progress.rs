use super::*;
pub(super) fn spinner(start: std::time::Instant, palette: Palette) -> impl IntoElement {
    gpui::canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            let radius = px(5.);
            let center = bounds.origin + gpui::point(px(5.5), px(5.5));
            let mut ring = gpui::PathBuilder::stroke(px(1.));
            ring.move_to(center + gpui::point(radius, px(0.)));
            ring.arc_to(
                gpui::point(radius, radius),
                px(0.),
                false,
                true,
                center - gpui::point(radius, px(0.)),
            );
            ring.arc_to(
                gpui::point(radius, radius),
                px(0.),
                false,
                true,
                center + gpui::point(radius, px(0.)),
            );
            if let Ok(path) = ring.build() {
                window.paint_path(path, palette.border);
            }
            let angle = start.elapsed().as_secs_f32() * std::f32::consts::TAU;
            let point =
                |theta: f32| center + gpui::point(px(theta.cos() * 5.), px(theta.sin() * 5.));
            let mut arc = gpui::PathBuilder::stroke(px(1.));
            arc.move_to(point(
                angle - std::f32::consts::FRAC_PI_2 - std::f32::consts::FRAC_PI_4,
            ));
            arc.arc_to(
                gpui::point(radius, radius),
                px(0.),
                false,
                true,
                point(angle - std::f32::consts::FRAC_PI_4),
            );
            if let Ok(path) = arc.build() {
                window.paint_path(path, palette.accent);
            }
            window.request_animation_frame();
        },
    )
    .w(px(11.))
    .h(px(11.))
}
