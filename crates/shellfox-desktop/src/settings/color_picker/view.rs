//! Layout of the color popup. The square and the hue slider are a few
//! gradient quads, so they cost the same to paint at any size.
use super::*;
use gpui::{Hsla, MouseButton, linear_color_stop as stop, linear_gradient};

const HUE_STOPS: [u32; 7] = [
    0xff0000, 0xffff00, 0x00ff00, 0x00ffff, 0x0000ff, 0xff00ff, 0xff0000,
];

impl Picker {
    fn swatch(&self, id: (&'static str, usize), rgb: Rgb, cx: &Context<Self>) -> impl IntoElement {
        let p = self.palette;
        let selected = rgb == self.value;
        div()
            .id(id)
            .size(px(SWATCH))
            .flex_shrink_0()
            .rounded(px(2.))
            .bg(tint(rgb))
            .border_1()
            .border_color(if selected { p.foreground } else { p.border })
            .hover(move |style| style.border_color(p.foreground))
            .on_click(cx.listener(move |picker, _, _, cx| picker.select(rgb, cx)))
    }

    fn row(&self, caption: &'static str, fields: &[usize]) -> gpui::Div {
        div()
            .h(px(ROW))
            .flex()
            .items_center()
            .gap(px(GAP))
            .child(
                div()
                    .w(px(CAPTION))
                    .text_color(self.palette.muted)
                    .child(caption),
            )
            .children(fields.iter().map(|index| {
                let width = if *index == 0 {
                    CHANNEL * 2. + GAP
                } else {
                    CHANNEL
                };
                div().w(px(width)).child(self.fields[*index].clone())
            }))
    }

    fn square(&self, cx: &Context<Self>) -> impl IntoElement {
        let white: Hsla = gpui::white();
        let black: Hsla = gpui::black();
        let hue = Hsv {
            saturation: 1.,
            value: 1.,
            ..self.hsv
        }
        .rgb();
        let layer = || div().absolute().inset_0();
        div()
            .id("color-square")
            .relative()
            .w(px(INNER))
            .h(px(SQUARE))
            .flex_shrink_0()
            .bg(tint(hue))
            .child(layer().bg(linear_gradient(
                90.,
                stop(white, 0.),
                stop(white.opacity(0.), 1.),
            )))
            .child(layer().bg(linear_gradient(
                180.,
                stop(black.opacity(0.), 0.),
                stop(black, 1.),
            )))
            .child(marker(
                self.hsv.saturation * INNER,
                (1. - self.hsv.value) * SQUARE,
                gpui::size(px(9.), px(9.)),
            ))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|picker, event: &gpui::MouseDownEvent, window, cx| {
                    picker.drag = Some(Drag::Square);
                    picker.drag_to(Drag::Square, event.position, window.viewport_size(), cx);
                }),
            )
    }

    fn hue_slider(&self, cx: &Context<Self>) -> impl IntoElement {
        let mut strip = div().absolute().inset_0().flex();
        for pair in HUE_STOPS.windows(2) {
            let [from, to] = [pair[0], pair[1]].map(|color| Hsla::from(gpui::rgb(color)));
            strip = strip.child(div().flex_1().h_full().bg(linear_gradient(
                90.,
                stop(from, 0.),
                stop(to, 1.),
            )));
        }
        div()
            .id("color-hue")
            .relative()
            .w(px(INNER))
            .h(px(HUE))
            .flex_shrink_0()
            .child(strip)
            .child(marker(
                self.hsv.hue / 360. * INNER,
                HUE / 2.,
                gpui::size(px(5.), px(HUE + 4.)),
            ))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|picker, event: &gpui::MouseDownEvent, window, cx| {
                    picker.drag = Some(Drag::Hue);
                    picker.drag_to(Drag::Hue, event.position, window.viewport_size(), cx);
                }),
            )
    }
}

