pub(super) use super::geometry::{caret_bounds, index_at};
use super::{
    Field, buffer,
    geometry::PaintedLine,
    semantics,
    text::{base_font, shape},
};
use gpui::{
    Context, ElementInputHandler, IntoElement, canvas, div, fill, point, prelude::*, px, size,
};

fn alignment(align: gpui::TextAlign, width: f32, available: f32) -> f32 {
    let space = (available - width).max(0.);
    match align {
        gpui::TextAlign::Left => 0.,
        gpui::TextAlign::Center => space / 2.,
        gpui::TextAlign::Right => space,
    }
}
fn unavailable_document(binding: &semantics::Binding) -> crate::accessibility::SemanticNode {
    let mut node = crate::accessibility::SemanticNode::new(
        binding.id,
        Some(binding.parent),
        crate::accessibility::Role::Group,
        binding.name.clone(),
    );
    node.data.set_description(
        "Large-document text accessibility is unavailable; keyboard editing remains available.",
    );
    node
}
fn document(
    field: &Field,
    state: &semantics::TextState,
    virtualized: bool,
) -> Option<Result<crate::accessibility::SemanticNode, crate::accessibility::SemanticError>> {
    field.semantic.as_ref().map(|binding| {
        if virtualized {
            // This is NOT a partial TextPattern. Full large-document AT is a separate reviewed producer.
            let mut node = unavailable_document(binding);
            node.focus = Some(field.focus.clone());
            Ok(node)
        } else {
            binding.document(
                state,
                field.secret,
                field.multiline,
                &field.placeholder,
                field.focus.clone(),
            )
        }
    })
}

