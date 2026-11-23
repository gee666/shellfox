//! Line endings in the file editor. Existing bytes are never rewritten; only
//! newlines that are typed or pasted take the file's style. In a mixed file the
//! first line break decides. A file without a line break uses LF. A lone CR is
//! kept as content, and a pasted lone CR becomes a newline.
use std::borrow::Cow;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum LineEnding {
    #[default]
    Lf,
    CrLf,
}

impl LineEnding {
    pub(super) fn detect(text: &str) -> Self {
        match text.find('\n') {
            Some(index) if text[..index].ends_with('\r') => Self::CrLf,
            _ => Self::Lf,
        }
    }

    pub(super) fn apply(self, inserted: &str) -> Cow<'_, str> {
        if !inserted.contains(['\r', '\n']) {
            return Cow::Borrowed(inserted);
        }
        let lf = inserted.replace("\r\n", "\n").replace('\r', "\n");
        Cow::Owned(match self {
            Self::Lf => lf,
            Self::CrLf => lf.replace('\n', "\r\n"),
        })
    }
}

/// Moves an offset that falls inside a CRLF pair to the start of that pair.
pub(super) fn boundary(text: &str, index: usize) -> usize {
    let bytes = text.as_bytes();
    if index > 0 && bytes.get(index - 1) == Some(&b'\r') && bytes.get(index) == Some(&b'\n') {
        index - 1
    } else {
        index
    }
}

pub(super) fn previous(text: &str, cursor: usize) -> usize {
    if text[..cursor].ends_with("\r\n") {
        return cursor - 2;
    }
    text[..cursor]
        .char_indices()
        .next_back()
        .map_or(0, |(index, _)| index)
}

pub(super) fn next(text: &str, cursor: usize) -> usize {
    if text[cursor..].starts_with("\r\n") {
        return cursor + 2;
    }
    text[cursor..]
        .chars()
        .next()
        .map_or(cursor, |ch| cursor + ch.len_utf8())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_line_break_decides_and_plain_text_defaults_to_lf() {
        assert_eq!(LineEnding::detect("a\r\nb\nc"), LineEnding::CrLf);
        assert_eq!(LineEnding::detect("a\nb\r\nc\r\n"), LineEnding::Lf);
        assert_eq!(LineEnding::detect("one line"), LineEnding::Lf);
        assert_eq!(LineEnding::detect("old\rmac"), LineEnding::Lf);
    }

    #[test]
    fn inserted_breaks_take_the_file_style() {
        assert_eq!(LineEnding::CrLf.apply("x\ny\r\nz\r"), "x\r\ny\r\nz\r\n");
        assert_eq!(LineEnding::Lf.apply("x\r\ny\rz\n"), "x\ny\nz\n");
        assert!(matches!(
            LineEnding::CrLf.apply("plain"),
            Cow::Borrowed("plain")
        ));
    }

    #[test]
    fn edits_in_a_mixed_file_keep_every_existing_break() {
        let mut text = "a\r\nb\nc".to_owned();
        let ending = LineEnding::detect(&text);
        let end_of_b = 4;
        super::super::buffer::replace_range(&mut text, end_of_b..end_of_b, &ending.apply("\n"), 64)
            .unwrap();
        assert_eq!(text, "a\r\nb\r\n\nc");
        let pasted = ending.apply("x\ny");
        let end = text.len();
        super::super::buffer::replace_range(&mut text, end..end, &pasted, 64).unwrap();
        assert_eq!(text, "a\r\nb\r\n\ncx\r\ny");
        // Backspace at the start of "b" and Delete at the end of "a" remove the whole pair.
        assert_eq!(&text[previous(&text, 3)..3], "\r\n");
        assert_eq!(&text[1..next(&text, 1)], "\r\n");
    }

    #[test]
    fn cursor_steps_and_boundaries_treat_crlf_as_one_break() {
        let text = "a\r\nb";
        assert_eq!(boundary(text, 2), 1);
        assert_eq!(boundary(text, 1), 1);
        assert_eq!(boundary(text, 3), 3);
        assert_eq!(boundary("a\rb", 2), 2);
        assert_eq!(next(text, 1), 3);
        assert_eq!(previous(text, 3), 1);
        assert_eq!(next("a\nb", 1), 2);
        assert_eq!(previous("a\rb", 2), 1);
    }
}
