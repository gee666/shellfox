pub(super) use super::geometry::{caret_bounds, index_at};
use super::{
    Field, buffer,
    geometry::PaintedLine,
    semantics,
    text::{base_font, shape},
};
use gpui::{Context, ElementInputHandler, IntoElement, canvas, point, prelude::*, px};
#[path = "paint_container.rs"]
mod container;

#[path = "paint_layout.rs"]
mod layout;
use layout::alignment;
#[path = "paint_document.rs"]
mod document;
#[path = "placeholder.rs"]
mod placeholder;

pub(super) fn render(
    field: &Field,
    window: &gpui::Window,
    cx: &Context<Field>,
) -> impl IntoElement {
    let entity = cx.entity();
    let snapshot = field.snapshot.clone();
    let syntax = field.syntax_spans.clone();
    let state = semantics::capture(field);
    let wrap = field.wrap_layout.clone();
    let nowrap = field.nowrap.clone();
    let asset_editor = field.asset_editor;
    let content_width = nowrap
        .as_ref()
        .map(|layout| (layout.width + 16.).max(field.file_viewport.unwrap_or(0.)));
    let row_count = wrap
        .as_ref()
        .map_or(snapshot.line_count(), |wrap| wrap.count(&snapshot));
    let virtualized = field.asset_editor
        && (snapshot.text.len() > shellfox_files::TEXT_BYTES || row_count > 4096);
    let mut semantic = field.semantic.clone();
    if !virtualized && semantic.is_some() && snapshot.line_count() > 4096 {
        if let Some(binding) = &semantic {
            binding
                .frame
                .reject(crate::accessibility::SemanticError::Limit);
        }
        semantic = None;
    }
    let document = document::document(field, &state, virtualized);
    let paint_semantic = if virtualized { None } else { semantic.clone() };
    let focus = field.focus.clone();
    let selection = field.selection.clone();
    let multiline = field.multiline;
    let session_title = field.session_title;
    let settings_input = field.settings_input;
    let placeholder = field.placeholder.clone();
    let text_align = field.text_align;
    let palette = field.palette;
    let cursor = field.cursor;
    let initial_scroll = field.scroll_x;
    let line_height = field.line_height;
    let font_size = field.font_size;
    let preferred_font = field.preferred_font.clone();
    let top = if multiline {
        field.multiline_text_inset()
    } else if session_title {
        1.
    } else {
        3.
    };
    let monospace = field.monospace;
    let secret = field.secret;
    let code = field.code_editor;
    let height = if multiline {
        let hint_rows = if snapshot.text.is_empty() {
            placeholder.lines().count()
        } else {
            0
        };
        row_count.max(hint_rows) as f32 * line_height + 16.
    } else {
        26.
    };
    let element = container::container(field, content_width, height, window, cx)
        .child(
            canvas(
                |_, _, _| (),
                move |bounds, _, window, cx| {
                    let font = preferred_font
                        .clone()
                        .map(gpui::font)
                        .unwrap_or_else(|| base_font(monospace, font_size, code, window));
                    let paint_style =
                        placeholder::Style::new(&font, palette.foreground, font_size, line_height);
                    let row = snapshot.row(cursor);
                    let caret_row = wrap
                        .as_ref()
                        .map_or(row, |wrap| wrap.row(&snapshot, cursor));
                    let column = snapshot
                        .line(row)
                        .map_or(0, |range| cursor.saturating_sub(range.start));
                    let scroll = if nowrap.is_some() {
                        0.
                    } else if wrap.is_none()
                        && focus.is_focused(window)
                        && let Some(range) = snapshot.window(row, column, virtualized)
                    {
                        let caret = cursor.min(range.end).saturating_sub(range.start);
                        layout::horizontal_scroll(
                            &snapshot.text[range],
                            caret,
                            secret,
                            &paint_style,
                            f32::from(bounds.size.width),
                            initial_scroll,
                            window,
                        )
                    } else {
                        initial_scroll
                    };
                    window.handle_input(
                        &focus,
                        ElementInputHandler::new(bounds, entity.clone()),
                        cx,
                    );
                    let clip = bounds.intersect(&window.content_mask().bounds);
                    let visible_x = f32::from(clip.left() - bounds.left())
                        ..f32::from(clip.right() - bounds.left());
                    let clip = f32::from(clip.top())..f32::from(clip.bottom());
                    let visible_rows = match &wrap {
                        Some(wrap) => wrap.visible_rows(
                            &snapshot,
                            f32::from(bounds.top()) + top,
                            line_height,
                            clip,
                        ),
                        None => {
                            snapshot.visible_rows(f32::from(bounds.top()), top, line_height, clip)
                        }
                    };
                    let rows = if paint_semantic.is_some() {
                        0..row_count
                    } else {
                        visible_rows.clone()
                    };
                    let mut semantic_runs = Vec::new();
                    let mut painted_lines = Vec::new();
                    for row in rows {
                        let slice = match (&wrap, &nowrap) {
                            (Some(wrap), _) => wrap.line(&snapshot, row).map(default_slice),
                            (_, Some(layout)) if paint_semantic.is_none() => snapshot
                                .line(row)
                                .map(|range| layout.slice(range, visible_x.clone())),
                            _ => snapshot.window(row, column, virtualized).map(default_slice),
                        };
                        let Some(slice) = slice else {
                            continue;
                        };
                        let range = slice.range;
                        let source = &snapshot.text[range.clone()];
                        let show_hint =
                            snapshot.text.is_empty() && row == 0 && !placeholder.is_empty();
                        let (display, projection) = if show_hint {
                            (placeholder::first(&placeholder).to_owned(), None)
                        } else if asset_editor {
                            let (text, projection, _) =
                                super::editor_line::Projection::build(source, slice.column);
                            (text, Some(projection))
                        } else {
                            (buffer::display_text(source, secret), None)
                        };
                        let foreground = if show_hint {
                            palette.placeholder
                        } else {
                            palette.foreground
                        };
                        let line = if syntax.is_empty() || display.is_empty() {
                            shape(&display, &font, foreground, font_size, window)
                        } else {
                            let runs = super::syntax::line_runs(
                                &syntax,
                                range.clone(),
                                &font,
                                palette,
                                projection.as_ref(),
                            );
                            window.text_system().shape_line(
                                display.clone().into(),
                                px(font_size),
                                &runs,
                                None,
                            )
                        };
                        let align = alignment(
                            text_align,
                            f32::from(line.width),
                            f32::from(bounds.size.width),
                        );
                        let origin = point(
                            bounds.left() + px(slice.left - scroll + align),
                            bounds.top() + px(row as f32 * line_height + top),
                        );
                        if let Some(binding) = &paint_semantic {
                            let actual = buffer::display_text(source, secret);
                            let actual_line = if actual == display {
                                line.clone()
                            } else {
                                shape(&actual, &font, palette.foreground, font_size, window)
                            };
                            let has_newline =
                                snapshot.text.as_bytes().get(range.end) == Some(&b'\n');
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
                            && if selection.is_empty() {
                                row == caret_row && (range.start..=range.end).contains(&cursor)
                            } else {
                                selection.start < range.end && selection.end > range.start
                            }
                        {
                            layout::paint_selection(
                                &line,
                                source,
                                range.start,
                                &selection,
                                layout::SelectionStyle {
                                    origin,
                                    height: line_height,
                                    caret_adjust,
                                    secret,
                                    projection: projection.as_ref(),
                                    palette,
                                },
                                window,
                            );
                        }
                        if let Err(error) = line.paint(origin, px(line_height), window, cx) {
                            eprintln!("editor paint: {error}");
                        }
                        if show_hint && multiline {
                            placeholder::paint_tail(
                                &placeholder,
                                paint_style.color(foreground),
                                origin,
                                window,
                                cx,
                            );
                        }
                        painted_lines.push(PaintedLine {
                            source: range,
                            line,
                            origin,
                            caret_adjust,
                            projection,
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
        .when(settings_input, |element| {
            element.focus(|style| style.border_color(palette.accent))
        })
        .overflow_hidden();
    document::wrap(semantic, document, element)
}

fn default_slice(range: std::ops::Range<usize>) -> super::horizontal::Slice {
    super::horizontal::Slice {
        range,
        left: 0.,
        column: 0,
    }
}

#[cfg(test)]
#[path = "paint_tests.rs"]
mod tests;
