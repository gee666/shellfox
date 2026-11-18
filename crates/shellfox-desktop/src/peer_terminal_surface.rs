use super::*;
use gpui::{ElementInputHandler, IntoElement, canvas, fill, point, rgb, size};
pub(super) fn surface(view: Entity<PeerTerminalView>, focus: FocusHandle) -> impl IntoElement {
    let layout = view.clone();
    canvas(
        move |bounds, _, cx| {
            layout.update(cx, |view, _| {
                view.set_bounds(bounds);
                (
                    view.screen.clone(),
                    view.font.clone(),
                    view.cell_width,
                    view.font_size,
                    view.line_height,
                    view.theme,
                )
            })
        },
        move |bounds: Bounds<Pixels>,
              (screen, font, cell_width, font_size, line_height, theme),
              window,
              cx| {
            window.handle_input(&focus, ElementInputHandler::new(bounds, view.clone()), cx);
            window.paint_quad(fill(bounds, rgb(theme.background)));
            let Some(screen) = screen else {
                return;
            };
            let result = renderer::paint_cells(
                &screen,
                bounds,
                renderer::CellStyle {
                    font: &font,
                    cell_width,
                    font_size,
                    line_height,
                    theme,
                },
                renderer::CellOverlay {
                    row: |_, _: &mut Window| {},
                    color: |_, _| None,
                },
                window,
                cx,
            );
            if let Err(error) = result {
                view.update(cx, |view, cx| {
                    view.error = Some(format!("Peer cell paint: {error:#}"));
                    cx.notify();
                });
                return;
            }
            // This is the actual native owner's cursor position/visibility. No
            // local Runtime presentation identity or style lease is fabricated.
            if screen.cursor.visible
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
    .flex_1()
    .min_h_0()
}
