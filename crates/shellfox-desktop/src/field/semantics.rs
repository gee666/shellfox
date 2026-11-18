use super::{Field, buffer};
use crate::accessibility::{
    Action, Role, SemanticAction, SemanticActionKind, SemanticError, SemanticFrame, SemanticId,
    SemanticNode, TextSelection,
};
use accesskit::{Rect, TextPosition};
use gpui::{Context, Pixels, ShapedLine, Window};
use std::{ops::Range, sync::Arc};

#[derive(Clone)]
pub(super) struct Binding {
    pub frame: SemanticFrame,
    pub id: SemanticId,
    pub parent: SemanticId,
    pub name: String,
    pub key: String,
}
#[derive(Clone)]
pub(super) struct TextState {
    pub text: Arc<str>,
    pub anchor: usize,
    pub cursor: usize,
    pub read_only: bool,
}
pub(super) struct Measured {
    state: TextState,
    runs: Vec<(SemanticId, Range<usize>)>,
}
/// The shaped line and placement used together by the current paint callback.
pub(super) struct MeasuredLine<'a> {
    pub line: &'a ShapedLine,
    pub origin: gpui::Point<Pixels>,
    pub line_height: f32,
}
impl Measured {
    fn offset(&self, point: &TextPosition) -> Option<usize> {
        let (_, range) = self.runs.iter().find(|(id, _)| *id == point.node)?;
        let text = self.state.text.get(range.clone())?;
        let offset = text.char_indices().nth(point.character_index).map_or_else(
            || (point.character_index == text.chars().count()).then_some(text.len()),
            |(index, _)| Some(index),
        )?;
        Some(range.start + offset)
    }
}
impl TextState {
    fn matches(&self, field: &Field) -> bool {
        self.text.as_ref() == field.text.as_str()
            && self.anchor == field.anchor
            && self.cursor == field.cursor
            && self.read_only == field.read_only
    }
}

