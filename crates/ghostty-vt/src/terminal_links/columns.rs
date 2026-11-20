//! Hard line breaks inside TUI table columns. Underline only the hit column.
use super::{file, safe_web_url, web_match, LinkRange};
use crate::{RowMetadata, Screen, ViewportPoint};
use std::ops::Range;

const BORDERS: &str = "│┃║┆┇┊┋╎╏";
const CHAIN_ROWS: usize = 24;
const EDGE_SLACK: u16 = 2;
const INDENT_SLACK: u16 = 4;

#[derive(Clone, Copy)]
enum Kind {
    Web,
    File,
}

impl Kind {
    fn last_start(self, text: &str) -> Option<Range<usize>> {
        match self {
            Self::Web => text
                .match_indices("http")
                .filter_map(|(start, _)| {
                    let tail = &text[start..];
                    (tail.starts_with("https://") || tail.starts_with("http://"))
                        .then(|| start..start + token(tail, "<>\"'`").len())
                })
                .last(),
            Self::File => file::file_start(text),
        }
    }

    fn target(self, text: &str) -> Option<(Range<usize>, &str)> {
        match self {
            Self::Web => web_match(text).find(|(_, text)| safe_web_url(text).is_some()),
            Self::File => file::file_match(text).next(),
        }
    }

    fn continuation<'a>(self, text: &'a str, joined: &str) -> Option<&'a str> {
        if matches!(self, Self::File) {
            if let Some(quote) = joined
                .chars()
                .next()
                .filter(|value| matches!(value, '\'' | '"'))
            {
                if joined.len() > 1 && joined.ends_with(quote) {
                    return None;
                }
                return Some(match text.find(quote) {
                    Some(end) => &text[..end + 1],
                    None => text.trim_end(),
                });
            }
        }
        let run = match self {
            Self::Web => token(text, "<>\"'`"),
            Self::File => token(text, "<>\"'`|=()[]{};,"),
        };
        if run.is_empty() {
            return None;
        }
        match self {
            Self::Web => {
                if run.starts_with("http://") || run.starts_with("https://") {
                    return None;
                }
                if !text[run.len()..].trim().is_empty()
                    && !run.contains(['/', '?', '#', '=', '&', '%', '~', '+'])
                {
                    return None;
                }
            }
            Self::File => {
                if run.starts_with(['/', '~'])
                    || run.starts_with("./")
                    || run.starts_with("../")
                    || run.split_once(':').is_some_and(|(head, tail)| {
                        head.starts_with(|c: char| c.is_ascii_alphabetic())
                            && !tail.starts_with(|c: char| c.is_ascii_digit())
                            && head
                                .chars()
                                .all(|c| c.is_ascii_alphanumeric() || "+.-".contains(c))
                    })
                {
                    return None;
                }
                if !text[run.len()..].trim().is_empty() && !run.contains(['/', '\\', '.', ':']) {
                    return None;
                }
            }
        }
        Some(run)
    }

    fn limit(self) -> usize {
        match self {
            Self::Web => 2048,
            Self::File => 8192,
        }
    }
}

fn token<'a>(text: &'a str, delimiters: &str) -> &'a str {
    &text[..text
        .find(|c: char| c.is_whitespace() || delimiters.contains(c))
        .unwrap_or(text.len())]
}

struct Region {
    y: u16,
    left: u16,
    right: u16,
    text: String,
    columns: Vec<(u16, u16)>,
}

impl Region {
    fn new(y: u16, left: u16, right: u16) -> Self {
        Self {
            y,
            left,
            right,
            text: String::new(),
            columns: Vec::new(),
        }
    }
    fn gap(&self, offset: usize) -> bool {
        self.text
            .get(offset..)
            .is_some_and(|tail| tail.trim().is_empty())
            && self
                .right
                .saturating_sub(self.columns.get(offset).map_or(self.right, |range| range.0))
                <= EDGE_SLACK
    }
    fn margin(&self) -> Option<(usize, u16)> {
        let offset = self.text.find(|c: char| !c.is_whitespace())?;
        Some((offset, self.columns.get(offset)?.0))
    }
}

