use super::{Field, buffer, history};
use std::ops::Range;

impl Field {
    pub(super) fn replace(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
    ) -> Option<Range<usize>> {
        if self.read_only {
            return None;
        }
        let limit = buffer::text_limit(self.multiline, self.asset_editor);
        if text
            .chars()
            .any(|ch| ch.is_control() && !(self.multiline && matches!(ch, '\n' | '\t' | '\r')))
            || text.len() > limit
        {
            return None;
        }
        let range = match range {
            Some(range) => self.byte_range(range)?,
            None => self.marked.clone().unwrap_or(self.selection.clone()),
        };
        if range.start > range.end || self.text.len() - range.len() + text.len() > limit {
            return None;
        }
        let title_limit = if self.session_title {
            Some(160)
        } else {
            self.compact.then_some(200)
        };
        let text = if let Some(limit) = self.max_utf16_units.or(title_limit) {
            let retained = self.text[..range.start].encode_utf16().count()
                + self.text[range.end..].encode_utf16().count();
            let capacity = limit.saturating_sub(retained);
            let mut units = 0;
            let end = text
                .char_indices()
                .find_map(|(index, ch)| {
                    units += ch.len_utf16();
                    (units > capacity).then_some(index)
                })
                .unwrap_or(text.len());
            &text[..end]
        } else {
            text
        };
        let before = (self.asset_editor && self.marked.is_none()).then(|| history::EditState {
            text: self.snapshot_text(),
            anchor: self.anchor,
            cursor: self.cursor,
        });
        let typed = range.is_empty()
            && range.start == self.cursor
            && text.chars().count() == 1
            && !text.contains(['\r', '\n', '\t']);
        let mut inserted = buffer::replace_range(&mut self.text, range, text, limit)?;
        if self.asset_editor && self.text.contains('\r') {
            inserted = history::normalized_offset(&self.text, inserted.start)
                ..history::normalized_offset(&self.text, inserted.end);
            self.text = self.text.replace("\r\n", "\n").replace('\r', "\n");
        }
        if let Some(before) = before.filter(|before| before.text.as_ref() != self.text) {
            self.history.record(before, typed.then_some(inserted.end));
        }
        self.refresh_snapshot();
        self.set_cursor(inserted.end, false);
        self.marked = None;
        Some(inserted)
    }

    pub(super) fn undo_edit(&mut self, redo: bool) {
        if self.read_only {
            return;
        }
        let current = history::EditState {
            text: self.snapshot_text(),
            anchor: self.anchor,
            cursor: self.cursor,
        };
        if let Some(state) = self.history.transition(redo, current) {
            self.text = state.text.to_string();
            self.refresh_snapshot();
            self.anchor = state.anchor;
            self.set_cursor(state.cursor, true);
            self.marked = None;
        }
    }
}
