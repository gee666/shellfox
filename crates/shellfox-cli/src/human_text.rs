//! Display-cell text rules and tables from src/cli/terminal-text.ts and table.ts.
use std::io::IsTerminal;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// Untrusted metadata is shown visibly, never run as terminal controls.
pub fn escape(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    for c in value.chars() {
        if matches!(c, '\0'..='\x1f' | '\x7f'..='\u{9f}' | '\u{2028}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        {
            output.push_str(&format!("\\u{:04x}", c as u32));
        } else {
            output.push(c);
        }
    }
    output
}

#[derive(Clone, Copy)]
pub struct Style(pub bool);
impl Style {
    fn wrap(self, code: &str, text: &str) -> String {
        if self.0 {
            format!("\x1b[{code}m{text}\x1b[0m")
        } else {
            text.to_owned()
        }
    }
    pub fn accent(self, text: &str) -> String {
        self.wrap("38;2;236;72;153", text)
    }
    pub fn dim(self, text: &str) -> String {
        self.wrap("2", text)
    }
    pub fn red(self, text: &str) -> String {
        self.wrap("31", text)
    }
    pub fn green(self, text: &str) -> String {
        self.wrap("32", text)
    }
}

/// Same rule as 0.9: a TTY, no NO_COLOR and not TERM=dumb.
pub fn color(stdout: bool) -> bool {
    let tty = if stdout {
        std::io::stdout().is_terminal()
    } else {
        std::io::stderr().is_terminal()
    };
    tty && std::env::var_os("NO_COLOR").is_none()
        && std::env::var("TERM").map_or(true, |term| term != "dumb")
}
/// Columns of a terminal stdout; unbounded when redirected.
pub fn terminal_width() -> usize {
    if !std::io::stdout().is_terminal() {
        return usize::MAX;
    }
    crossterm::terminal::size().map_or(usize::MAX, |(cols, _)| usize::from(cols).max(1))
}

fn strip_sgr(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' && chars.peek() == Some(&'[') {
            chars.next();
            for c in chars.by_ref() {
                if c.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            output.push(c);
        }
    }
    output
}
fn grapheme_width(grapheme: &str) -> usize {
    grapheme.width()
}
pub fn text_width(text: &str) -> usize {
    strip_sgr(text).graphemes(true).map(grapheme_width).sum()
}
pub fn pad(text: &str, width: usize) -> String {
    format!(
        "{text}{}",
        " ".repeat(width.saturating_sub(text_width(text)))
    )
}

/// Wraps by display cells, keeping graphemes and SGR colors intact.
pub fn wrap(text: &str, width: usize) -> String {
    if width == usize::MAX {
        return text.to_owned();
    }
    let width = width.max(1);
    let mut lines = Vec::new();
    for line in text.split('\n') {
        let (mut current, mut used) = (String::new(), 0);
        let mut rest = line;
        while !rest.is_empty() {
            if let Some(after) = rest.strip_prefix("\x1b[") {
                let end = after.find(|c: char| !(c.is_ascii_digit() || c == ';'));
                if let Some(end) = end.filter(|end| after[*end..].starts_with('m')) {
                    current.push_str(&rest[..end + 3]);
                    rest = &after[end + 1..];
                    continue;
                }
            }
            let next = rest
                .find("\x1b[")
                .filter(|at| *at > 0)
                .unwrap_or(rest.len());
            for grapheme in rest[..next].graphemes(true) {
                let size = grapheme_width(grapheme);
                if used > 0 && used + size > width {
                    lines.push(std::mem::take(&mut current));
                    used = 0;
                }
                current.push_str(grapheme);
                used += size;
            }
            rest = &rest[next..];
        }
        lines.push(current);
    }
    lines.join("\n")
}

/// Columns when they fit; narrow output uses complete labeled records.
pub fn table(
    headers: &[String],
    rows: &[Vec<String>],
    style: Style,
    width: usize,
    value_style: &dyn Fn(&str) -> String,
) -> String {
    let widths: Vec<usize> = headers
        .iter()
        .enumerate()
        .map(|(index, header)| {
            rows.iter().fold(text_width(header), |size, row| {
                size.max(text_width(row.get(index).map_or("", String::as_str)))
            })
        })
        .collect();
    let total = widths.iter().sum::<usize>() + headers.len().saturating_sub(1) * 2;
    if total <= width {
        let line = |row: &[String], heading: bool| {
            (0..headers.len())
                .map(|index| {
                    let value = row.get(index).map_or("", String::as_str);
                    let text = if heading {
                        style.accent(value)
                    } else {
                        value_style(value)
                    };
                    if index + 1 == headers.len() {
                        text
                    } else {
                        pad(&text, widths[index])
                    }
                })
                .collect::<Vec<_>>()
                .join("  ")
        };
        let mut lines = vec![line(headers, true)];
        lines.extend(rows.iter().map(|row| line(row, false)));
        return lines.join("\n");
    }
    if rows.is_empty() {
        return wrap(&headers.join("  "), width);
    }
    let label = headers.iter().map(|h| text_width(h)).max().unwrap_or(0);
    rows.iter()
        .map(|row| {
            headers
                .iter()
                .enumerate()
                .map(|(index, header)| {
                    let value = row.get(index).map_or("", String::as_str);
                    let prefix = format!("{}  ", pad(header, label));
                    if text_width(&prefix) + text_width(value) > width {
                        format!(
                            "{}\n{}",
                            wrap(&style.accent(header), width),
                            value_style(&wrap(value, width))
                        )
                    } else {
                        format!("{}{}", style.accent(&prefix), value_style(value))
                    }
                })
                .collect::<Vec<_>>()
                .join("\n")
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn controls_are_visible_and_wide_text_wraps_by_cells() {
        assert_eq!(escape("a\x1b[6n\u{202e}"), "a\\u001b[6n\\u202e");
        assert_eq!(text_width("雪🙂a"), 5);
        assert_eq!(wrap("abcdef", 4), "abcd\nef");
        assert_eq!(wrap("\x1b[2mabc\x1b[0m", 2), "\x1b[2mab\nc\x1b[0m");
        assert_eq!(wrap("雪雪", 3), "雪\n雪");
    }
    #[test]
    fn tables_fall_back_to_labeled_records_when_narrow() {
        let headers = vec!["NAME".to_owned(), "ID".to_owned()];
        let rows = vec![vec!["api".to_owned(), "1234".to_owned()]];
        let plain = |v: &str| v.to_owned();
        assert_eq!(
            table(&headers, &rows, Style(false), 80, &plain),
            "NAME  ID\napi   1234"
        );
        assert_eq!(
            table(&headers, &rows, Style(false), 9, &plain),
            "NAME  api\nID\n1234"
        );
    }
}
