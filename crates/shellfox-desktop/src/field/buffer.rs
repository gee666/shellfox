use std::ops::Range;

pub(super) fn text_limit(multiline: bool, asset_editor: bool) -> usize {
    if multiline && asset_editor {
        shellfox_files::TRANSFER_TEXT_BYTES as usize
    } else if multiline {
        shellfox_files::TEXT_BYTES
    } else {
        8192
    }
}

pub(super) fn byte_offset(text: &str, utf16: usize) -> Option<usize> {
    let mut offset = 0;
    for (index, ch) in text.char_indices() {
        if offset == utf16 {
            return Some(index);
        }
        if offset > utf16 {
            return None;
        }
        offset += ch.len_utf16();
    }
    (offset == utf16).then_some(text.len())
}

pub(super) fn replace_range(
    text: &mut String,
    range: Range<usize>,
    inserted: &str,
    limit: usize,
) -> Option<Range<usize>> {
    if range.start > range.end
        || !text.is_char_boundary(range.start)
        || !text.is_char_boundary(range.end)
    {
        return None;
    }
    if text.len() - range.len() + inserted.len() > limit {
        return None;
    }
    let selection = range.start..range.start + inserted.len();
    text.replace_range(range, inserted);
    Some(selection)
}

pub(super) fn display_text(text: &str, secret: bool) -> String {
    if secret {
        "•".repeat(text.chars().count())
    } else {
        text.to_owned()
    }
}
pub(super) fn display_index(text: &str, index: usize, secret: bool) -> usize {
    if secret {
        text[..index].chars().count() * "•".len()
    } else {
        index
    }
}
pub(super) fn source_index(text: &str, index: usize, secret: bool) -> usize {
    if secret {
        text.char_indices()
            .nth(index / "•".len())
            .map_or(text.len(), |(index, _)| index)
    } else {
        index
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_explicit_asset_editor_accepts_eight_mib_and_failed_edits_leave_text_unchanged() {
        assert_eq!(text_limit(false, true), 8192);
        assert_eq!(text_limit(true, false), 128 * 1024);
        assert_eq!(text_limit(true, true), 8 * 1024 * 1024);
        let mut text = "x".repeat(text_limit(true, true) - 7);
        let start = text.len();
        assert!(replace_range(&mut text, start..start, "雪😀", text_limit(true, true)).is_some());
        assert_eq!(text.len(), text_limit(true, true));
        assert!(replace_range(&mut text, 0..0, "x", text_limit(true, true)).is_none());
        assert_eq!(text.len(), text_limit(true, true));
        assert!(
            replace_range(
                &mut text,
                text_limit(true, true) - 5..text_limit(true, true) - 4,
                "x",
                text_limit(true, true)
            )
            .is_none()
        );
    }
    #[test]
    fn password_layout_never_contains_plaintext_and_keeps_unicode_indices() {
        let text = "a😀界";
        assert_eq!(display_text(text, true), "•••");
        for source in [0, 1, 5, 8] {
            let display = display_index(text, source, true);
            assert_eq!(source_index(text, display, true), source);
        }
        assert_eq!(source_index(text, 999, true), text.len());
    }
    #[test]
    fn repeated_non_bmp_native_replacements_keep_byte_boundaries() {
        let mut text = "a😀b".to_owned();
        for replacement in ["界😀", "𐐀", "😀😀", "z"] {
            let start = byte_offset(&text, 1).unwrap();
            let end = text.len() - 1;
            let end_utf16 = text[..end].encode_utf16().count();
            let end = byte_offset(&text, end_utf16).unwrap();
            let selection = replace_range(&mut text, start..end, replacement, 1024).unwrap();
            assert!(text.is_char_boundary(selection.start));
            assert!(text.is_char_boundary(selection.end));
            assert_eq!(text, format!("a{replacement}b"));
        }
    }
    #[test]
    fn invalid_ranges_and_size_fail_without_mutation() {
        let mut text = "a😀b".to_owned();
        let original = text.clone();
        assert!(replace_range(&mut text, 2..3, "x", 1024).is_none());
        assert!(replace_range(&mut text, 0..99, "x", 1024).is_none());
        assert!(replace_range(&mut text, 0..0, "xxxxxxxx", 6).is_none());
        assert_eq!(text, original);
    }
}
