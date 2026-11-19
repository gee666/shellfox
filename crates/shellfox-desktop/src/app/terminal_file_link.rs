use std::ops::Range;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TerminalFileLink {
    pub path: String,
    pub line: Option<usize>,
    pub column: Option<usize>,
}

fn number(value: &str) -> Option<usize> {
    let value: u64 = value.parse().ok()?;
    (value > 0 && value < (1 << 53))
        .then(|| usize::try_from(value).ok())
        .flatten()
}

fn windows_absolute(path: &str) -> bool {
    matches!(path.as_bytes(), [drive, b':', b'/' | b'\\', ..] if drive.is_ascii_alphabetic())
}

fn disallowed(c: char) -> bool {
    c.is_whitespace() || "<>:\"'`|?*".contains(c)
}

fn suffix(path: &str) -> Option<(&str, &str)> {
    path.rsplit_once(':')
        .filter(|(_, n)| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

fn decode_url_path(value: &str) -> Option<String> {
    let mut decoded = Vec::with_capacity(value.len());
    let mut bytes = value.bytes();
    while let Some(b) = bytes.next() {
        if b == b'%' {
            decoded.push(
                char::from(bytes.next()?).to_digit(16)? as u8 * 16
                    + char::from(bytes.next()?).to_digit(16)? as u8,
            );
        } else {
            decoded.push(b);
        }
    }
    String::from_utf8(decoded).ok()
}

fn file_url_position(hash: &str) -> Option<(usize, Option<usize>)> {
    let hash = hash.strip_prefix(['L', 'l']).unwrap_or(hash);
    let (start, end) = hash
        .split_once('-')
        .map_or((hash, None), |(a, b)| (a, Some(b)));
    if let Some(end) = end {
        let end = end.strip_prefix(['L', 'l']).unwrap_or(end);
        if let Some((line, column)) = end.split_once(['C', 'c']) {
            number(line)?;
            number(column)?;
        } else {
            number(end)?;
        }
    }
    if let Some((line, column)) = start.split_once(['C', 'c', ':']) {
        Some((number(line)?, Some(number(column)?)))
    } else {
        Some((number(start)?, None))
    }
}

pub(super) fn parse(value: &str) -> Option<TerminalFileLink> {
    let value = value.trim();
    if value.is_empty() || value.len() > 8192 || value.chars().any(char::is_control) {
        return None;
    }
    let mut line = None;
    let mut column = None;
    let path = if value
        .get(..5)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("file:"))
    {
        let url = url::Url::parse(value).ok()?;
        if (url
            .host_str()
            .is_some_and(|host| !host.eq_ignore_ascii_case("localhost")))
            || url.query().is_some()
        {
            return None;
        }
        if let Some(fragment) = url.fragment() {
            let (row, col) = file_url_position(fragment)?;
            line = Some(row);
            column = col;
        }
        let mut path = decode_url_path(url.path())?;
        if path.strip_prefix('/').is_some_and(windows_absolute) {
            path.remove(0);
        }
        path
    } else {
        if !windows_absolute(value)
            && value.split_once(':').is_some_and(|(prefix, _)| {
                prefix
                    .as_bytes()
                    .first()
                    .is_some_and(u8::is_ascii_alphabetic)
                    && prefix
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"+.-".contains(&b))
            })
        {
            return None;
        }
        value.to_owned()
    };
    let mut path = path.as_str();
    if let Some((base, last)) = suffix(path) {
        if let Some((base, row)) = suffix(base) {
            if line.is_none() {
                line = Some(number(row)?);
            }
            if column.is_none() {
                column = Some(number(last)?);
            }
            path = base;
        } else {
            if line.is_none() {
                line = Some(number(last)?);
            }
            path = base;
        }
    }
    if path.is_empty() || path.chars().any(char::is_control) {
        return None;
    }
    let explicit = path.starts_with('/')
        || windows_absolute(path)
        || ["~/", "~\\", "./", ".\\", "../", "..\\"]
            .iter()
            .any(|prefix| path.starts_with(prefix));
    if !explicit {
        let trimmed = path.trim_end_matches(['/', '\\']);
        if trimmed.is_empty()
            || trimmed.chars().any(disallowed)
            || trimmed.split(['/', '\\']).any(str::is_empty)
        {
            return None;
        }
        if path.len() == trimmed.len() {
            let base = trimmed.rsplit(['/', '\\']).next()?;
            let (stem, ext) = base.split_once('.')?;
            if stem.is_empty()
                || !ext
                    .as_bytes()
                    .first()
                    .is_some_and(u8::is_ascii_alphanumeric)
                || !ext
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._+-".contains(&b))
            {
                return None;
            }
        }
    }
    Some(TerminalFileLink {
        path: path.into(),
        line,
        column,
    })
}

