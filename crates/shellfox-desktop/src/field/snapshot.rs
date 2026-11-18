use std::{
    cell::RefCell,
    ops::Range,
    sync::{Arc, Weak},
};

#[derive(Default)]
pub(super) struct MatchCache(RefCell<Option<CachedMatch>>);
struct CachedMatch {
    current: Weak<str>,
    saved: Weak<str>,
    equal: bool,
}
impl MatchCache {
    pub fn matches(&self, current: &Arc<str>, saved: &Arc<str>) -> bool {
        if Arc::ptr_eq(current, saved) {
            return true;
        }
        if let Some(cached) = &*self.0.borrow()
            && cached
                .current
                .upgrade()
                .is_some_and(|old| Arc::ptr_eq(&old, current))
            && cached
                .saved
                .upgrade()
                .is_some_and(|old| Arc::ptr_eq(&old, saved))
        {
            return cached.equal;
        }
        let equal = current.as_ref() == saved.as_ref();
        *self.0.borrow_mut() = Some(CachedMatch {
            current: Arc::downgrade(current),
            saved: Arc::downgrade(saved),
            equal,
        });
        equal
    }
}

pub(super) const LINE_WINDOW_BYTES: usize = 4096;
/// Immutable per-edit data. Rendering clones handles, never the complete editor buffer.
#[derive(Clone)]
pub(super) struct Snapshot {
    pub text: Arc<str>,
    checkpoints: Arc<[Checkpoint]>,
    rows: usize,
    utf16_len: usize,
}
#[derive(Clone, Copy)]
struct Checkpoint {
    offset: usize,
    row: usize,
    line_start: usize,
    utf16: usize,
}
impl Snapshot {
    pub fn new(text: &str) -> Self {
        // Byte checkpoints bound index memory even for millions of empty lines. Lookups scan
        // one 4 KiB block plus UTF-8 boundary bytes, including a long line's final block
        // rather than its complete prefix.
        let mut checkpoints = Vec::with_capacity(text.len() / LINE_WINDOW_BYTES + 1);
        let mut row = 0;
        let mut line_start = 0;
        let mut utf16 = 0;
        let mut next_checkpoint = LINE_WINDOW_BYTES;
        checkpoints.push(Checkpoint {
            offset: 0,
            row,
            line_start,
            utf16,
        });
        for (offset, character) in text.char_indices() {
            if offset >= next_checkpoint {
                checkpoints.push(Checkpoint {
                    offset,
                    row,
                    line_start,
                    utf16,
                });
                next_checkpoint = offset + LINE_WINDOW_BYTES;
            }
            if character == '\n' {
                row += 1;
                line_start = offset + 1;
            }
            utf16 += character.len_utf16();
        }
        Self {
            text: Arc::from(text),
            checkpoints: checkpoints.into(),
            rows: row + 1,
            utf16_len: utf16,
        }
    }
    pub fn line_count(&self) -> usize {
        self.rows
    }
    pub fn row(&self, offset: usize) -> usize {
        let offset = offset.min(self.text.len());
        let index = self
            .checkpoints
            .partition_point(|point| point.offset <= offset)
            .saturating_sub(1);
        let point = self.checkpoints[index];
        point.row
            + self.text.as_bytes()[point.offset..offset]
                .iter()
                .filter(|byte| **byte == b'\n')
                .count()
    }
    pub fn line(&self, row: usize) -> Option<Range<usize>> {
        if row >= self.rows {
            return None;
        }
        let index = self
            .checkpoints
            .partition_point(|point| point.row <= row)
            .saturating_sub(1);
        let point = self.checkpoints[index];
        let bytes = self.text.as_bytes();
        let mut offset = point.offset;
        let mut start = point.line_start;
        for _ in point.row..row {
            offset += bytes[offset..].iter().position(|byte| *byte == b'\n')? + 1;
            start = offset;
        }
        let end = bytes[offset..]
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(bytes.len(), |length| offset + length);
        Some(start..end)
    }
    pub fn utf16_for_byte(&self, offset: usize) -> Option<usize> {
        if !self.text.is_char_boundary(offset) {
            return None;
        }
        let index = self
            .checkpoints
            .partition_point(|point| point.offset <= offset)
            .saturating_sub(1);
        let point = self.checkpoints[index];
        Some(point.utf16 + self.text[point.offset..offset].encode_utf16().count())
    }
    pub fn byte_for_utf16(&self, offset: usize) -> Option<usize> {
        if offset > self.utf16_len {
            return None;
        }
        let index = self
            .checkpoints
            .partition_point(|point| point.utf16 <= offset)
            .saturating_sub(1);
        let point = self.checkpoints[index];
        super::buffer::byte_offset(&self.text[point.offset..], offset - point.utf16)
            .map(|index| point.offset + index)
    }
    pub fn window(&self, row: usize, column: usize, virtualized: bool) -> Option<Range<usize>> {
        let line = self.line(row)?;
        if !virtualized || line.len() <= LINE_WINDOW_BYTES {
            return Some(line);
        }
        let anchor = line.start + column.min(line.len());
        let mut start = anchor.saturating_sub(LINE_WINDOW_BYTES / 2).max(line.start);
        while !self.text.is_char_boundary(start) {
            start -= 1;
        }
        let mut end = (start + LINE_WINDOW_BYTES).min(line.end);
        while !self.text.is_char_boundary(end) {
            end -= 1;
        }
        Some(start..end)
    }
    pub fn visible_rows(
        &self,
        top: f32,
        padding: f32,
        line_height: f32,
        clip: Range<f32>,
    ) -> Range<usize> {
        if !top.is_finite()
            || !line_height.is_finite()
            || line_height <= 0.
            || clip.end <= clip.start
        {
            return 0..0;
        }
        let start = ((clip.start - top - padding) / line_height).floor().max(0.) as usize;
        let end = ((clip.end - top - padding) / line_height).ceil().max(0.) as usize;
        start.min(self.line_count())..end.min(self.line_count())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn eight_mib_many_line_snapshots_are_shared_and_only_visible_rows_are_visited() {
        let mut source = "x\r\n".repeat(8 * 1024 * 1024 / 3);
        source.push_str("xx");
        assert_eq!(source.len(), 8 * 1024 * 1024);
        let snapshot = Snapshot::new(&source);
        let other = snapshot.clone();
        assert!(Arc::ptr_eq(&snapshot.text, &other.text));
        assert!(Arc::ptr_eq(&snapshot.checkpoints, &other.checkpoints));
        assert!(snapshot.checkpoints.len() <= source.len() / LINE_WINDOW_BYTES + 1);
        let rows = snapshot.visible_rows(-18_000_000., 8., 18., 0.0..900.);
        assert!(rows.len() <= 51);
        assert!(rows.start > 900_000);
        for row in rows {
            assert_eq!(&snapshot.text[snapshot.line(row).unwrap()], "x\r");
        }
    }
    #[test]
    fn sparse_checkpoints_match_real_rows_across_utf8_crlf_and_dense_newlines() {
        let text = format!(
            "{}{}{}\nlast\n",
            "\u{feff}界😀\r\n".repeat(900),
            "\n".repeat(9000),
            "雪".repeat(7000)
        );
        let snapshot = Snapshot::new(&text);
        let mut offset = 0;
        for (row, line) in text.split('\n').enumerate() {
            let range = snapshot.line(row).unwrap();
            assert_eq!(range, offset..offset + line.len());
            assert_eq!(&snapshot.text[range.clone()], line);
            assert_eq!(snapshot.row(range.start), row);
            assert_eq!(snapshot.row(range.end), row);
            offset += line.len() + 1;
        }
        assert!(snapshot.line(snapshot.line_count()).is_none());
        assert_eq!(snapshot.row(text.len()), snapshot.line_count() - 1);
    }
    #[test]
    fn indexed_native_offsets_preserve_utf16_surrogates_bom_and_crlf() {
        let text = format!(
            "{}界\r\n{}雪",
            "a😀\u{feff}".repeat(1500),
            "x".repeat(20_000)
        );
        let snapshot = Snapshot::new(&text);
        for utf16 in [
            0,
            1,
            2,
            3,
            4095,
            4096,
            4097,
            10_000,
            text.encode_utf16().count(),
            usize::MAX,
        ] {
            assert_eq!(
                snapshot.byte_for_utf16(utf16),
                super::super::buffer::byte_offset(&text, utf16)
            );
        }
        for offset in [0, 1, 2, 3, 4095, 4096, 4097, text.len(), text.len() + 1] {
            let expected = text
                .get(..offset)
                .map(|prefix| prefix.encode_utf16().count());
            assert_eq!(snapshot.utf16_for_byte(offset), expected);
        }
    }
    #[test]
    fn dirty_comparison_is_cached_by_both_immutable_snapshots_and_save_keeps_later_edits() {
        let saved: Arc<str> = Arc::from("x".repeat(8 * 1024 * 1024));
        let draft: Arc<str> = Arc::from(format!("{}y", "x".repeat(saved.len() - 1)));
        let later: Arc<str> = Arc::from(format!("{}z", "x".repeat(saved.len() - 1)));
        let cache = MatchCache::default();
        assert!(cache.matches(&saved, &saved));
        assert!(!cache.matches(&draft, &saved));
        assert!(!cache.matches(&draft.clone(), &saved.clone()));
        assert!(
            cache
                .0
                .borrow()
                .as_ref()
                .is_some_and(|cached| !cached.equal)
        );
        // The completed save captures draft, not text typed while its chunk write was in flight.
        let saved_after_write = draft.clone();
        assert!(!cache.matches(&later, &saved_after_write));
        assert!(cache.matches(&draft, &saved_after_write));
        let reverted: Arc<str> = Arc::from(saved_after_write.as_ref());
        assert!(cache.matches(&reverted, &saved_after_write));
        assert!(cache.matches(&reverted, &saved_after_write));
    }
    #[test]
    fn long_utf8_lines_have_bounded_paint_windows_and_exact_source_boundaries() {
        let snapshot = Snapshot::new(&format!("\u{feff}{}\r\nend\n", "雪😀".repeat(100_000)));
        for column in [0, 1, 100, 12_345, 699_999, 700_003] {
            let range = snapshot.window(0, column, true).unwrap();
            assert!(range.len() <= LINE_WINDOW_BYTES);
            assert!(snapshot.text.is_char_boundary(range.start));
            assert!(snapshot.text.is_char_boundary(range.end));
            assert_eq!(snapshot.row(range.start), 0);
        }
        assert_eq!(&snapshot.text[snapshot.line(1).unwrap()], "end");
        assert_eq!(
            snapshot.line(2).unwrap(),
            snapshot.text.len()..snapshot.text.len()
        );
        assert_eq!(snapshot.row(snapshot.text.len()), 2);
    }
}
