use super::cursor;
use crate::app::TerminalView;
use gpui::{
    Bounds, ElementInputHandler, Entity, FocusHandle, IntoElement, Pixels, canvas, fill, point,
    prelude::*, px, rgb, size,
};

pub fn surface(view: Entity<TerminalView>, focus: FocusHandle) -> impl IntoElement {
    let layout_view = view.clone();
    canvas(
        move |bounds, window, cx| {
            let state = layout_view.update(cx, |view, cx| {
                view.set_terminal_bounds(bounds, cx);
                (
                    view.screen.clone(),
                    view.font.clone(),
                    view.cell_width,
                    view.font_size,
                    view.line_height,
                    view.theme,
                    view.presentation_frame(),
                    view.cursor_reset_epoch,
                )
            });
            let phase = state
                .6
                .as_ref()
                .is_none_or(|frame| cursor::phase(frame, state.7, window, cx));
            (state, phase)
        },
        move |bounds: Bounds<Pixels>,
              (
            (screen, base_font, cell_width, font_size, line_height, theme, presentation, _),
            phase,
        ),
              window,
              cx| {
            let drag_view = view.clone();
            window.on_mouse_event(move |event: &gpui::MouseMoveEvent, phase, window, cx| {
                if phase == gpui::DispatchPhase::Capture && !bounds.contains(&event.position) {
                    drag_view.update(cx, |view, cx| view.pointer_move_outside(event, window, cx));
                }
            });
            window.handle_input(&focus, ElementInputHandler::new(bounds, view.clone()), cx);
            window.paint_quad(fill(bounds, rgb(theme.background)));
            let cursor = presentation.as_ref().and_then(|frame| {
                cursor::CursorPaint::new(frame.cursor_visual, screen.cols, screen.rows, phase)
            });
            let painted = super::paint_cells(
                &screen,
                bounds,
                super::CellStyle {
                    font: &base_font,
                    cell_width,
                    font_size,
                    line_height,
                    theme,
                },
                super::CellOverlay {
                    row: |row, window: &mut gpui::Window| {
                        if let Some(cursor) = cursor.filter(|cursor| {
                            usize::from(cursor.row) == row
                                && cursor.shape == shellfox_runtime::CursorShape::Block
                        }) {
                            cursor.paint(
                                bounds,
                                cell_width,
                                line_height,
                                rgb(theme.background).into(),
                                window,
                            );
                        }
                    },
                    color: |col, row| {
                        cursor
                            .filter(|cursor| cursor.covers_glyph(col, row))
                            .map(|cursor| cursor.glyph_color(rgb(theme.background).into()))
                    },
                },
                window,
                cx,
            );
            if let Err(error) = painted {
                view.update(cx, |view, cx| {
                    view.fail(format!("terminal glyph paint: {error}"));
                    cx.notify();
                });
                return;
            }
            if let Some(cursor) =
                cursor.filter(|cursor| cursor.shape != shellfox_runtime::CursorShape::Block)
            {
                cursor.paint(
                    bounds,
                    cell_width,
                    line_height,
                    rgb(theme.background).into(),
                    window,
                );
            }
            view.update(cx, |view, _| {
                view.paint_semantics(presentation.as_deref(), bounds, window)
            });
            // Fixture snapshots have no service presentation metadata. Never use this for a live frame.
            if presentation.is_none()
                && screen.cursor.visible
                && screen.cursor.x < screen.cols
                && screen.cursor.y < screen.rows
            {
                window.paint_quad(fill(
                    Bounds::new(
                        bounds.origin
                            + point(
                                px(f32::from(screen.cursor.x) * cell_width),
                                px(f32::from(screen.cursor.y) * line_height),
                            ),
                        size(px(2.), px(line_height)),
                    ),
                    rgb(theme.accent),
                ));
            }
        },
    )
    .w_full()
    .min_h_0()
    .flex_1()
}
