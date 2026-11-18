use super::SemanticError;
use accesskit::{Action, Live, Node, NodeId, Rect, Role, TextDirection, TextSelection};
use shellfox_ghostty_vt::Screen;
use std::time::{Duration, Instant};

#[path = "terminal_sample.rs"]
mod sample;
#[path = "terminal_view.rs"]
mod view;
pub use view::TerminalViewStamp;

const UPDATE_INTERVAL: Duration = Duration::from_millis(50);
const MAX_ROWS: usize = 250;
const MAX_CELLS: usize = 500 * MAX_ROWS;
const MAX_BYTES: usize = 16 * 1024 * 1024 + MAX_ROWS;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TerminalGeometry {
    /// Actual canvas bounds and advances, in AccessKit platform units.
    pub bounds: Rect,
    pub cell_width: f64,
    pub line_height: f64,
}
impl TerminalGeometry {
    pub fn from_layout(
        bounds: gpui::Bounds<gpui::Pixels>,
        cell_width: f32,
        line_height: f32,
        window: &gpui::Window,
    ) -> Self {
        let scale = if cfg!(target_os = "macos") {
            1.
        } else {
            f64::from(window.scale_factor())
        };
        Self {
            bounds: super::element::native_rect(bounds, window),
            cell_width: f64::from(cell_width) * scale,
            line_height: f64::from(line_height) * scale,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TerminalPoint {
    pub column: u16,
    pub row: u16,
}
#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TerminalSelection {
    pub anchor: TerminalPoint,
    pub focus: TerminalPoint,
}
#[derive(Clone)]
pub struct TerminalSnapshot {
    pub document: Node,
    pub runs: Vec<(NodeId, Node)>,
    /// Coherent with these text runs, even when a newer presentation is coalesced.
    pub terminal_stamp: Option<TerminalViewStamp>,
}

struct Row {
    text: String,
    lengths: Vec<u8>,
    columns: Vec<u16>,
    widths: Vec<f32>,
    end_column: u16,
}

/// Reads immutable renderer frames only. Never feeds/replays bytes, drains effects or writes input.
pub struct TerminalDocument {
    id: NodeId,
    rows: Vec<NodeId>,
    last: Option<Instant>,
    text: Vec<Row>,
    selection: Option<TextSelection>,
    source: Option<TerminalViewStamp>,
    grid: (u16, u16),
    pending: bool,
}
impl TerminalDocument {
    pub fn new(id: NodeId, rows: Vec<NodeId>) -> Result<Self, SemanticError> {
        let unique: std::collections::BTreeSet<_> = rows.iter().copied().collect();
        if rows.is_empty()
            || rows.len() > MAX_ROWS
            || unique.len() != rows.len()
            || unique.contains(&id)
        {
            return Err(SemanticError::InvalidText(id));
        }
        Ok(Self {
            id,
            rows,
            last: None,
            text: Vec::new(),
            selection: None,
            source: None,
            grid: (0, 0),
            pending: false,
        })
    }

    /// Fixtures without producer metadata. Production uses snapshot_view.
    #[cfg(test)]
    pub fn snapshot(
        &mut self,
        screen: &Screen,
        geometry: TerminalGeometry,
        selection: Option<TerminalSelection>,
        read_only: bool,
        now: Instant,
    ) -> Result<TerminalSnapshot, SemanticError> {
        self.validate(screen, geometry)?;
        if self.source.is_some() || self.grid != (screen.cols, screen.rows) || self.sample_due(now)
        {
            let cursor = screen.cursor.visible.then_some(TerminalPoint {
                column: screen.cursor.x,
                row: screen.cursor.y,
            });
            self.sample(
                screen,
                None,
                cursor,
                sample::SelectionInput::Fixture(selection),
            )?;
            self.source = None;
            self.sampled(screen, now);
        } else {
            self.pending = true;
        }
        Ok(self.render_snapshot(geometry, read_only))
    }

    fn validate(&self, screen: &Screen, geometry: TerminalGeometry) -> Result<(), SemanticError> {
        let TerminalGeometry {
            bounds,
            cell_width,
            line_height,
        } = geometry;
        if ![
            bounds.x0,
            bounds.y0,
            bounds.x1,
            bounds.y1,
            cell_width,
            line_height,
        ]
        .into_iter()
        .all(f64::is_finite)
            || bounds.x1 < bounds.x0
            || bounds.y1 < bounds.y0
            || cell_width <= 0.
            || line_height <= 0.
        {
            return Err(SemanticError::InvalidBounds(self.id));
        }
        let count = usize::from(screen.rows) * usize::from(screen.cols);
        if screen.rows == 0
            || screen.cols == 0
            || usize::from(screen.rows) > self.rows.len()
            || count > MAX_CELLS
            || count != screen.cells.len()
        {
            return Err(SemanticError::InvalidText(self.id));
        }
        Ok(())
    }
    fn sample_due(&self, now: Instant) -> bool {
        self.last
            .is_none_or(|last| now.saturating_duration_since(last) >= UPDATE_INTERVAL)
    }
    fn sampled(&mut self, screen: &Screen, now: Instant) {
        self.last = Some(now);
        self.grid = (screen.cols, screen.rows);
        self.pending = false;
    }
    fn render_snapshot(&self, geometry: TerminalGeometry, read_only: bool) -> TerminalSnapshot {
        let TerminalGeometry {
            bounds,
            cell_width,
            line_height,
        } = geometry;
        let mut document = Node::new(Role::Terminal);
        document.set_label(if read_only {
            "Read-only terminal"
        } else {
            "Interactive terminal"
        });
        document.set_read_only();
        document.set_live(Live::Off);
        document.add_action(Action::Focus);
        document.add_action(Action::SetTextSelection);
        document.set_bounds(bounds);
        if let Some(selection) = self.selection {
            document.set_text_selection(selection);
        }
        let runs = self
            .text
            .iter()
            .enumerate()
            .map(|(index, row)| {
                let mut run = Node::new(Role::TextRun);
                run.set_value(row.text.clone());
                run.set_character_lengths(row.lengths.clone());
                run.set_text_direction(TextDirection::LeftToRight);
                run.set_character_positions(
                    row.columns
                        .iter()
                        .map(|col| (f64::from(*col) * cell_width) as f32)
                        .collect::<Vec<_>>(),
                );
                run.set_character_widths(
                    row.widths
                        .iter()
                        .map(|w| *w * cell_width as f32)
                        .collect::<Vec<_>>(),
                );
                let top = bounds.y0 + index as f64 * line_height;
                run.set_bounds(Rect::new(bounds.x0, top, bounds.x1, top + line_height));
                (self.rows[index], run)
            })
            .collect();
        TerminalSnapshot {
            document,
            runs,
            terminal_stamp: self.source,
        }
    }
    pub fn refresh_due(&self, now: Instant) -> bool {
        self.pending && self.sample_due(now)
    }
}
