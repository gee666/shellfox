//! Up to seventeen owner-thread views over one authoritative parser.
use crate::{
    presentation_api::*, GridLimits, Identity, RuntimeError, ScreenFrame, MAX_SCREEN_TEXT_BYTES,
};
use shellfox_ghostty_vt::{PresentationView, Terminal, MAX_PRESENTATION_VIEWS};
use std::sync::Arc;

#[path = "presentation_focus.rs"]
mod focus;

pub(crate) enum PresentationResult {
    Frame(Arc<ViewFrame>),
    Applied,
    Text(Option<String>),
}
struct Entry {
    lease: PresentationLease,
    view: PresentationView,
    version: u64,
    source: u64,
    focused: bool,
    reported_focus: Option<bool>,
    cursor: Option<CursorVisual>,
    cache: Option<Arc<ViewFrame>>,
}
impl Entry {
    fn invalidate(&mut self) -> Result<(), RuntimeError> {
        self.version = self
            .version
            .checked_add(1)
            .ok_or_else(|| RuntimeError::validation("presentation version exhausted"))?;
        self.cache = None;
        Ok(())
    }
}
#[derive(Default)]
pub(crate) struct Presentations {
    entries: Vec<Entry>,
    source: u64,
}
fn optional(error: shellfox_ghostty_vt::Error) -> RuntimeError {
    RuntimeError {
        code: if error.0 == -2 {
            crate::ErrorCode::Validation
        } else {
            crate::ErrorCode::Unsupported
        },
        message: format!("Terminal presentation unavailable: {error}"),
    }
}
impl Presentations {
    pub(crate) fn new() -> Self {
        Self::default()
    }
    pub(crate) fn controller_changed(&mut self, lease: crate::Lease) {
        self.entries.retain(
            |entry| !matches!(entry.lease, PresentationLease::Controller(prior) if prior != lease),
        );
    }
    pub(crate) fn release_controller(&mut self, lease: crate::Lease) {
        self.entries
            .retain(|entry| entry.lease != PresentationLease::Controller(lease));
    }
    pub(crate) fn observer_changed(&mut self, lease: crate::ObserverLease) {
        self.entries.retain(|entry| !matches!(entry.lease, PresentationLease::Observer(prior) if prior.viewer == lease.viewer && prior != lease));
    }
    pub(crate) fn unobserve(&mut self, lease: crate::ObserverLease) {
        self.entries
            .retain(|entry| entry.lease != PresentationLease::Observer(lease));
    }
    pub(crate) fn source_changed(&mut self) -> Result<(), RuntimeError> {
        self.source = self
            .source
            .checked_add(1)
            .ok_or_else(|| RuntimeError::validation("presentation source version exhausted"))?;
        Ok(())
    }
    fn entry(
        &mut self,
        terminal: &mut Terminal,
        lease: PresentationLease,
    ) -> Result<&mut Entry, RuntimeError> {
        if let Some(index) = self.entries.iter().position(|entry| entry.lease == lease) {
            return Ok(&mut self.entries[index]);
        }
        if self.entries.len() >= MAX_PRESENTATION_VIEWS {
            return Err(RuntimeError::validation(
                "At most 17 presentation views may attach.",
            ));
        }
        let view = terminal.new_presentation_view().map_err(optional)?;
        self.entries.push(Entry {
            lease,
            view,
            version: 0,
            source: self.source,
            focused: true,
            reported_focus: None,
            cursor: None,
            cache: None,
        });
        self.entries
            .last_mut()
            .ok_or_else(|| RuntimeError::validation("presentation view missing"))
    }
    fn check_selection_frame(
        &self,
        lease: PresentationLease,
        version: u64,
        selection: &Option<shellfox_ghostty_vt::Selection>,
    ) -> Result<(), RuntimeError> {
        let stale = || RuntimeError {
            code: crate::ErrorCode::StalePresentation,
            message: "Selection coordinates are stale. Request a fresh semantic frame.".into(),
        };
        let entry = self
            .entries
            .iter()
            .find(|entry| entry.lease == lease)
            .ok_or_else(stale)?;
        let cached = entry.cache.as_ref().ok_or_else(stale)?;
        // Source revisions invalidate lazily: an unchanged cached version alone
        // cannot prove that no output/reflow occurred after the sampled frame.
        if entry.source != self.source || entry.version != version || cached.version != version {
            return Err(stale());
        }
        let valid = |point: shellfox_ghostty_vt::ViewportPoint| {
            point.x < cached.screen.screen.cols && point.y < cached.screen.screen.rows
        };
        let valid = match selection {
            Some(shellfox_ghostty_vt::Selection::Range { start, end, .. }) => {
                valid(*start) && valid(*end)
            }
            Some(
                shellfox_ghostty_vt::Selection::Word(point)
                | shellfox_ghostty_vt::Selection::Line(point),
            ) => valid(*point),
            Some(shellfox_ghostty_vt::Selection::All) | None => true,
        };
        if !valid {
            return Err(RuntimeError::validation(
                "Selection coordinates are outside the sampled frame.",
            ));
        }
        Ok(())
    }
    pub(crate) fn action(
        &mut self,
        terminal: &mut Terminal,
        identity: Identity,
        sequence: u64,
        limits: GridLimits,
        lease: PresentationLease,
        action: PresentationAction,
    ) -> Result<PresentationResult, RuntimeError> {
        if let PresentationAction::SelectionAtVersion { version, selection } = &action {
            self.check_selection_frame(lease, *version, selection)?;
        }
        let source = self.source;
        let entry = self.entry(terminal, lease)?;
        match action {
            PresentationAction::ResizeExternal { .. } => {
                return Err(RuntimeError::validation(
                    "External layout must be handled by its runtime owner.",
                ))
            }
            PresentationAction::Scroll { delta } => {
                terminal
                    .view_scroll(&mut entry.view, delta)
                    .map_err(optional)?;
                entry.invalidate()?;
            }
            PresentationAction::FollowTail => {
                terminal
                    .view_follow_tail(&mut entry.view)
                    .map_err(optional)?;
                entry.invalidate()?;
            }
            PresentationAction::Selection { selection }
            | PresentationAction::SelectionAtVersion { selection, .. } => {
                terminal
                    .view_select(&mut entry.view, selection)
                    .map_err(optional)?;
                entry.invalidate()?;
            }
            PresentationAction::Copy => {
                return terminal
                    .view_copy(&mut entry.view)
                    .map(PresentationResult::Text)
                    .map_err(optional)
            }
            PresentationAction::Hyperlink { point } => {
                return terminal
                    .view_hyperlink(&mut entry.view, point)
                    .map(PresentationResult::Text)
                    .map_err(optional)
            }
            PresentationAction::Frame { focused } => {
                let frame = frame(terminal, entry, identity, sequence, limits, source, focused)?;
                return Ok(PresentationResult::Frame(frame));
            }
        }
        Ok(PresentationResult::Applied)
    }
    /// Input follows only an already-created controller view. Headless input
    /// does not allocate a presentation handle or copy a cell grid.
    pub(crate) fn follow_input(
        &mut self,
        terminal: &mut Terminal,
        lease: crate::Lease,
    ) -> Result<(), RuntimeError> {
        if let Some(entry) = self
            .entries
            .iter_mut()
            .find(|entry| entry.lease == PresentationLease::Controller(lease))
        {
            terminal
                .view_follow_tail(&mut entry.view)
                .map_err(optional)?;
            entry.invalidate()?;
        }
        Ok(())
    }
    pub(crate) fn legacy(
        &mut self,
        terminal: &mut Terminal,
        identity: Identity,
        sequence: u64,
        limits: GridLimits,
        lease: crate::Lease,
    ) -> Result<Arc<ScreenFrame>, RuntimeError> {
        let source = self.source;
        let entry = self.entry(terminal, PresentationLease::Controller(lease))?;
        let focused = entry.focused;
        frame(terminal, entry, identity, sequence, limits, source, focused)
            .map(|frame| Arc::clone(&frame.screen))
    }
}
fn frame(
    terminal: &mut Terminal,
    entry: &mut Entry,
    identity: Identity,
    sequence: u64,
    limits: GridLimits,
    source: u64,
    focused: bool,
) -> Result<Arc<ViewFrame>, RuntimeError> {
    if entry.source == source {
        if let Some(cached) = &entry.cache {
            limits.validate_size(cached.screen.screen.cols, cached.screen.screen.rows)?;
            if entry.focused == focused {
                return Ok(Arc::clone(cached));
            }
            // A focus repaint shares cells and row metadata, not a second grid copy.
            entry.version = entry
                .version
                .checked_add(1)
                .ok_or_else(|| RuntimeError::validation("presentation version exhausted"))?;
            let updated = Arc::new(ViewFrame {
                identity,
                view: entry.lease,
                sequence,
                version: entry.version,
                screen: Arc::clone(&cached.screen),
                row_metadata: Arc::clone(&cached.row_metadata),
                cursor_visual: entry
                    .cursor
                    .ok_or_else(|| RuntimeError::validation("cursor visual missing"))?
                    .for_focus(focused),
                top_row: cached.top_row,
                total_rows: cached.total_rows,
                selection: cached.selection,
            });
            entry.focused = focused;
            entry.cache = Some(Arc::clone(&updated));
            return Ok(updated);
        }
    }
    let rendered = terminal
        .view_render_with_limit(&mut entry.view, limits.max_cells())
        .map_err(optional)?;
    limits.validate_size(rendered.screen.cols, rendered.screen.rows)?;
    let bytes = rendered
        .screen
        .cells
        .iter()
        .try_fold(0usize, |total, cell| total.checked_add(cell.text.len()));
    if bytes.is_none_or(|bytes| bytes > MAX_SCREEN_TEXT_BYTES) {
        return Err(RuntimeError::validation("presentation text exceeds 16 MiB"));
    }
    let selection = terminal.view_selection(&mut entry.view).map_err(optional)?;
    if selection.is_some_and(|selection| {
        selection.view_top_row != rendered.top_row || selection.total_rows != rendered.total_rows
    }) {
        return Err(RuntimeError::validation(
            "selection endpoints do not match the view frame",
        ));
    }
    let mut cursor = terminal.cursor_visual().map_err(optional)?;
    cursor.x = rendered.screen.cursor.x;
    cursor.y = rendered.screen.cursor.y;
    cursor.visible = rendered.screen.cursor.visible;
    entry.version = entry
        .version
        .checked_add(1)
        .ok_or_else(|| RuntimeError::validation("presentation version exhausted"))?;
    let screen = Arc::new(ScreenFrame {
        identity,
        sequence,
        screen: rendered.screen,
        mouse_tracking: terminal.mouse_tracking().map_err(optional)?,
        alternate_scroll: terminal.alternate_scroll().map_err(optional)?,
    });
    let result = Arc::new(ViewFrame {
        identity,
        view: entry.lease,
        sequence,
        version: entry.version,
        screen,
        row_metadata: rendered.row_metadata.into(),
        cursor_visual: cursor.for_focus(focused),
        selection,
        top_row: rendered.top_row,
        total_rows: rendered.total_rows,
    });
    entry.source = source;
    entry.focused = focused;
    entry.cursor = Some(cursor);
    entry.cache = Some(Arc::clone(&result));
    Ok(result)
}
#[cfg(test)]
#[path = "presentation_behaviour_tests.rs"]
mod behaviour_tests;
#[cfg(test)]
#[path = "presentation_guard_tests.rs"]
mod guard_tests;
#[cfg(test)]
#[path = "presentation_selection_tests.rs"]
mod selection_tests;
#[cfg(test)]
#[path = "presentation_tests.rs"]
mod tests;
