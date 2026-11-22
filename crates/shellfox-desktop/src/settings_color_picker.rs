//! Compact custom-color popup: preset and saved swatches, a preview and
//! HEX, RGB and HSL fields that stay in sync. Every change applies live.
use super::*;
use crate::field::NumberKind;
use gpui::{Bounds, Hsla, Pixels, point};
use std::{cell::Cell, rc::Rc};

#[path = "settings_color_picker_model.rs"]
mod model;
use model::{Rgb, from_hsl, hex, parse_channel, parse_hex, to_hsl};

const PAD: f32 = 6.;
const GAP: f32 = 4.;
const SWATCH: f32 = 14.;
const ROW: f32 = 24.;
const CAPTION: f32 = 26.;
const CHANNEL: f32 = 40.;
const PREVIEW: f32 = 34.;
const WIDTH: f32 = PAD * 2. + PREVIEW + GAP + CAPTION + CHANNEL * 3. + GAP * 2.;
const HEIGHT: f32 = PAD * 2. + SWATCH + GAP * 2. + ROW * 3. + GAP * 2.;
const KINDS: [Option<NumberKind>; 7] = [
    None,
    Some(NumberKind::Byte),
    Some(NumberKind::Byte),
    Some(NumberKind::Byte),
    Some(NumberKind::Hue),
    Some(NumberKind::Percent),
    Some(NumberKind::Percent),
];

pub(super) type Anchor = Rc<Cell<Option<Bounds<Pixels>>>>;

pub(super) struct Active {
    pub view: Entity<Picker>,
    _subscription: Subscription,
}

enum Event {
    Color(Rgb),
    Close,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Group {
    Hex,
    Rgb,
    Hsl,
}

fn group(index: usize) -> Group {
    match index {
        0 => Group::Hex,
        1..=3 => Group::Rgb,
        _ => Group::Hsl,
    }
}

pub(super) struct Picker {
    anchor: Anchor,
    palette: Palette,
    presets: Vec<Rgb>,
    saved: Vec<Rgb>,
    initial: Rgb,
    value: Rgb,
    hsl: [u16; 3],
    /// HEX, then R G B, then H S L.
    fields: [Entity<Field>; 7],
    seen: [String; 7],
    focus_on_open: bool,
    _observations: Vec<Subscription>,
}
impl EventEmitter<Event> for Picker {}

impl Picker {
    fn new(
        initial: Rgb,
        presets: Vec<Rgb>,
        saved: Vec<Rgb>,
        palette: Palette,
        anchor: Anchor,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let fields = KINDS.map(|kind| {
            let field = Field::color_input(kind, cx);
            field.update(cx, |field, cx| field.apply_palette(palette, cx));
            field
        });
        let mut observations = Vec::new();
        for (index, field) in fields.iter().enumerate() {
            observations.push(cx.observe_in(field, window, move |picker, field, _, cx| {
                if !field.read(cx).is_composing() {
                    picker.edit(index, cx);
                }
            }));
            let focus = field.read(cx).edit_focus();
            observations.push(cx.on_focus(&focus, window, move |picker, _, cx| {
                picker.fields[index].update(cx, Field::select_all_text);
            }));
            observations.push(cx.on_blur(&focus, window, |picker, _, cx| {
                picker.sync_fields(None, cx);
            }));
        }
        let mut picker = Self {
            anchor,
            palette,
            presets,
            saved,
            initial,
            value: initial,
            hsl: to_hsl(initial),
            fields,
            seen: Default::default(),
            focus_on_open: true,
            _observations: observations,
        };
        picker.sync_fields(None, cx);
        picker
    }

    pub(super) fn set_palette(&mut self, palette: Palette, cx: &mut Context<Self>) {
        self.palette = palette;
        for field in &self.fields {
            field.update(cx, |field, cx| field.apply_palette(palette, cx));
        }
        cx.notify();
    }

    fn edit(&mut self, index: usize, cx: &mut Context<Self>) {
        let text = self.fields[index].read(cx).text.clone();
        if text == self.seen[index] {
            return;
        }
        self.seen[index] = text.clone();
        let parsed = match (group(index), KINDS[index]) {
            (Group::Hex, _) => parse_hex(&text).map(|rgb| (rgb, to_hsl(rgb))),
            (group, Some(kind)) => {
                let Some(value) = parse_channel(&text, kind.range().1 as u16) else {
                    return;
                };
                if value.to_string() != text.trim() {
                    // Out-of-range input is clamped in place.
                    self.set_field(index, value.to_string(), cx);
                }
                if group == Group::Rgb {
                    let mut rgb = self.value;
                    rgb[index - 1] = value as u8;
                    Some((rgb, to_hsl(rgb)))
                } else {
                    let mut hsl = self.hsl;
                    hsl[index - 4] = value;
                    Some((from_hsl(hsl), hsl))
                }
            }
            (_, None) => None,
        };
        if let Some((rgb, hsl)) = parsed {
            self.hsl = hsl;
            self.apply(rgb, Some(group(index)), cx);
        }
    }

    fn select(&mut self, rgb: Rgb, cx: &mut Context<Self>) {
        self.hsl = to_hsl(rgb);
        self.apply(rgb, None, cx);
    }