pub(super) fn render(field: &Field, cx: &Context<Field>) -> impl IntoElement {
    let entity = cx.entity();
    let snapshot = field.snapshot.clone();
    let state = semantics::capture(field);
    let virtualized = field.asset_editor
        && (snapshot.text.len() > shellfox_files::TEXT_BYTES || snapshot.line_count() > 4096);
    let mut semantic = field.semantic.clone();
    if !virtualized && semantic.is_some() && snapshot.line_count() > 4096 {
        if let Some(binding) = &semantic {
            binding
                .frame
                .reject(crate::accessibility::SemanticError::Limit);
        }
        semantic = None;
    }
    let document = document(field, &state, virtualized);
    let paint_semantic = if virtualized { None } else { semantic.clone() };
    let focus = field.focus.clone();
    let selection = field.selection.clone();
    let multiline = field.multiline;
    let directory = field.directory;
    let placeholder = field.placeholder.clone();
    let text_align = field.text_align;
    let palette = field.palette;
    let cursor = field.cursor;
    let initial_scroll = field.scroll_x;
    let line_height = field.line_height;
    let font_size = field.font_size;
    let top = if multiline { 8. } else { 3. };
    let monospace = field.monospace;
    let secret = field.secret;
    let code = field.code_editor;
    let height = if multiline {
        snapshot.line_count() as f32 * line_height + 16.
    } else {
        26.
    };
    let element = div()
        .w_full()
        .min_w_0()
        .when(!directory, |style| {
            style.px_2().bg(if multiline {
                palette.panel
            } else {
                palette.input_bg
            })
        })
        .when(!multiline && !directory, |style| {
            style
                .h(px(28.))
                .border_1()
                .border_color(palette.input_border)
                .rounded_sm()
        })
        .when(directory, |style| style.h(px(28.)))
        .when(multiline, |style| style.h(px(height)))
        .track_focus(&field.focus)
        .on_key_down(cx.listener(Field::key))
        .on_mouse_down(gpui::MouseButton::Left, cx.listener(Field::click))
        .on_mouse_move(cx.listener(Field::drag))
        .on_mouse_up(gpui::MouseButton::Left, cx.listener(Field::release))
        .on_mouse_up_out(gpui::MouseButton::Left, cx.listener(Field::release))
        .child(
            canvas(
                |_, _, _| (),
                move |bounds, _, window, cx| {
                    let font = base_font(monospace, font_size, code, window);
                    let row = snapshot.row(cursor);
                    let column = snapshot
                        .line(row)
                        .map_or(0, |range| cursor.saturating_sub(range.start));
                    let mut scroll = initial_scroll;
                    if focus.is_focused(window)
                        && let Some(range) = snapshot.window(row, column, virtualized)
                    {
                        let prefix = &snapshot.text[range.start..cursor.min(range.end)];
                        let x = f32::from(
                            shape(
                                &buffer::display_text(prefix, secret),
                                &font,
                                palette.foreground,
                                font_size,
                                window,
                            )
                            .width,
                        );
                        let width = f32::from(
                            shape(
                                &buffer::display_text(&snapshot.text[range], secret),
                                &font,
                                palette.foreground,
                                font_size,
                                window,
                            )
                            .width,
                        );
                        let visible = f32::from(bounds.size.width).max(1.);
                        if width <= visible {
                            scroll = 0.;
                        } else if x < scroll {
                            scroll = x;
                        } else if x + 3. > scroll + visible {
                            scroll = x + 3. - visible;
                        }
                    }
                    window.handle_input(
                        &focus,
                        ElementInputHandler::new(bounds, entity.clone()),
                        cx,
                    );
                    let clip = bounds.intersect(&window.content_mask().bounds);
                    let visible_rows = snapshot.visible_rows(
                        f32::from(bounds.top()),
                        top,
                        line_height,
                        f32::from(clip.top())..f32::from(clip.bottom()),
                    );
                    let rows = if paint_semantic.is_some() {
                        0..snapshot.line_count()
                    } else {
                        visible_rows.clone()
                    };
                    let mut semantic_runs = Vec::new();
                    let mut painted_lines = Vec::new();
                    for row in rows {
                        let Some(range) = snapshot.window(row, column, virtualized) else {
                            continue;
                        };
                        let source = &snapshot.text[range.clone()];
                        let display = if source.is_empty() && row == 0 && !placeholder.is_empty() {
                            placeholder.clone()
                        } else {
                            buffer::display_text(source, secret)
                        };
                        let foreground = if display == placeholder && !placeholder.is_empty() {
                            palette.placeholder
                        } else {
                            palette.foreground
                        };
                        let line = shape(&display, &font, foreground, font_size, window);
                        let align = alignment(
                            text_align,
                            f32::from(line.width),
                            f32::from(bounds.size.width),
                        );
                        let origin = point(
                            bounds.left() - px(scroll) + px(align),
                            bounds.top() + px(row as f32 * line_height + top),
                        );
                        if let Some(binding) = &paint_semantic {
                            let actual = buffer::display_text(source, secret);
                            let actual_line = if actual == display {
                                line.clone()
                            } else {
                                shape(&actual, &font, palette.foreground, font_size, window)
                            };
                            let has_newline = range.end < snapshot.text.len();
                            match binding.run(
                                row,
                                source,
                                has_newline,
                                secret,
                                semantics::MeasuredLine {
                                    line: &actual_line,
                                    origin,
                                    line_height,
                                },
                                window,
                            ) {
                                Ok((id, measured)) => semantic_runs.push((
                                    id,
                                    range.start + measured.start..range.start + measured.end,
                                )),
                                Err(error) => binding.frame.reject(error),
                            }
                        }
                        if !visible_rows.contains(&row) {
                            continue;
                        }
                        let caret_adjust = if source.is_empty() {
                            alignment(text_align, 0., f32::from(bounds.size.width)) - align
                        } else {
                            0.
                        };
                        if focus.is_focused(window)
                            && selection.start <= range.end
                            && selection.end >= range.start
                        {
                            let start = selection
                                .start
                                .saturating_sub(range.start)
                                .min(source.len());
                            let end = selection.end.saturating_sub(range.start).min(source.len());
                            let left =
                                line.x_for_index(buffer::display_index(source, start, secret));
                            let right =
                                line.x_for_index(buffer::display_index(source, end, secret));
                            window.paint_quad(fill(
                                gpui::Bounds::new(
                                    point(origin.x + left + px(caret_adjust), origin.y),
                                    size((right - left).max(px(1.)), px(line_height)),
                                ),
                                if start == end {
                                    palette.foreground
                                } else {
                                    palette.accent.opacity(0.35)
                                },
                            ));
                        }
                        if let Err(error) = line.paint(origin, px(line_height), window, cx) {
                            eprintln!("editor paint: {error}");
                        }
                        painted_lines.push(PaintedLine {
                            source: range,
                            line,
                            origin,
                            caret_adjust,
                        });
                    }
                    entity.update(cx, |field, cx| {
                        if !std::sync::Arc::ptr_eq(&field.snapshot.text, &snapshot.text) {
                            field.painted_lines.clear();
                            field.measured_semantics = None;
                            cx.notify();
                            return;
                        }
                        field.scroll_x = scroll;
                        field.painted_lines = painted_lines;
                        field.measured_semantics = paint_semantic
                            .as_ref()
                            .map(|_| semantics::measured(state, semantic_runs));
                    });
                },
            )
            .w_full()
            .h(px(height)),
        )
        .overflow_hidden();
    match (semantic, document) {
        (Some(binding), Some(Ok(node))) => binding.frame.wrap(node, element).into_any_element(),
        (Some(binding), Some(Err(error))) => {
            binding.frame.reject(error);
            element.into_any_element()
        }
        _ => element.into_any_element(),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn temporary_large_editor_coverage_does_not_advertise_a_partial_text_pattern() {
        use crate::accessibility::{Action, Role, SemanticFrame, SemanticId};
        let binding = super::semantics::Binding {
            frame: SemanticFrame::new(SemanticId(1)),
            id: SemanticId(2),
            parent: SemanticId(1),
            name: "File contents".into(),
            key: "field/test".into(),
        };
        let node = super::unavailable_document(&binding);
        assert_eq!(node.data.role(), Role::Group);
        assert!(node.data.value().is_none());
        assert!(node.data.text_selection().is_none());
        assert!(node.data.bounds().is_none());
        for action in [
            Action::SetValue,
            Action::ReplaceSelectedText,
            Action::SetTextSelection,
        ] {
            assert!(!node.data.supports_action(action));
        }
        assert!(
            node.data
                .description()
                .is_some_and(|description| description.contains("unavailable"))
        );
    }
}