impl Field {
    pub(crate) fn set_frame(
        &mut self,
        frame: SemanticFrame,
        parent: SemanticId,
        name: impl Into<String>,
        cx: &mut Context<Self>,
    ) -> Result<SemanticId, SemanticError> {
        let name = name.into();
        let key = format!("field/{:?}", cx.entity_id());
        let id = frame.id_for(&key)?;
        if let Some(old) = &self.semantic {
            if !old.frame.same_context(&frame) {
                return Err(SemanticError::FrameWindow);
            }
            if old.parent == parent && old.name == name {
                return Ok(id);
            }
        }
        self.semantic = Some(Binding {
            frame,
            id,
            parent,
            name,
            key,
        });
        self.measured_semantics = None;
        cx.notify();
        Ok(id)
    }
    pub(crate) fn semantic_id(&self) -> Option<SemanticId> {
        self.semantic.as_ref().map(|binding| binding.id)
    }
    pub(crate) fn handle_semantic_action(
        &mut self,
        event: &SemanticAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.semantic_id() != Some(event.target) {
            return false;
        }
        if matches!(event.kind, SemanticActionKind::Focus) {
            self.focus(window);
            cx.notify();
            return true;
        }
        let Some(measured) = &self.measured_semantics else {
            return false;
        };
        if !measured.state.matches(self) {
            cx.notify();
            return false;
        }
        match &event.kind {
            SemanticActionKind::SetValue(text) => {
                let end = self.text.encode_utf16().count();
                if self.replace(Some(0..end), text).is_none() {
                    return false;
                }
            }
            SemanticActionKind::ReplaceSelectedText(text) => {
                if self.replace(None, text).is_none() {
                    return false;
                }
            }
            SemanticActionKind::SetNumericValue(value) if value.is_finite() => {
                let end = self.text.encode_utf16().count();
                if self.replace(Some(0..end), &value.to_string()).is_none() {
                    return false;
                }
            }
            SemanticActionKind::SetTextSelection(selection) => {
                let (Some(anchor), Some(cursor)) = (
                    measured.offset(&selection.anchor),
                    measured.offset(&selection.focus),
                ) else {
                    return false;
                };
                self.anchor = anchor;
                self.set_cursor(cursor, true);
            }
            SemanticActionKind::ScrollIntoView => self.ensure_caret_visible(),
            _ => return false,
        }
        self.measured_semantics = None;
        cx.notify();
        true
    }
}
impl Binding {
    fn run_id(&self, row: usize) -> Result<SemanticId, SemanticError> {
        self.frame.id_for(&format!("{}/line/{row}", self.key))
    }
    pub(super) fn document(
        &self,
        state: &TextState,
        secret: bool,
        multiline: bool,
        placeholder: &str,
        focus: gpui::FocusHandle,
    ) -> Result<SemanticNode, SemanticError> {
        let mut node = SemanticNode::new(
            self.id,
            Some(self.parent),
            if secret {
                Role::PasswordInput
            } else if multiline {
                Role::MultilineTextInput
            } else {
                Role::TextInput
            },
            self.name.clone(),
        );
        node.focus = Some(focus);
        node.data
            .set_value(buffer::display_text(&state.text, secret));
        if !placeholder.is_empty() {
            node.data.set_placeholder(placeholder.to_owned());
        }
        if state.read_only {
            node.data.set_read_only();
        } else {
            node.data.add_action(Action::SetValue);
            node.data.add_action(Action::ReplaceSelectedText);
        }
        node.data.add_action(Action::SetTextSelection);
        node.data.add_action(Action::ScrollIntoView);
        let point = |offset: usize| -> Result<TextPosition, SemanticError> {
            let prefix = &state.text[..offset];
            let row = prefix.bytes().filter(|byte| *byte == b'\n').count();
            let start = prefix.rfind('\n').map_or(0, |index| index + 1);
            Ok(TextPosition {
                node: self.run_id(row)?,
                character_index: state.text[start..offset].chars().count(),
            })
        };
        node.data.set_text_selection(TextSelection {
            anchor: point(state.anchor)?,
            focus: point(state.cursor)?,
        });
        Ok(node)
    }
    pub(super) fn run(
        &self,
        row: usize,
        source: &str,
        has_newline: bool,
        secret: bool,
        measured: MeasuredLine<'_>,
        window: &Window,
    ) -> Result<(SemanticId, Range<usize>), SemanticError> {
        let MeasuredLine {
            line,
            origin,
            line_height,
        } = measured;
        let id = self.run_id(row)?;
        let mut data = crate::accessibility::Node::new(Role::TextRun);
        let mut text = buffer::display_text(source, secret);
        if has_newline {
            text.push('\n');
        }
        data.set_value(text.clone());
        data.set_character_lengths(
            text.chars()
                .map(|ch| ch.len_utf8() as u8)
                .collect::<Vec<_>>(),
        );
        let scale = if cfg!(target_os = "macos") {
            1.
        } else {
            window.scale_factor()
        };
        let mut positions = Vec::new();
        let mut widths = Vec::new();
        for (index, ch) in text.char_indices() {
            let next = index + ch.len_utf8();
            let left = line.x_for_index(index.min(line.len()));
            let right = line.x_for_index(next.min(line.len()));
            positions.push(f32::from(left) * scale);
            widths.push(f32::from(right - left).max(0.) * scale);
        }
        data.set_character_positions(positions);
        data.set_character_widths(widths);
        data.set_bounds(Rect::new(
            f64::from(f32::from(origin.x) * scale),
            f64::from(f32::from(origin.y) * scale),
            f64::from(f32::from(origin.x + line.width) * scale),
            f64::from((f32::from(origin.y) + line_height) * scale),
        ));
        self.frame.insert_measured(
            SemanticNode {
                id,
                parent: Some(self.id),
                data,
                focus: None,
            },
            window,
        );
        Ok((id, 0..source.len() + usize::from(has_newline)))
    }
}
pub(super) fn capture(field: &Field) -> TextState {
    TextState {
        text: field.snapshot_text(),
        anchor: field.anchor,
        cursor: field.cursor,
        read_only: field.read_only,
    }
}
pub(super) fn measured(state: TextState, runs: Vec<(SemanticId, Range<usize>)>) -> Measured {
    Measured { state, runs }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn text_range_units_map_to_utf8_without_splitting_surrogates() {
        let measured = Measured {
            state: TextState {
                text: "ab😀\n界".into(),
                anchor: 0,
                cursor: 0,
                read_only: false,
            },
            runs: vec![(SemanticId(2), 0..7), (SemanticId(3), 7..10)],
        };
        for (run, index, expected) in [(2, 0, 0), (2, 2, 2), (2, 3, 6), (2, 4, 7), (3, 1, 10)] {
            assert_eq!(
                measured.offset(&TextPosition {
                    node: SemanticId(run),
                    character_index: index
                }),
                Some(expected)
            );
        }
        assert_eq!(
            measured.offset(&TextPosition {
                node: SemanticId(2),
                character_index: 99
            }),
            None
        );
        assert_eq!(
            measured.offset(&TextPosition {
                node: SemanticId(9),
                character_index: 0
            }),
            None
        );
    }
    #[test]
    fn field_run_ids_keep_the_same_window_collector() {
        let frame = SemanticFrame::new(SemanticId(1));
        let binding = Binding {
            frame: frame.clone(),
            id: frame.id_for("field/42").unwrap(),
            parent: SemanticId(1),
            name: "File contents".into(),
            key: "field/42".into(),
        };
        let first = binding.run_id(0).unwrap();
        let _ = frame.begin_frame();
        assert_eq!(binding.run_id(0).unwrap(), first);
        assert_ne!(binding.run_id(1).unwrap(), first);
        assert!(frame.same_context(&binding.frame));
    }
}
