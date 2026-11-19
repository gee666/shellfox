use super::*;
use gpui::{BorderStyle, Hsla, MouseButton, linear_color_stop as stop, linear_gradient, size};

fn tint([r, g, b]: Rgb) -> Hsla {
    gpui::rgb(u32::from_be_bytes([0, r, g, b])).into()
}

fn well(hue: f32) -> impl IntoElement {
    gpui::canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            let white: Hsla = gpui::white();
            let black: Hsla = gpui::black();
            let layer = |fill| {
                gpui::quad(
                    bounds,
                    px(0.),
                    fill,
                    px(0.),
                    gpui::transparent_black(),
                    BorderStyle::Solid,
                )
            };
            window.paint_quad(layer(gpui::solid_background(tint(color::hue_rgb(hue)))));
            window.paint_quad(layer(linear_gradient(
                90.,
                stop(white, 0.01),
                stop(white.opacity(0.), 0.99),
            )));
            window.paint_quad(layer(linear_gradient(
                180.,
                stop(black.opacity(0.), 0.01),
                stop(black, 0.99),
            )));
        },
    )
    .size_full()
}

fn hue_strip() -> impl IntoElement {
    gpui::canvas(
        |_, _, _| (),
        |bounds, _, window, _| {
            let width = SLIDER_WIDTH as u16;
            for x in 0..width {
                let color = tint(color::hue_rgb(hue_at((f32::from(x) + 0.5) / SLIDER_WIDTH)));
                let bounds = Bounds::new(
                    bounds.origin + point(px(f32::from(x)), px(0.)),
                    size(px(1.), bounds.size.height),
                );
                window.paint_quad(gpui::fill(bounds, color));
            }
        },
    )
    .size_full()
}

fn ring(x: f32, y: f32, diameter: f32, focused: bool, text: Hsla) -> gpui::Div {
    let outer = diameter + if focused { 4. } else { 0. };
    div()
        .absolute()
        .left(px(x - outer / 2.))
        .top(px(y - outer / 2.))
        .size(px(outer))
        .rounded_full()
        .border_2()
        .border_color(if focused { text } else { gpui::white() })
        .when(focused, |ring| {
            ring.child(
                div()
                    .size_full()
                    .rounded_full()
                    .border_2()
                    .border_color(gpui::white()),
            )
        })
        .when(!focused, |ring| {
            ring.shadow(vec![gpui::BoxShadow {
                color: gpui::black().opacity(0.4),
                offset: point(px(0.), px(0.)),
                blur_radius: px(4.),
                spread_radius: px(0.),
            }])
        })
}

