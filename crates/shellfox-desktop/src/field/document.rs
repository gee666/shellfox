use super::{Field, snapshot::Snapshot};
use gpui::{App, Entity};
use std::sync::Arc;

impl Field {
    /// Only the bounded chunk/asset editor opts into 8 MiB. Forms retain their existing limits.
    pub(crate) fn asset_editor(text: String, cx: &mut App) -> Entity<Self> {
        let entity = Self::editor(text, cx);
        entity.update(cx, |field, _| {
            field.asset_editor = true;
            field.set_cursor(0, false);
        });
        entity
    }
    pub(crate) fn snapshot_text(&self) -> Arc<str> {
        self.snapshot.text.clone()
    }
    pub(crate) fn matches_snapshot(&self, saved: &Arc<str>) -> bool {
        self.dirty_cache.matches(&self.snapshot.text, saved)
    }
    pub(super) fn refresh_snapshot(&mut self) {
        self.snapshot = Snapshot::new(&self.text);
        self.dirty_cache = super::snapshot::MatchCache::default();
        self.measured_semantics = None;
        self.painted_lines.clear();
    }
}