pub(super) fn file_start(text: &str) -> Option<Range<usize>> {
    let end = text.trim_end().len();
    if end == 0 {
        return None;
    }
    let value = &text[..end];
    let end_mark = value
        .chars()
        .next_back()
        .filter(|ch| matches!(ch, '\'' | '"'));
    let start = if let Some(mark) = end_mark {
        value[..end - 1].rfind(mark)?
    } else if let Some((offset, _)) = value.char_indices().rfind(|&(i, ch)| {
        matches!(ch, '\'' | '"')
            && value[..i]
                .chars()
                .next_back()
                .is_none_or(|previous| !previous.is_alphanumeric() && previous != '_')
    }) {
        offset
    } else {
        value
            .char_indices()
            .rfind(|&(_, ch)| ch.is_whitespace() || "<>`|=()[]{};,│┃║┆┇┊┋╎╏".contains(ch))
            .map_or(0, |(offset, ch)| offset + ch.len_utf8())
    };
    let token = value[start..].trim_matches(['\'', '"']);
    let trimmed = token.trim_end_matches(|ch: char| ch == ':' || ch.is_ascii_digit());
    let relative = token
        .split_once(['/', '\\'])
        .is_some_and(|(head, _)| !head.is_empty() && !head.chars().any(disallowed));
    if parse(token).is_some() || parse(trimmed).is_some() || relative || token.starts_with("file:")
    {
        Some(start..end)
    } else {
        None
    }
}

pub(super) fn file_match(text: &str) -> impl Iterator<Item = (Range<usize>, &str)> {
    let mut offset = 0;
    let end_token = |value: char| {
        value.is_whitespace() || value.is_control() || "<>\"'`|=()[]{};,│┃║┆┇┊┋╎╏".contains(value)
    };
    std::iter::from_fn(move || {
        while offset < text.len() {
            let c = text[offset..].chars().next()?;
            let mut start = offset;
            if matches!(c, '\'' | '"') {
                start += c.len_utf8();
                let tail = &text[start..];
                if let Some(end) = tail
                    .find(c)
                    .filter(|end| !tail[..*end].contains(['\r', '\n']))
                {
                    offset = start + end + c.len_utf8();
                } else {
                    offset = start;
                    continue;
                }
            } else if end_token(c) {
                offset += c.len_utf8();
                continue;
            } else {
                offset = start + text[start..].find(end_token).unwrap_or(text.len() - start);
            }
            let quoted = matches!(c, '\'' | '"');
            let end = offset - if quoted { c.len_utf8() } else { 0 };
            let target = text[start..end].trim_end_matches(['.', '!', '?']);
            if parse(target).is_some() {
                return Some((start..start + target.len(), target));
            }
        }
        None
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn desktop_file_links_keep_source_position_and_ignore_external_schemes() {
        let windows = parse(r"C:\app\menu.rs:34:12").unwrap();
        assert_eq!(windows.path, r"C:\app\menu.rs");
        assert_eq!((windows.line, windows.column), (Some(34), Some(12)));
        let url = parse("file:///C:/my%20folder/menu.rs#L8C19-L10").unwrap();
        assert_eq!(url.path, "C:/my folder/menu.rs");
        assert_eq!((url.line, url.column), (Some(8), Some(19)));
        for candidate in [
            "https://example.com/file.rs",
            "file://server/home/a.txt",
            "./file.rs:0",
            "malicious:open",
        ] {
            assert!(parse(candidate).is_none(), "{candidate}");
        }
    }
    #[test]
    fn quote_and_terminal_delimiters_keep_only_the_file_target() {
        let line = "┆ \"/home/name with space.rs:3\" │ src/main.rs:8:4, end";
        let links = file_match(line)
            .map(|(_, target)| target)
            .collect::<Vec<_>>();
        assert_eq!(links, ["/home/name with space.rs:3", "src/main.rs:8:4"]);
    }
}
