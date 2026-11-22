use crate::theme::Palette;
use gpui::{
    AnyElement, Bounds, DispatchPhase, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    Pixels, Point, ScrollHandle, Window, canvas, fill, point, prelude::*, px, rgb, size,
};
use std::{cell::Cell, rc::Rc};

const THICKNESS: f32 = 15.;

#[derive(Default)]
pub(crate) struct Scrollbar(Rc<Cell<Option<f32>>>);
impl Scrollbar {
    pub fn render(&self, scroll: &ScrollHandle, palette: Palette) -> AnyElement {
        self.render_axis(scroll, palette, Axis::Vertical, false)
    }
    pub fn horizontal(&self, scroll: &ScrollHandle, palette: Palette) -> AnyElement {
        self.render_axis(scroll, palette, Axis::Horizontal, false)
    }
    pub(crate) fn thin(&self, scroll: &ScrollHandle, palette: Palette) -> AnyElement {
        self.render_axis(scroll, palette, Axis::Vertical, true)
    }
    fn render_axis(
        &self,
        scroll: &ScrollHandle,
        palette: Palette,
        axis: Axis,
        thin: bool,
    ) -> AnyElement {
        let thickness = if thin { 10. } else { THICKNESS };
        let drag = self.0.clone();
        let scroll = scroll.clone();
        canvas(
            |_, _, _| (),
            move |bounds, _, window, _| paint(bounds, window, &scroll, palette, &drag, axis, thin),
        )
        .absolute()
        .bottom_0()
        .right_0()
        .when(matches!(axis, Axis::Horizontal), |bar| {
            bar.left_0().h(px(thickness))
        })
        .when(matches!(axis, Axis::Vertical), |bar| {
            bar.top_0().w(px(thickness))
        })
        .into_any_element()
    }
}

#[derive(Clone, Copy)]
enum Axis {
    Horizontal,
    Vertical,
}
impl Axis {
    fn component(self, x: Pixels, y: Pixels) -> f32 {
        f32::from(match self {
            Self::Horizontal => x,
            Self::Vertical => y,
        })
    }
    fn point(self, along: f32, cross: f32) -> Point<Pixels> {
        match self {
            Self::Horizontal => point(px(along), px(cross)),
            Self::Vertical => point(px(cross), px(along)),
        }
    }
    fn position(self, local: Point<Pixels>) -> f32 {
        self.component(local.x, local.y)
    }
    fn rect(
        self,
        origin: Point<Pixels>,
        along: f32,
        cross: f32,
        len: f32,
        height: f32,
    ) -> Bounds<Pixels> {
        let dimensions = self.point(len, height);
        Bounds::new(
            origin + self.point(along, cross),
            size(dimensions.x, dimensions.y),
        )
    }
    fn move_to(self, scroll: &ScrollHandle, offset: f32, max_scroll: f32) {
        let next = px(-offset.clamp(0., max_scroll));
        let current = scroll.offset();
        scroll.set_offset(match self {
            Self::Horizontal => point(next, current.y),
            Self::Vertical => point(current.x, next),
        });
    }
}

#[derive(Clone, Copy)]
struct Geometry {
    view: f32,
    max_scroll: f32,
    track: f32,
    thumb: f32,
    top: f32,
    gap: f32,
    thickness: f32,
}
impl Geometry {
    fn new(view: f32, max_scroll: f32, offset: f32, thin: bool) -> Option<Self> {
        let gap = if thin { 10. } else { 18. };
        if max_scroll <= 0. || view <= 2. * gap + 1. {
            return None;
        }
        let track = view - 2. * gap;
        let thumb = (track * view / (view + max_scroll))
            .round()
            .clamp(17_f32.min(track), track);
        let top = gap + (offset.clamp(0., max_scroll) / max_scroll * (track - thumb)).round();
        Some(Self {
            view,
            max_scroll,
            track,
            thumb,
            top,
            gap,
            thickness: if thin { 10. } else { THICKNESS },
        })
    }
    fn offset(self, top: f32) -> f32 {
        ((top - self.gap) / (self.track - self.thumb).max(1.) * self.max_scroll)
            .clamp(0., self.max_scroll)
    }
}