impl Render for Picker {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if std::mem::take(&mut self.focus_on_open) {
            self.well_focus.focus(window);
        }
        let size = window.viewport_size();
        let popup = self.popup(size);
        let origin = popup.origin;
        let [background, border, input_border, text, hover] =
            [0xffffff, 0xbfbfbf, 0xcecece, 0x000000, 0xf7f7f7]
                .map(|hex| gpui::Hsla::from(gpui::rgb(hex)));
        let area = div()
            .id("color-well")
            .relative()
            .w(px(WELL_WIDTH))
            .h(px(WELL_HEIGHT))
            .rounded_t(px(2.))
            .overflow_hidden()
            .track_focus(&self.well_focus)
            .child(well(self.hue))
            .child(ring(
                self.well.0 * WELL_WIDTH,
                self.well.1 * WELL_HEIGHT,
                12.,
                self.well_focus.is_focused(window),
                text,
            ))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|picker, event: &gpui::MouseDownEvent, window, cx| {
                    picker.drag = Some(Drag::Well);
                    picker.well_focus.focus(window);
                    picker.change_pointer(Drag::Well, event.position, window, cx);
                }),
            );
        let slider = div()
            .id("hue-slider")
            .absolute()
            .left(px(87.))
            .top(px(144.))
            .w(px(SLIDER_WIDTH))
            .h(px(32.))
            .track_focus(&self.hue_focus)
            .child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .top(px(10.))
                    .h(px(12.))
                    .rounded(px(2.))
                    .overflow_hidden()
                    .child(hue_strip()),
            )
            .child(ring(
                self.hue_x,
                16.,
                16.,
                self.hue_focus.is_focused(window),
                text,
            ))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|picker, event: &gpui::MouseDownEvent, window, cx| {
                    picker.drag = Some(Drag::Hue);
                    picker.hue_focus.focus(window);
                    picker.change_pointer(Drag::Hue, event.position, window, cx);
                }),
            );
        let mut inputs = div()
            .absolute()
            .top(px(183.))
            .left(px(23.2))
            .w(px(171.))
            .h(px(32.))
            .flex()
            .gap(px(6.84));
        let mut labels = div().flex().w(px(172.)).gap(px(6.84));
        for (index, (_, label, _)) in self.format.fields().iter().enumerate() {
            let field = self.fields[index].clone();
            let active = field.read(cx).is_focused(window);
            inputs = inputs.child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .flex()
                    .items_center()
                    .rounded(px(2.))
                    .border_1()
                    .border_color(if active { text } else { input_border })
                    .child(field),
            );
            labels = labels.child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h(px(24.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(*label),
            );
        }
        let format = div()
            .id("color-format")
            .absolute()
            .left(px(23.2))
            .top(px(219.5))
            .w(px(200.))
            .h(px(24.))
            .flex()
            .items_center()
            .text_size(px(12.))
            .rounded(px(2.))
            .track_focus(&self.format_focus)
            .hover(move |button| button.bg(hover))
            .when(self.format_focus.is_focused(window), |button| {
                button.child(
                    div()
                        .absolute()
                        .left(px(-1.))
                        .right(px(-1.))
                        .top(px(-1.))
                        .bottom(px(-1.))
                        .rounded(px(4.))
                        .border_2()
                        .border_color(gpui::rgb(0xe59700)),
                )
            })
            .child(labels)
            .child(
                div()
                    .w(px(6.))
                    .h(px(8.))
                    .child(crate::assets::icon_sized("color-format", 8.)),
            )
            .on_click(cx.listener(|picker, event: &gpui::ClickEvent, window, cx| {
                if matches!(event, gpui::ClickEvent::Mouse(_)) {
                    picker.format_focus.focus(window);
                    picker.switch(false, cx);
                }
            }));
        let card = div()
            .id("chromium-color-picker")
            .absolute()
            .left(origin.x)
            .top(origin.y)
            .w(px(WIDTH))
            .h(px(HEIGHT))
            .font_family("Arial")
            .text_color(text)
            .bg(background)
            .border_1()
            .border_color(border)
            .child(area)
            .child(
                div()
                    .id("color-eyedropper")
                    .absolute()
                    .left(px(4.64))
                    .top(px(144.))
                    .size(px(32.))
                    .rounded(px(2.))
                    .track_focus(&self.sampler_focus)
                    .when(self.sampling.is_some(), |button| {
                        button.bg(gpui::rgb(0xcecece))
                    })
                    .when(self.sampling.is_none(), |button| {
                        button.hover(move |button| button.bg(hover))
                    })
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(crate::assets::icon_sized("color-eyedropper", 20.))
                    .on_click(cx.listener(|picker, event: &gpui::ClickEvent, window, cx| {
                        if matches!(event, gpui::ClickEvent::Mouse(_)) {
                            picker.begin_sampling(window, cx);
                        }
                    })),
            )
            .child(
                div()
                    .absolute()
                    .left(px(41.3))
                    .top(px(145.))
                    .size(px(32.))
                    .rounded_full()
                    .bg(tint(self.value))
                    .border_1()
                    .border_color(gpui::black().opacity(0.19)),
            )
            .child(slider)
            .child(inputs)
            .child(format);
        let layer = div()
            .id("color-picker-backdrop")
            .w(size.width)
            .h(size.height)
            .occlude()
            .when(self.sampling.is_some(), |layer| {
                layer.cursor(gpui::CursorStyle::Crosshair)
            })
            .capture_any_mouse_down(cx.listener(
                move |picker, event: &gpui::MouseDownEvent, window, cx| {
                    if picker.sampling.is_some() {
                        picker.sample_on_click(event.button, cx);
                        window.prevent_default();
                        cx.stop_propagation();
                    } else if !popup.contains(&event.position) {
                        cx.emit(Event::Close);
                        cx.stop_propagation();
                    }
                },
            ))
            .on_mouse_move(
                cx.listener(|picker, event: &gpui::MouseMoveEvent, window, cx| {
                    if let Some(drag) = picker.drag
                        && event.pressed_button == Some(MouseButton::Left)
                    {
                        picker.change_pointer(drag, event.position, window, cx);
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
