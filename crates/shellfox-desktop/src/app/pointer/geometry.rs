use super::super::{Backend, TerminalTheme, TerminalView};
use crate::renderer;
use anyhow::{Result, ensure};
use gpui::{Bounds, Context, Pixels, Point, Window};
use shellfox_ghostty_vt::ViewportPoint;
use shellfox_runtime::Command;

pub(super) fn hit_cell(
    bounds: Bounds<Pixels>,
    position: Point<Pixels>,
    cell_width: f32,
    line_height: f32,
    grid: (u16, u16),
    clamp: bool,
) -> Option<ViewportPoint> {
    if !clamp && !bounds.contains(&position) {
        return None;
    }
    let x = f32::from(position.x - bounds.left());
    let y = f32::from(position.y - bounds.top());
    if !x.is_finite() || !y.is_finite() || cell_width <= 0. || line_height <= 0. {
        return None;
    }
    if !clamp && (x >= f32::from(grid.0) * cell_width || y >= f32::from(grid.1) * line_height) {
        return None;
    }
    Some(ViewportPoint {
        x: (x / cell_width)
            .floor()
            .clamp(0., f32::from(grid.0.saturating_sub(1))) as u16,
        y: (y / line_height)
            .floor()
            .clamp(0., f32::from(grid.1.saturating_sub(1))) as u16,
    })
}

impl TerminalView {
    pub fn is_failed(&self) -> bool {
        self.failed
    }
    pub fn runtime_phase(&self) -> Option<shellfox_runtime::Phase> {
        match &self.backend {
            Backend::Fixture(_) => None,
            Backend::Attached(_) => self.phase,
        }
    }

    pub fn apply_preferences(
        &mut self,
        preferred_font: &str,
        font_size: f32,
        theme: TerminalTheme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<()> {
        ensure!(!self.failed && !self.closing, "terminal has stopped");
        ensure!(theme.valid(), "theme colors must be 24-bit RGB");
        let (font, cell_width) = renderer::choose_font(window, preferred_font, font_size)?;
        let colors = theme.colors();
        // Observer fonts/themes are local. Only a controller changes parser colors.
        if matches!(self.backend, Backend::Fixture(_)) || self.lease.is_some() {
            self.apply_terminal(
                |lease| Command::Colors {
                    lease,
                    colors: Box::new(colors.clone()),
                },
                |terminal| terminal.set_colors(&colors),
            )?;
        }
        self.font = font;
        self.cell_width = cell_width;
        self.font_size = font_size;
        self.line_height = (font_size * renderer::LINE_HEIGHT / renderer::FONT_SIZE).ceil();
        self.theme = theme;
        if let Some(bounds) = self.bounds {
            self.set_terminal_bounds(bounds, cx);
        }
        cx.notify();
        Ok(())
    }

    /// Content bounds exclude application chrome and the terminal status line.
    pub fn set_terminal_bounds(&mut self, bounds: Bounds<Pixels>, cx: &mut Context<Self>) {
        self.bounds = Some(bounds);
        if self.failed
            || self.closing
            || bounds.size.width <= gpui::px(0.)
            || bounds.size.height <= gpui::px(0.)
        {
            return;
        }
        let grid = renderer::grid_size(
            bounds.size.width.into(),
            bounds.size.height.into(),
            self.cell_width,
            self.line_height,
        );
        if grid == self.grid {
            return;
        }
        if matches!(self.backend, Backend::Attached(_)) && self.lease.is_none() {
            self.grid = grid;
            return;
        }
        let result = self.apply_terminal(
            |lease| Command::Resize {
                lease,
                cols: grid.0,
                rows: grid.1,
            },
            |terminal| {
                terminal.resize(grid.0, grid.1)?;
                terminal.drain_replies()?;
                Ok(())
            },
        );
        match result {
            Ok(()) => self.grid = grid,
            Err(error) => self.status = format!("Resize rejected: {error}"),
        }
        self.selection_drag.finish();
        self.link_press = None;
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{point, px, size};
    #[test]
    fn hit_testing_subtracts_sidebar_and_header_and_rejects_gutter() {
        let bounds = Bounds::new(point(px(240.), px(48.)), size(px(805.), px(465.)));
        assert_eq!(
            hit_cell(bounds, point(px(261.), px(90.)), 10., 19., (80, 24), false),
            Some(ViewportPoint { x: 2, y: 2 })
        );
        assert!(hit_cell(bounds, point(px(200.), px(90.)), 10., 19., (80, 24), false).is_none());
        assert!(hit_cell(bounds, point(px(1043.), px(90.)), 10., 19., (80, 24), false).is_none());
        assert_eq!(
            hit_cell(bounds, point(px(2000.), px(900.)), 10., 19., (80, 24), true),
            Some(ViewportPoint { x: 79, y: 23 })
        );
    }
}
