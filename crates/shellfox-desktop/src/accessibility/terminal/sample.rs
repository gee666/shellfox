use super::*;
use accesskit::TextPosition;
use shellfox_runtime::{HistoryPoint, PresentationSelection, RowMetadata};

pub(super) enum SelectionInput<'a> {
    #[cfg(test)]
    Fixture(Option<TerminalSelection>),
    View {
        selection: Option<&'a PresentationSelection>,
        top_row: u64,
    },
}

impl Row {
    fn character_at(&self, column: u16) -> Option<usize> {
        self.columns
            .iter()
            .zip(&self.widths)
            .position(|(start, width)| {
                *width > 0. && column >= *start && f32::from(column - *start) < *width
            })
    }
}

impl TerminalDocument {
    pub(super) fn sample(
        &mut self,
        screen: &Screen,
        metadata: Option<&[RowMetadata]>,
        cursor: Option<TerminalPoint>,
        input: SelectionInput<'_>,
    ) -> Result<(), SemanticError> {
        let mut rows = Vec::with_capacity(usize::from(screen.rows));
        let mut bytes = 0usize;
        for (index, cells) in screen
            .cells
            .chunks_exact(usize::from(screen.cols))
            .enumerate()
        {
            let meta = metadata.map(|rows| rows[index]).unwrap_or(RowMetadata {
                soft_wrapped: false,
                wrap_continuation: false,
            });
            let mut row = Row {
                text: String::new(),
                lengths: Vec::new(),
                columns: Vec::new(),
                widths: Vec::new(),
                end_column: screen.cols,
            };
            for (col, cell) in cells.iter().enumerate() {
                if cell.width == 0 {
                    continue;
                }
                let text = if cell.invisible || cell.text.is_empty() {
                    " "
                } else {
                    &cell.text
                };
                if text.len() > usize::from(u8::MAX)
                    || text.chars().any(char::is_control)
                    || cell.width > 2
                    || col + usize::from(cell.width) > cells.len()
                {
                    return Err(SemanticError::InvalidText(self.id));
                }
                // One entry per actual VT cell cluster. Never re-cluster across cells.
                row.text.push_str(text);
                row.lengths.push(text.len() as u8);
                row.columns.push(col as u16);
                row.widths.push(f32::from(cell.width));
            }
            if !meta.soft_wrapped {
                row.text.push('\n');
                row.lengths.push(1);
                row.columns.push(screen.cols);
                row.widths.push(0.);
            }
            bytes += row.text.len();
            if bytes > MAX_BYTES {
                return Err(SemanticError::Limit);
            }
            rows.push(row);
        }
        let boundary = |point: TerminalPoint| -> Option<TextPosition> {
            let row = rows.get(usize::from(point.row))?;
            let index = if point.column == row.end_column {
                row.columns
                    .iter()
                    .position(|col| *col == row.end_column)
                    .unwrap_or(row.lengths.len())
            } else {
                row.character_at(point.column)?
            };
            Some(TextPosition {
                node: self.rows[usize::from(point.row)],
                character_index: index,
            })
        };
        let caret = cursor.and_then(boundary).map(|caret| TextSelection {
            anchor: caret,
            focus: caret,
        });
        let selected = match input {
            #[cfg(test)]
            SelectionInput::Fixture(Some(selection)) => Some(TextSelection {
                anchor: boundary(selection.anchor).ok_or(SemanticError::InvalidText(self.id))?,
                focus: boundary(selection.focus).ok_or(SemanticError::InvalidText(self.id))?,
            }),
            #[cfg(test)]
            SelectionInput::Fixture(None) => caret,
            SelectionInput::View {
                selection: None, ..
            } => caret,
            SelectionInput::View {
                selection: Some(selection),
                top_row,
            } => {
                let locate = |point: HistoryPoint| -> Option<(usize, usize)> {
                    let row = usize::try_from(point.row.checked_sub(top_row)?).ok()?;
                    Some((row, rows.get(row)?.character_at(point.x)?))
                };
                match (
                    selection.rectangle,
                    locate(selection.anchor),
                    locate(selection.focus),
                ) {
                    (false, Some((a_row, a)), Some((f_row, f))) => {
                        let forward = (selection.anchor.row, selection.anchor.x)
                            <= (selection.focus.row, selection.focus.x);
                        let anchor = TextPosition {
                            node: self.rows[a_row],
                            character_index: a + usize::from(!forward),
                        };
                        let focus = TextPosition {
                            node: self.rows[f_row],
                            character_index: f + usize::from(forward),
                        };
                        Some(TextSelection { anchor, focus })
                    }
                    // Offscreen, disjoint or unmapped endpoints have no fabricated visible range.
                    _ => None,
                }
            }
        };
        self.text = rows;
        self.selection = selected;
        Ok(())
    }
}
