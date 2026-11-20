//! Match links against wrapped rows, keeping UTF-8 offsets tied to terminal cells.
use crate::{Result, RowMetadata, Screen, Terminal, ViewportPoint};
use std::ops::Range;
use url::Url;

mod columns;
mod file;
pub use file::{parse as parse_file_link, TerminalFileLink};

pub const MAX_WEB_LINK_BYTES: usize = 2048;

/// Terminal text must not pass arbitrary schemes or credentials to an opener.
pub fn safe_web_url(value: &str) -> Option<Url> {
    if value.len() > MAX_WEB_LINK_BYTES
        || value
            .chars()
            .any(|c| c.is_control() || c.is_whitespace() || c == '\\')
    {
        return None;
    }
    let url = Url::parse(value).ok()?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return None;
    }
    Some(url)
}

impl Terminal {
    /// Resolve a plaintext file link in the current viewport, including scrollback.
    pub fn file_link_at(&mut self, point: ViewportPoint) -> Result<Option<String>> {
        if point.x >= self.cols || point.y >= self.rows {
            return Ok(None);
        }
        let screen = self.render()?;
        let metadata = self.engine.viewport_rows().map_err(crate::Error)?;
        Ok(find_link(&screen, &metadata, point, false, true).map(|link| link.target))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinkRange {
    pub target: String,
    pub start: ViewportPoint,
    pub end: ViewportPoint,
}

pub(super) fn web_match(text: &str) -> impl Iterator<Item = (Range<usize>, &str)> {
    text.match_indices("http").filter_map(move |(start, _)| {
        let tail = &text[start..];
        if !tail.starts_with("http://") && !tail.starts_with("https://") {
            return None;
        }
        let end = tail
            .find(|c: char| c.is_whitespace() || c.is_control() || "<>\"'`".contains(c))
            .unwrap_or(tail.len());
        let mut candidate = &tail[..end];
        loop {
            let last = candidate.chars().next_back()?;
            let unmatched =
                |open, close| candidate.matches(open).count() < candidate.matches(close).count();
            if ".,;:!?*".contains(last)
                || last == ')' && unmatched('(', ')')
                || last == ']' && unmatched('[', ']')
            {
                candidate = &candidate[..candidate.len() - last.len_utf8()];
            } else {
                break;
            }
        }
        Some((start..start + candidate.len(), candidate))
    })
}

pub fn link_at(
    screen: &Screen,
    metadata: &[RowMetadata],
    point: ViewportPoint,
    files: bool,
) -> Option<LinkRange> {
    find_link(screen, metadata, point, true, files)
}

fn find_link(
    screen: &Screen,
    metadata: &[RowMetadata],
    point: ViewportPoint,
    web: bool,
    files: bool,
) -> Option<LinkRange> {
    let (x, y, cols, rows) = (
        usize::from(point.x),
        usize::from(point.y),
        usize::from(screen.cols),
        usize::from(screen.rows),
    );
    if x >= cols || y >= rows {
        return None;
    }
    if let Some(link) = columns::column_link_at(screen, metadata, point, web, files) {
        return Some(link);
    }
    let continued = |row: usize| {
        metadata
            .get(row)
            .is_some_and(|flags| flags.wrap_continuation)
    };
    let mut first = y;
    while first > 0 && continued(first) {
        first -= 1;
    }
    let mut last = y;
    while last + 1 < rows && continued(last + 1) {
        last += 1;
    }
    let mut text = String::new();
    let mut offsets = Vec::new();
    let mut hit = None;
    for row in first..=last {
        let cells = screen.cells.get(row * cols..row * cols + cols)?;
        for (col, cell) in cells.iter().enumerate().filter(|(_, cell)| cell.width > 0) {
            // A wide glyph moved to the next row leaves its unusable last cell blank.
            if col + 1 == cols
                && row < last
                && cell.text.is_empty()
                && screen
                    .cells
                    .get((row + 1) * cols)
                    .is_some_and(|next| next.width == 2)
            {
                continue;
            }
            let cell_text = if cell.text.is_empty() {
                " "
            } else {
                &cell.text
            };
            if row == y && (col..col + usize::from(cell.width)).contains(&x) {
                hit = Some(text.len());
            }
            let start = ViewportPoint {
                x: col as u16,
                y: row as u16,
            };
            let end = ViewportPoint {
                x: (col + usize::from(cell.width) - 1) as u16,
                y: row as u16,
            };
            offsets.extend(std::iter::repeat_n((start, end), cell_text.len()));
            text.push_str(cell_text);
        }
    }
    let hit = hit?;
    let web = web.then(|| web_match(&text).filter(|(_, url)| safe_web_url(url).is_some()));
    let files = files.then(|| file::file_match(&text));
    let link = web
        .into_iter()
        .flatten()
        .chain(files.into_iter().flatten())
        .find_map(|(offset, target)| {
            if !offset.contains(&hit) {
                return None;
            }
            Some(LinkRange {
                target: target.into(),
                start: offsets.get(offset.start)?.0,
                end: offsets.get(offset.end.checked_sub(1)?)?.1,
            })
        });
    link
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_halves_of_a_soft_wrapped_web_link_share_the_complete_target() {
        let mut term = Terminal::new(20, 4).unwrap();
        term.feed("界 https://example.com/path".as_bytes()).unwrap();
        let mut view = term.new_presentation_view().unwrap();
        let frame = term.view_render(&mut view).unwrap();
        let left = link_at(
            &frame.screen,
            &frame.row_metadata,
            ViewportPoint { x: 4, y: 0 },
            true,
        )
        .unwrap();
        let right = link_at(
            &frame.screen,
            &frame.row_metadata,
            ViewportPoint { x: 2, y: 1 },
            true,
        )
        .unwrap();
        assert_eq!(left, right);
        assert_eq!(left.target, "https://example.com/path");
        assert_eq!(left.start, ViewportPoint { x: 3, y: 0 });
    }
}