fn paint(
    bounds: Bounds<Pixels>,
    window: &mut Window,
    scroll: &ScrollHandle,
    palette: Palette,
    drag: &Rc<Cell<Option<f32>>>,
    axis: Axis,
    thin: bool,
) {
    let thickness = if thin { 10. } else { THICKNESS };
    let length = axis.component(bounds.size.width, bounds.size.height);
    let maximum = scroll.max_offset();
    let corner = axis.component(maximum.height, maximum.width) > 0.;
    let position = scroll.offset();
    let Some(bar) = Geometry::new(
        length - if corner { thickness } else { 0. },
        axis.component(maximum.width, maximum.height),
        -axis.position(position),
        thin,
    ) else {
        return;
    };
    let active = axis.rect(bounds.origin, 0., 0., bar.view, thickness);
    let track = rgb(if palette.dark { 0x2c2c2c } else { 0xf1f1f1 });
    let hover = window.mouse_position();
    let local = axis.position(hover - bounds.origin);
    let hovered_thumb = active.contains(&hover) && (bar.top..=bar.top + bar.thumb).contains(&local);
    let color = rgb(
        match (palette.dark, drag.get().is_some() || hovered_thumb) {
            (true, true) => 0xbdbdbd,
            (true, false) => 0x9f9f9f,
            (false, true) => 0xa8a8a8,
            (false, false) => 0xc1c1c1,
        },
    );
    if !thin {
        window.paint_quad(fill(bounds, track));
    }
    let thumb = axis.rect(
        bounds.origin,
        bar.top,
        if thin { 2. } else { 3. },
        bar.thumb,
        if thin { 6. } else { 9. },
    );
    let thumb_color: gpui::Hsla = if thin { palette.border } else { color.into() };
    window.paint_quad(gpui::quad(
        thumb,
        px(if thin { 3. } else { 4.5 }),
        thumb_color,
        px(0.),
        gpui::transparent_black(),
        gpui::BorderStyle::default(),
    ));
    let arrow_offset = if thin { 5. } else { 8. };
    let arrow_color = if thin { palette.border } else { color.into() };
    triangle(
        window,
        bounds.origin + axis.point(arrow_offset, thickness / 2.),
        true,
        arrow_color,
        axis,
        thin,
    );
    triangle(
        window,
        bounds.origin + axis.point(bar.view - arrow_offset, thickness / 2.),
        false,
        arrow_color,
        axis,
        thin,
    );
    attach_input(window, active, scroll, bar, axis, drag, hover);
}

fn attach_input(
    window: &mut Window,
    bounds: Bounds<Pixels>,
    scroll: &ScrollHandle,
    bar: Geometry,
    axis: Axis,
    drag: &Rc<Cell<Option<f32>>>,
    hover: Point<Pixels>,
) {
    let down_scroll = scroll.clone();
    let down_drag = drag.clone();
    window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
        if phase != DispatchPhase::Capture
            || event.button != MouseButton::Left
            || !bounds.contains(&event.position)
        {
            return;
        }
        let point = axis.position(event.position - bounds.origin);
        let start = -axis.position(down_scroll.offset());
        let target = if point < bar.thickness {
            start - 40.
        } else if point >= bar.view - bar.thickness {
            start + 40.
        } else if (bar.top..=bar.top + bar.thumb).contains(&point) {
            down_drag.set(Some(point - bar.top));
            start
        } else if point < bar.top {
            start - bar.view
        } else {
            start + bar.view
        };
        axis.move_to(&down_scroll, target, bar.max_scroll);
        window.refresh();
        cx.stop_propagation();
    });
    let move_scroll = scroll.clone();
    let move_drag = drag.clone();
    window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
        if phase != DispatchPhase::Capture {
            return;
        }
        if let Some(grab) = move_drag.get() {
            if event.pressed_button != Some(MouseButton::Left) {
                move_drag.set(None);
            } else {
                let target = bar.offset(axis.position(event.position - bounds.origin) - grab);
                axis.move_to(&move_scroll, target, bar.max_scroll);
                cx.stop_propagation();
            }
            window.refresh();
        } else if bounds.contains(&event.position) || bounds.contains(&hover) {
            window.refresh();
        }
    });
    let up_drag = drag.clone();
    window.on_mouse_event(move |event: &MouseUpEvent, phase, window, cx| {
        if phase == DispatchPhase::Capture
            && event.button == MouseButton::Left
            && up_drag.take().is_some()
        {
            window.refresh();
            cx.stop_propagation();
        }
    });
}

fn triangle(
    window: &mut Window,
    center: Point<Pixels>,
    beginning: bool,
    color: gpui::Hsla,
    axis: Axis,
    thin: bool,
) {
    let direction = if beginning { 1. } else { -1. };
    let mut path = gpui::PathBuilder::fill();
    let scale = if thin { 0.65 } else { 1. };
    path.move_to(center + axis.point(-3. * direction * scale, 0.));
    path.line_to(center + axis.point(3. * direction * scale, 4. * scale));
    path.line_to(center + axis.point(3. * direction * scale, -4. * scale));
    path.close();
    if let Ok(path) = path.build() {
        window.paint_path(path, color);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn thumb_travel_maps_both_document_extents() {
        let first = Geometry::new(220., 400., 0., false).unwrap();
        let last = Geometry::new(220., 400., 400., false).unwrap();
        assert_eq!(first.offset(first.top), 0.);
        assert_eq!(last.offset(last.top), 400.);
        assert!(Geometry::new(220., 0., 0., false).is_none());
    }
}
