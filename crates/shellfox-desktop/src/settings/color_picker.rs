//! Compact custom-color popup: a saturation/value square, a hue slider,
//! HEX, RGB and HSL fields, and the preset and saved swatches. They all stay
//! in sync, and every change applies live.
use super::*;
use crate::field::NumberKind;
use gpui::{Bounds, Pixels, point};
use std::{cell::Cell, rc::Rc};

mod model;
mod view;
mod open;
pub(super) use open::remember_anchor;
use model::{Hsv, Rgb, from_hsl, hex, parse_channel, parse_hex, to_hsl};

const PAD: f32 = 6.;
const GAP: f32 = 4.;
const SWATCH: f32 = 12.;
const ROW: f32 = 20.;
const CAPTION: f32 = 22.;
const CHANNEL: f32 = 42.;
const INNER: f32 = CAPTION + GAP + CHANNEL * 3. + GAP * 2.;
const SQUARE: f32 = 96.;
const HUE: f32 = 10.;
/// Content plus padding and the 1 px border.
const WIDTH: f32 = PAD * 2. + INNER + 2.;
const HEIGHT: f32 = PAD * 2. + SQUARE + HUE + ROW * 3. + SWATCH + GAP * 5. + 2.;
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
    /// The square or the hue slider, which own the HSV state while dragged.
    Area,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Drag {
    Square,
    Hue,
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
    hsv: Hsv,
    drag: Option<Drag>,
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
            hsv: Hsv::from_rgb(initial, Hsv { hue: 0., saturation: 0., value: 0. }),
            drag: None,
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
        if editing != Some(Group::Area) {
            self.hsv = Hsv::from_rgb(rgb, self.hsv);
        }
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

    /// Moves the dragged marker to the pointer, clamped to its area.
    fn drag_to(&mut self, drag: Drag, position: gpui::Point<Pixels>, viewport: gpui::Size<Pixels>, cx: &mut Context<Self>) {
        let origin = self.popup(viewport).origin;
        let x = (f32::from(position.x - origin.x) - PAD - 1.) / INNER;
        match drag {
            Drag::Square => {
                let y = (f32::from(position.y - origin.y) - PAD - 1.) / SQUARE;
                self.hsv.saturation = x.clamp(0., 1.);
                self.hsv.value = 1. - y.clamp(0., 1.);
            }
            Drag::Hue => self.hsv.hue = x.clamp(0., 1.) * 360.,
        }
        let rgb = self.hsv.rgb();
        self.hsl = to_hsl(rgb);
        self.apply(rgb, Some(Group::Area), cx);
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
}