    /// Sets the color and refreshes every group except the one being typed in.
    fn apply(&mut self, rgb: Rgb, editing: Option<Group>, cx: &mut Context<Self>) {
        let changed = rgb != self.value;
        self.value = rgb;
        self.sync_fields(editing, cx);
        if changed {
            cx.emit(Event::Color(rgb));
        }
        cx.notify();
    }

    fn sync_fields(&mut self, skip: Option<Group>, cx: &mut Context<Self>) {
        let [r, g, b] = self.value;
        let [h, s, l] = self.hsl;
        let texts = [
            hex(self.value),
            r.to_string(),
            g.to_string(),
            b.to_string(),
            h.to_string(),
            s.to_string(),
            l.to_string(),
        ];
        for (index, text) in texts.into_iter().enumerate() {
            if Some(group(index)) != skip {
                self.set_field(index, text, cx);
            }
        }
    }

    fn set_field(&mut self, index: usize, text: String, cx: &mut Context<Self>) {
        self.seen[index] = text.clone();
        self.fields[index].update(cx, |field, cx| {
            if field.text != text {
                field.set_text(text, cx);
            }
        });
    }

    fn popup(&self, viewport: gpui::Size<Pixels>) -> Bounds<Pixels> {
        let anchor = self.anchor.get().unwrap_or_default();
        let size = gpui::size(px(WIDTH), px(HEIGHT));
        let below = anchor.bottom() + px(2.);
        let above = anchor.top() - size.height - px(2.);
        let y = if below + size.height <= viewport.height || above < px(0.) {
            below
        } else {
            above
        };
        Bounds::new(
            point(
                anchor.left().min(viewport.width - size.width).max(px(0.)),
                y.min(viewport.height - size.height).max(px(0.)),
            ),
            size,
        )
    }

    fn key(&mut self, event: &gpui::KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.fields.iter().any(|field| field.read(cx).is_composing()) {
            return;
        }
        match event.keystroke.key.as_str() {
            "enter" => cx.emit(Event::Close),
            "escape" if self.value == self.initial => cx.emit(Event::Close),
            "escape" => self.select(self.initial, cx),
            "tab" => {
                let current = self
                    .fields
                    .iter()
                    .position(|field| field.read(cx).is_focused(window))
                    .unwrap_or(0);
                let step = if event.keystroke.modifiers.shift { 6 } else { 1 };
                self.fields[(current + step) % 7].read(cx).focus(window);
            }
            _ => return,
        }
        window.prevent_default();
        cx.stop_propagation();
    }

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

impl SettingsPanel {
    pub(in crate::settings) fn choose_color(
        &mut self,
        kind: api::ColorKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.operations.frozen() || self.color_picker.is_some() {
            return;
        }
        let Some(draft) = &self.draft else { return };
        let (value, presets, saved): (_, &[&str], _) = match kind {
            api::ColorKind::Accent => (
                &draft.value.accent_color,
                &colors::ACCENTS,
                self.custom_colors.as_ref().map(|colors| &colors.accent),
            ),
            api::ColorKind::Background => (
                &draft.value.background_color,
                &colors::BACKGROUNDS,
                self.custom_colors.as_ref().map(|colors| &colors.background),
            ),
        };
        let Some(initial) = parse_hex(value) else { return };
        let presets = presets.iter().filter_map(|color| parse_hex(color)).collect();
        let saved = saved
            .into_iter()
            .flatten()
            .filter_map(|color| parse_hex(color))
            .collect();
        let (anchor, palette) = (self.color_anchor(kind).clone(), self.palette);
        let view = cx.new(|cx| Picker::new(initial, presets, saved, palette, anchor, window, cx));
        let opener = format!("{kind:?}-chooser");
        let subscription = cx.subscribe_in(&view, window, move |panel, _, event, window, cx| {
            match event {
                Event::Close => {
                    panel.color_picker = None;
                    if let Some(focus) = panel.controls.get(&opener) {
                        focus.focus(window);
                    }
                }
                Event::Color(rgb) => {
                    let hex = hex(*rgb);
                    let patch = match kind {
                        api::ColorKind::Accent => SettingsPatch {
                            accent_color: Some(hex),
                            ..Default::default()
                        },
                        api::ColorKind::Background => SettingsPatch {
                            background_color: Some(hex),
                            ..Default::default()
                        },
                    };
                    panel.stage(patch, cx);
                }
            }
            cx.notify();
        });
        self.color_picker = Some(Active {
            view,
            _subscription: subscription,
        });
        cx.notify();
    }

    pub(in crate::settings) fn color_anchor(&self, kind: api::ColorKind) -> &Anchor {
        match kind {
            api::ColorKind::Accent => &self.accent_anchor,
            api::ColorKind::Background => &self.background_anchor,
        }
    }
}

pub(super) fn remember_anchor(anchor: Anchor) -> impl IntoElement {
    gpui::canvas(move |bounds, _, _| anchor.set(Some(bounds)), |_, _, _, _| {})
        .absolute()
        .left(px(-1.))
        .right(px(-1.))
        .top(px(-1.))
        .bottom(px(-1.))
}
