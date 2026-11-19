use super::*;
use gpui::{Bounds, Pixels, Point, Size, point};
use std::{cell::Cell, rc::Rc};

#[path = "settings_picker_color.rs"]
mod color;
#[path = "settings_picker_integration.rs"]
mod integration;
#[path = "settings_picker_view.rs"]
mod view;

#[path = "settings_picker_sampler.rs"]
mod sampler;

use color::{Format, Rgb, hex, hue_at, nearest_hue_x, position_of, well_rgb};

pub(super) const WIDTH: f32 = 234.;
pub(super) const HEIGHT: f32 = 252.;
pub(super) const WELL_WIDTH: f32 = 232.;
pub(super) const WELL_HEIGHT: f32 = 138.;
pub(super) const SLIDER_WIDTH: f32 = 130.;

pub(super) type Anchor = Rc<Cell<Option<Bounds<Pixels>>>>;

pub(super) struct Active {
    pub view: Entity<Picker>,
    _subscription: Subscription,
}

#[derive(Clone)]
enum Event {
    Color(Rgb),
    Close,
    SamplingError(String),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Drag {
    Well,
    Hue,
}

pub(super) struct Picker {
    anchor: Anchor,
    initial: Rgb,
    value: Rgb,
    hue: f32,
    hue_x: f32,
    well: (f32, f32),
    format: Format,
    channels: [u16; 3],
    fields: [Entity<Field>; 3],
    seen: [String; 3],
    well_focus: FocusHandle,
    focus_on_open: bool,
    hue_focus: FocusHandle,
    sampler_focus: FocusHandle,
    sampling: Option<sampler::Sampling>,
    #[cfg(not(windows))]
    sampling_epoch: u64,
    format_focus: FocusHandle,
    drag: Option<Drag>,
    _observations: Vec<Subscription>,
}
impl EventEmitter<Event> for Picker {}

impl Picker {
    fn new(initial: Rgb, anchor: Anchor, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let initial_position = position_of(initial);
        let format = Format::Rgb;
        let channels = format.channels(initial);
        let seen = format.text(initial, channels);
        let fields = std::array::from_fn(|index| {
            let field = Field::color_channel(cx);
            // The picker has its own light document, separate from the workspace palette.
            let mut palette = Palette::new("#000000", "#ffffff");
            palette.input_bg = gpui::white();
            palette.foreground = gpui::black();
            palette.accent = gpui::transparent_black();
            palette.input_border = gpui::transparent_black();
            field.update(cx, |field, cx| {
                field.apply_palette(palette, cx);
                field.set_text(seen[index].clone(), cx);
            });
            field
        });
        let well_focus = cx.focus_handle();
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
                cx.notify();
            }));
            observations.push(cx.on_blur(&focus, window, move |picker, _, cx| {
                picker.sync_fields(cx);
                cx.notify();
            }));
        }
        Self {
            anchor,
            initial,
            value: initial,
            hue: initial_position.0,
            hue_x: nearest_hue_x(initial_position.0),
            well: initial_position.1,
            format,
            channels,
            fields,
            seen,
            well_focus,
            focus_on_open: true,
            hue_focus: cx.focus_handle(),
            sampler_focus: cx.focus_handle(),
            sampling: None,
            #[cfg(not(windows))]
            sampling_epoch: 0,
            format_focus: cx.focus_handle(),
            drag: None,
            _observations: observations,
        }
    }

    fn edit(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some((_, _, max)) = self.format.fields().get(index) else {
            return;
        };
        let field = &self.fields[index];
        let mut text = field.read(cx).text.clone();
        if text == self.seen[index] {
            return;
        }
        if let Some((byte, _)) = text.char_indices().nth(*max) {
            text.truncate(byte);
            field.update(cx, |field, cx| field.set_text(text.clone(), cx));
        }
        self.seen[index] = text.clone();
        if let Some(rgb) = self.format.edit(&text, index, &mut self.channels)
            && rgb != self.value
        {
            self.move_to_color(rgb);
            self.value = rgb;
            cx.emit(Event::Color(rgb));
        }
        cx.notify();
    }

    fn sync_fields(&mut self, cx: &mut Context<Self>) {
        self.seen = self.format.text(self.value, self.channels);
        for (field, text) in self.fields.iter().zip(&self.seen) {
            field.update(cx, |field, cx| {
                if field.text != *text {
                    field.set_text(text.clone(), cx);
                }
            });
        }
    }

    fn select(&mut self, rgb: Rgb, cx: &mut Context<Self>) {
        if self.value != rgb {
            self.value = rgb;
            self.channels = self.format.channels(rgb);
            self.sync_fields(cx);
            cx.emit(Event::Color(rgb));
        }
        cx.notify();
    }

    fn popup(&self, viewport: Size<Pixels>) -> Bounds<Pixels> {
        let anchor = self.anchor.get().unwrap_or_default();
        let size = gpui::size(px(WIDTH), px(HEIGHT));
        let below = anchor.bottom();
        let above = anchor.top() - size.height;
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

    fn change_pointer(
        &mut self,
        drag: Drag,
        position: Point<Pixels>,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        let bounds = self.popup(window.viewport_size());
        let position = position - bounds.origin;
        match drag {
            Drag::Well => {
                self.well = (
                    snap(f32::from(position.x) - 1., WELL_WIDTH),
                    snap(f32::from(position.y) - 1., WELL_HEIGHT),
                )
            }
            Drag::Hue => self.move_hue(f32::from(position.x) - 88.),
        }
        self.select_well(cx);
    }

    fn move_to_color(&mut self, rgb: Rgb) {
        (self.hue, self.well) = position_of(rgb);
        self.hue_x = nearest_hue_x(self.hue);
    }

    fn move_hue(&mut self, x: f32) {
        self.hue_x = x.floor().clamp(0., SLIDER_WIDTH - 1.);
        self.hue = hue_at((self.hue_x + 0.5) / SLIDER_WIDTH);
    }

    fn select_well(&mut self, cx: &mut Context<Self>) {
        let [_, s, l] = color::hsl(well_rgb(self.hue, self.well.0, self.well.1));
        self.select(color::from_hsl([(self.hue.round() as u16) % 360, s, l]), cx);
    }

    fn switch(&mut self, previous: bool, cx: &mut Context<Self>) {
        self.format = self.format.cycle(previous);
        self.channels = self.format.channels(self.value);
        self.sync_fields(cx);
        cx.notify();
    }

    fn key(&mut self, event: &gpui::KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self
            .fields
            .iter()
            .any(|field| field.read(cx).is_composing())
        {
            return;
        }
        let key = event.keystroke.key.as_str();
        if self.sampling.is_some() {
            if key == "escape" {
                self.sampling = None;
                cx.notify();
            }
            window.prevent_default();
            cx.stop_propagation();
            return;
        }
        match key {
            "enter" if self.sampler_focus.is_focused(window) => self.begin_sampling(window, cx),
            "enter" => cx.emit(Event::Close),
            "escape" if self.value == self.initial => cx.emit(Event::Close),
            "escape" => {
                self.move_to_color(self.initial);
                self.select(self.initial, cx);
            }
            "tab" => {
                let mut handles = vec![
                    self.well_focus.clone(),
                    self.sampler_focus.clone(),
                    self.hue_focus.clone(),
                ];
                handles.extend(
                    self.fields
                        .iter()
                        .take(self.format.fields().len())
                        .map(|field| field.read(cx).edit_focus()),
                );
                handles.push(self.format_focus.clone());
                let current = handles
                    .iter()
                    .position(|handle| handle.is_focused(window))
                    .unwrap_or(0);
                let step = if event.keystroke.modifiers.shift {
                    handles.len() - 1
                } else {
                    1
                };
                handles[(current + step) % handles.len()].focus(window);
            }
            "up" | "down" | "space" if self.format_focus.is_focused(window) => {
                self.switch(key == "up", cx)
            }
            "up" | "down" | "left" | "right"
                if self.well_focus.is_focused(window) || self.hue_focus.is_focused(window) =>
            {
                let step = if matches!(key, "right" | "down") {
                    1.
                } else {
                    -1.
                };
                let multiplier = if event.keystroke.modifiers.control {
                    10.
                } else {
                    1.
                };
                if self.hue_focus.is_focused(window) {
                    self.move_hue(self.hue_x + step * multiplier);
                } else if matches!(key, "left" | "right") {
                    self.well.0 = snap(self.well.0 * WELL_WIDTH + step * multiplier, WELL_WIDTH);
                } else {
                    self.well.1 = snap(self.well.1 * WELL_HEIGHT + step * multiplier, WELL_HEIGHT);
                }
                self.select_well(cx);
            }
            _ => return,
        }
        window.prevent_default();
        cx.stop_propagation();
    }
}

fn snap(coordinate: f32, length: f32) -> f32 {
    (coordinate.floor().clamp(0., length - 1.) + 0.5) / length
}

pub(super) fn remember_anchor(anchor: Anchor) -> impl IntoElement {
    gpui::canvas(
        move |bounds, _, _| anchor.set(Some(bounds)),
        |_, _, _, _| {},
    )
    .absolute()
    .left(px(-1.))
    .right(px(-1.))
    .top(px(-1.))
    .bottom(px(-1.))
}
