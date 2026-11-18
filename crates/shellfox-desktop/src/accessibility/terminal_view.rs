use super::*;
use shellfox_runtime::{Identity, PresentationLease, PresentationSelection, ViewFrame};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TerminalViewStamp {
    pub identity: Identity,
    pub view: PresentationLease,
    pub sequence: u64,
    pub version: u64,
    pub top_row: u64,
    pub total_rows: u64,
    pub selection: Option<PresentationSelection>,
}
impl TerminalViewStamp {
    pub(super) fn checked(view: &ViewFrame) -> Option<Self> {
        let screen = &view.screen.screen;
        if view.identity != view.screen.identity
            || view.screen.sequence > view.sequence
            || view.version == 0
            || view.row_metadata.len() != usize::from(screen.rows)
            || view.top_row.checked_add(u64::from(screen.rows))? > view.total_rows
        {
            return None;
        }
        if let Some(selection) = view.selection
            && (selection.view_top_row != view.top_row
                || selection.total_rows != view.total_rows
                || selection.anchor.row >= view.total_rows
                || selection.focus.row >= view.total_rows
                || selection.anchor.x >= screen.cols
                || selection.focus.x >= screen.cols)
        {
            return None;
        }
        Some(Self {
            identity: view.identity,
            view: view.view,
            sequence: view.sequence,
            version: view.version,
            top_row: view.top_row,
            total_rows: view.total_rows,
            selection: view.selection,
        })
    }
}

impl TerminalDocument {
    /// Live path: use TerminalView::presentation_frame().as_deref(), retaining the caller's Arc.
    /// Frame cells/metadata are borrowed, never cloned. No controller or parser operation occurs.
    pub fn snapshot_view(
        &mut self,
        view: &ViewFrame,
        geometry: TerminalGeometry,
        read_only: bool,
        now: Instant,
    ) -> Result<TerminalSnapshot, SemanticError> {
        let screen = &view.screen.screen;
        self.validate(screen, geometry)?;
        let stamp = TerminalViewStamp::checked(view).ok_or(SemanticError::InvalidText(self.id))?;
        if self.source.is_some_and(|old| {
            old.identity == stamp.identity
                && old.view == stamp.view
                && (stamp.version < old.version || stamp.sequence < old.sequence)
        }) {
            return Err(SemanticError::InvalidText(self.id));
        }
        let replaced = self
            .source
            .is_none_or(|old| old.identity != stamp.identity || old.view != stamp.view);
        let changed = self.source != Some(stamp);
        if replaced || self.grid != (screen.cols, screen.rows) || changed && self.sample_due(now) {
            let visual = view.cursor_visual;
            let cursor = if visual.visible {
                if visual.x >= screen.cols || visual.y >= screen.rows {
                    return Err(SemanticError::InvalidText(self.id));
                }
                Some(TerminalPoint {
                    column: visual.x,
                    row: visual.y,
                })
            } else {
                None
            };
            self.sample(
                screen,
                Some(&view.row_metadata),
                cursor,
                super::sample::SelectionInput::View {
                    selection: view.selection.as_ref(),
                    top_row: view.top_row,
                },
            )?;
            self.source = Some(stamp);
            self.sampled(screen, now);
        } else {
            self.pending = changed;
        }
        let read_only = read_only || matches!(view.view, PresentationLease::Observer(_));
        Ok(self.render_snapshot(geometry, read_only))
    }

    /// Convert AT boundaries to inclusive VT cells, retaining the exact sampled owner version.
    pub fn selection_action_for_view(
        &self,
        selection: &TextSelection,
        captured: Option<TerminalViewStamp>,
        view: &ViewFrame,
    ) -> Option<shellfox_runtime::PresentationAction> {
        let sampled = captured?;
        if self.source != Some(sampled) || sampled != TerminalViewStamp::checked(view)? {
            return None;
        }
        let version = sampled.version;
        let position = |point: accesskit::TextPosition| {
            let row = self.rows.iter().position(|id| *id == point.node)?;
            (point.character_index <= self.text.get(row)?.lengths.len())
                .then_some((row, point.character_index))
        };
        let anchor = position(selection.anchor)?;
        let focus = position(selection.focus)?;
        if anchor == focus {
            return Some(shellfox_runtime::PresentationAction::SelectionAtVersion {
                version,
                selection: None,
            });
        }
        let forward = anchor < focus;
        let cell = |(row, boundary): (usize, usize), after: bool| {
            let index = if after {
                boundary.checked_sub(1)?
            } else {
                boundary
            };
            let text = self.text.get(row)?;
            let width = *text.widths.get(index)?;
            if width <= 0. {
                return None;
            } // Hard LF has no VT cell.
            let start = *text.columns.get(index)?;
            let x = if after {
                start.checked_add(width as u16 - 1)?
            } else {
                start
            };
            Some(shellfox_ghostty_vt::ViewportPoint {
                x,
                y: u16::try_from(row).ok()?,
            })
        };
        let start = cell(anchor, !forward)?;
        let end = cell(focus, forward)?;
        // An equal narrow-cell pair cannot encode a reverse gesture. Do not report a fake direction.
        if !forward && start == end {
            return None;
        }
        Some(shellfox_runtime::PresentationAction::SelectionAtVersion {
            version,
            selection: Some(shellfox_ghostty_vt::Selection::Range {
                start,
                end,
                rectangle: false,
            }),
        })
    }
}
