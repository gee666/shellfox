use std::ops::Range;

/// Converts native source byte offsets to the same tab-expanded presentation used by the editor.
#[derive(Clone, Default)]
pub(super) struct Projection {
    edits: Vec<(usize, Range<usize>)>,
}

impl Projection {
    pub fn build(source: &str, initial_column: usize) -> (String, Self, usize) {
        let mut text = String::with_capacity(source.len());
        let mut column = initial_column;
        let mut edits = Vec::new();
        for (offset, ch) in source.char_indices() {
            match ch {
                '\t' => {
                    let start = text.len();
                    let advance = 4 - column % 4;
                    text.extend(std::iter::repeat_n(' ', advance));
                    edits.push((offset, start..text.len()));
                    column += advance;
                }
                '\r' if offset + 1 == source.len() => {
                    edits.push((offset, text.len()..text.len()));
                }
                _ => {
                    text.push(ch);
                    column += unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
                }
            }
        }
        (text, Self { edits }, column)
    }

    pub fn display_index(&self, source: usize) -> usize {
        let count = self.edits.partition_point(|(offset, _)| *offset < source);
        count.checked_sub(1).map_or(source, |last| {
            let (offset, span) = &self.edits[last];
            span.end + source - offset - 1
        })
    }

    pub fn source_index(&self, display: usize) -> usize {
        let count = self.edits.partition_point(|(_, span)| span.end <= display);
        if let Some((offset, span)) = self.edits.get(count)
            && span.contains(&display)
        {
            return offset + usize::from((display - span.start) * 2 >= span.len());
        }
        count.checked_sub(1).map_or(display, |last| {
            let (offset, span) = &self.edits[last];
            offset + 1 + display - span.end
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tabs_land_on_four_column_stops_and_boundaries_round_trip() {
        let (visible, map, column) = Projection::build("界\tx\tZ\r", 0);
        assert_eq!(visible, "界  x   Z");
        assert_eq!(column, 9);
        for (source, display) in [(0, 0), (3, 3), (4, 5), (5, 6), (6, 9), (8, 10)] {
            assert_eq!(map.display_index(source), display);
            assert_eq!(map.source_index(display), source);
        }
        assert_eq!(map.source_index(4), 4);
        assert_eq!(Projection::build("\tb", 3).0, " b");
    }
}