fn regions(screen: &Screen, rows: &[RowMetadata], y: u16) -> Vec<Region> {
    if rows
        .get(usize::from(y))
        .is_some_and(|row| row.wrap_continuation || row.soft_wrapped)
    {
        return Vec::new();
    }
    let mut result = Vec::new();
    let mut region = Region::new(y, 0, screen.cols);
    let left = usize::from(y) * usize::from(screen.cols);
    for (column, cell) in screen.cells[left..left + usize::from(screen.cols)]
        .iter()
        .enumerate()
    {
        if cell.width == 0 {
            continue;
        }
        let column = column as u16;
        let text = if cell.text.is_empty() {
            " "
        } else {
            &cell.text
        };
        if text.chars().count() == 1 && BORDERS.contains(text) {
            region.right = column;
            result.push(region);
            region = Region::new(y, column + 1, screen.cols);
        } else {
            region.text.push_str(text);
            region.columns.extend(std::iter::repeat_n(
                (column, column + u16::from(cell.width) - 1),
                text.len(),
            ));
        }
    }
    result.push(region);
    result
}

pub(super) fn column_link_at(
    screen: &Screen,
    rows: &[RowMetadata],
    hit: ViewportPoint,
    web: bool,
    files: bool,
) -> Option<LinkRange> {
    let first = usize::from(hit.y).saturating_sub(CHAIN_ROWS);
    let last = (usize::from(hit.y) + CHAIN_ROWS).min(usize::from(screen.rows).saturating_sub(1));
    let supplies = (first..=last)
        .map(|y| regions(screen, rows, y as u16))
        .collect::<Vec<_>>();
    [web.then_some(Kind::Web), files.then_some(Kind::File)]
        .into_iter()
        .flatten()
        .find_map(|kind| {
            let max_start = usize::from(hit.y) - first;
            for (y, row) in supplies.iter().enumerate().take(max_start + 1) {
                for region in row {
                    if !(region.left..region.right).contains(&hit.x) {
                        continue;
                    }
                    if let Some(found) = column_candidate(&supplies[y + 1..], region, kind, hit) {
                        return Some(found);
                    }
                }
            }
            None
        })
}

fn column_candidate(
    below: &[Vec<Region>],
    region: &Region,
    kind: Kind,
    hit: ViewportPoint,
) -> Option<LinkRange> {
    let start = kind.last_start(&region.text)?;
    if !region.gap(start.end) {
        return None;
    }
    let (_, margin) = region.margin()?;
    let mut joined = region.text[start.clone()].to_owned();
    let mut pieces = vec![(region, start)];
    for row in below.iter().take(CHAIN_ROWS) {
        if joined.len() > kind.limit() {
            return None;
        }
        let Some(next) = row
            .iter()
            .find(|part| part.left == region.left && part.right == region.right)
        else {
            break;
        };
        let Some((from, column)) = next.margin() else {
            break;
        };
        if !(margin..=margin.saturating_add(INDENT_SLACK)).contains(&column) {
            break;
        }
        let Some(run) = kind.continuation(&next.text[from..], &joined) else {
            break;
        };
        joined.push_str(run);
        pieces.push((next, from..from + run.len()));
        if !next.gap(from + run.len()) {
            break;
        }
    }
    if pieces.len() < 2 || joined.len() > kind.limit() {
        return None;
    }
    let (range, target) = kind.target(&joined)?;
    let mut offset = 0;
    let mut hits = 0;
    let mut found = None;
    for (piece, span) in pieces {
        let start = range.start.saturating_sub(offset);
        let end = range.end.saturating_sub(offset).min(span.len());
        if start < end {
            hits += 1;
            let left = piece.columns.get(span.start + start)?.0;
            let right = piece.columns.get(span.start + end - 1)?.1;
            if piece.y == hit.y && (left..=right).contains(&hit.x) {
                found = Some(LinkRange {
                    target: target.into(),
                    start: ViewportPoint { x: left, y: hit.y },
                    end: ViewportPoint { x: right, y: hit.y },
                });
            }
        }
        offset += span.len();
    }
    if hits >= 2 {
        found
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Terminal;
    #[test]
    fn hard_wrapped_web_and_file_links_do_not_bleed_into_adjacent_columns() {
        let mut term = Terminal::new(42, 5).unwrap();
        term.feed(
            format!(
                "│ {:<22}│ prose\r\n│ {:<22}│ other\r\n│ {:<22}│ other\r\n│ {:<22}│ other",
                "https://example.com/a", "bc?d=2", "src/main/terminal.rs:", "8:4"
            )
            .as_bytes(),
        )
        .unwrap();
        let screen = term.render().unwrap();
        let hit = |x, y| {
            column_link_at(&screen, &[], ViewportPoint { x, y }, true, true).map(|link| link.target)
        };
        assert_eq!(hit(4, 1).as_deref(), Some("https://example.com/abc?d=2"));
        assert_eq!(hit(2, 3).as_deref(), Some("src/main/terminal.rs:8:4"));
        assert!(hit(27, 1).is_none());
    }
}
