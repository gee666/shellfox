use super::{Field, snapshot::Snapshot, syntax};
use gpui::{App, Entity};
use std::sync::Arc;

impl Field {
    /// Only the bounded chunk/asset editor opts into 8 MiB. Forms retain their existing limits.
    pub(crate) fn asset_editor(path: &str, text: String, cx: &mut App) -> Entity<Self> {
        let entity = Self::editor(text, cx);
        entity.update(cx, |field, _| {
            field.asset_editor = true;
            field.syntax_language = syntax::Language::for_path(path);
            field.refresh_syntax();
            field.set_cursor(0, false);
        });
        entity
    }
    pub(crate) fn goto_source_position(&mut self, line: usize, column: Option<usize>) {
        let row = line
            .saturating_sub(1)
            .min(self.snapshot.line_count().saturating_sub(1));
        if let Some(range) = self.snapshot.line(row) {
            let offset = source_offset(
                self.text[range.clone()].trim_end_matches('\r'),
                column.unwrap_or(1),
            );
            self.set_cursor(range.start + offset, false);
            self.scroll.set_offset(gpui::point(
                gpui::px(0.),
                gpui::px(-((row.saturating_sub(3)) as f32) * self.line_height),
            ));
        }
    }

    pub(crate) fn show_file_location(&mut self, path: String, cx: &mut gpui::Context<Self>) {
        self.set_text(path, cx);
        self.scroll_x = 0.;
        self.set_cursor(0, false);
    }

    pub(super) fn multiline_text_inset(&self) -> f32 {
        // The Linux textarea has the same CSS inset and line box, but seats the baseline 1px lower.
        if self.asset_editor && cfg!(target_os = "linux") {
            9.
        } else {
            8.
        }
    }

    pub(crate) fn snapshot_text(&self) -> Arc<str> {
        self.snapshot.text.clone()
    }
    pub(crate) fn matches_snapshot(&self, saved: &Arc<str>) -> bool {
        self.dirty_cache.matches(&self.snapshot.text, saved)
    }
    pub(super) fn refresh_snapshot(&mut self) {
        self.snapshot = Snapshot::new(&self.text);
        self.refresh_syntax();
        self.dirty_cache = super::snapshot::MatchCache::default();
        self.measured_semantics = None;
        self.painted_lines.clear();
    }

    fn refresh_syntax(&mut self) {
        if let Some(language) = self.syntax_language {
            self.syntax_spans = syntax::highlight(language, &self.text);
        }
    }
}

fn source_offset(line: &str, column: usize) -> usize {
    let mut remaining = column.saturating_sub(1);
    for (offset, ch) in line.char_indices() {
        let Some(rest) = remaining.checked_sub(ch.len_utf16()) else {
            return offset;
        };
        remaining = rest;
    }
    line.len()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn source_position_uses_utf16_columns_without_splitting_utf8() {
        assert_eq!(source_offset("a🦊z", 2), 1);
        assert_eq!(source_offset("a🦊z", 3), 1);
        assert_eq!(source_offset("a🦊z", 4), 5);
        assert_eq!(source_offset("a🦊z", 99), 6);
    }
}
