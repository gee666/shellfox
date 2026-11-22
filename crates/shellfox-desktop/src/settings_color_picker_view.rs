//! Layout of the color popup.
use super::*;
use gpui::Hsla;

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

    fn channel_row(&self, caption: &'static str, fields: &[usize]) -> impl IntoElement {
        let width = if fields.len() == 1 { CHANNEL * 3. + GAP * 2. } else { CHANNEL };
        div()
            .h(px(ROW))
            .flex()
            .items_center()
            .gap(px(GAP))
            .child(div().w(px(CAPTION)).text_color(self.palette.muted).child(caption))
            .children(
                fields
                    .iter()
                    .map(|index| div().w(px(width)).child(self.fields[*index].clone())),
            )
    }
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
        let mut swatches = div().flex().items_center().gap(px(3.));
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
            .w(px(PREVIEW))
            .h(px(ROW * 3. + GAP * 2.))
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
        let rows = div()
            .flex()
            .flex_col()
            .gap(px(GAP))
            .child(self.channel_row("HEX", &[0]))
            .child(self.channel_row("RGB", &[1, 2, 3]))
            .child(self.channel_row("HSL", &[4, 5, 6]));
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
            .gap(px(GAP * 2.))
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
            .child(swatches)
            .child(div().flex().gap(px(GAP)).child(preview).child(rows));
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
            .capture_key_down(cx.listener(|picker, event, window, cx| picker.key(event, window, cx)))
            .child(card);
        gpui::deferred(gpui::anchored().position(point(px(0.), px(0.))).child(layer))
            .with_priority(60)
    }
}