/// A hollow white marker with a dark rim, centered on `(x, y)`.
fn marker(x: f32, y: f32, size: gpui::Size<Pixels>) -> gpui::Div {
    div()
        .absolute()
        .left(px(x) - size.width / 2.)
        .top(px(y) - size.height / 2.)
        .w(size.width)
        .h(size.height)
        .rounded(px(2.))
        .border_1()
        .border_color(gpui::white())
        .shadow(vec![gpui::BoxShadow {
            color: gpui::black().opacity(0.6),
            offset: point(px(0.), px(0.)),
            blur_radius: px(1.),
            spread_radius: px(1.),
        }])
}

fn tint([r, g, b]: Rgb) -> Hsla {
    gpui::rgb(u32::from_be_bytes([0, r, g, b])).into()
}

impl Render for Picker {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if std::mem::take(&mut self.focus_on_open) {
            self.fields[0].read(cx).focus(window);
        }
        let p = self.palette;
        let viewport = window.viewport_size();
        let popup = self.popup(viewport);
        let mut swatches = div().h(px(SWATCH)).flex().items_center().gap(px(2.));
        for (index, rgb) in self.presets.iter().enumerate() {
            swatches = swatches.child(self.swatch(("color-preset", index), *rgb, cx));
        }
        if !self.saved.is_empty() {
            swatches = swatches.child(div().w(px(1.)).h(px(SWATCH)).mx(px(2.)).bg(p.border));
            for (index, rgb) in self.saved.iter().enumerate() {
                swatches = swatches.child(self.swatch(("color-saved", index), *rgb, cx));
            }
        }
        let initial = self.initial;
        // New color on top, the color the popup opened with below. Click it to revert.
        let preview = div()
            .w(px(CHANNEL))
            .h(px(ROW))
            .flex_shrink_0()
            .flex()
            .flex_col()
            .border_1()
            .border_color(p.border)
            .child(div().flex_1().bg(tint(self.value)))
            .child(
                div()
                    .id("color-initial")
                    .flex_1()
                    .bg(tint(initial))
                    .on_click(cx.listener(move |picker, _, _, cx| picker.select(initial, cx))),
            );
        let card = div()
            .id("color-picker")
            .absolute()
            .left(popup.origin.x)
            .top(popup.origin.y)
            .w(px(WIDTH))
            .h(px(HEIGHT))
            .p(px(PAD))
            .flex()
            .flex_col()
            .gap(px(GAP))
            .bg(p.panel)
            .border_1()
            .border_color(p.border)
            .rounded(px(4.))
            .shadow(vec![gpui::BoxShadow {
                color: gpui::hsla(0., 0., 0., 0.3),
                offset: point(px(0.), px(4.)),
                blur_radius: px(12.),
                spread_radius: px(0.),
            }])
            .text_size(px(11.))
            .text_color(p.foreground)
            .child(self.square(cx))
            .child(self.hue_slider(cx))
            .child(self.row("HEX", &[0]).child(preview))
            .child(self.row("RGB", &[1, 2, 3]))
            .child(self.row("HSL", &[4, 5, 6]))
            .child(swatches);
        let layer = div()
            .id("color-picker-backdrop")
            .w(viewport.width)
            .h(viewport.height)
            .occlude()
            .capture_any_mouse_down(cx.listener(move |_, event: &gpui::MouseDownEvent, _, cx| {
                if !popup.contains(&event.position) {
                    cx.emit(Event::Close);
                    cx.stop_propagation();
                }
            }))
            .on_mouse_move(
                cx.listener(|picker, event: &gpui::MouseMoveEvent, window, cx| {
                    if let Some(drag) = picker.drag {
                        if event.pressed_button == Some(MouseButton::Left) {
                            picker.drag_to(drag, event.position, window.viewport_size(), cx);
                        } else {
                            picker.drag = None;
                        }
                    }
                }),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|picker, _, _, _| picker.drag = None),
            )
            .capture_key_down(
                cx.listener(|picker, event, window, cx| picker.key(event, window, cx)),
            )
            .child(card);
        gpui::deferred(
            gpui::anchored()
                .position(point(px(0.), px(0.)))
                .child(layer),
        )
        .with_priority(60)
    }
}
