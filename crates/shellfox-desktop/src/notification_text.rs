use gpui::{Pixels, SharedString, TextStyle, Window, px};
use std::{cell::RefCell, collections::HashMap};
use unicode_segmentation::UnicodeSegmentation;

#[derive(Default)]
pub(super) struct ToastTextCache(RefCell<HashMap<u64, (Pixels, SharedString)>>);

impl ToastTextCache {
    pub(super) fn forget(&self, id: u64) {
        self.0.borrow_mut().remove(&id);
    }

    pub(super) fn text(
        &self,
        id: u64,
        window: &Window,
        message: &str,
        width: Pixels,
    ) -> SharedString {
        if let Some((_, text)) = self.0.borrow().get(&id).filter(|entry| entry.0 == width) {
            return text.clone();
        }
        let text: SharedString =
            wrap(message, |line| shaped_width(window, line, 12.) <= width).into();
        self.0.borrow_mut().insert(id, (width, text.clone()));
        text
    }
}

pub(super) fn content_width(window: &Window, toast_width: Pixels, review: bool) -> Pixels {
    // Browser content box: 12 + 8 padding, 2 + 1 border, 8 gap, 28 dismiss.
    let reserved = if review {
        px(28.) + shaped_width(window, "Review", 11.)
    } else {
        px(0.)
    };
    (toast_width - px(59.) - reserved).max(px(1.))
}

fn shaped_width(window: &Window, line: &str, size: f32) -> Pixels {
    let style = TextStyle {
        font_family: crate::ui_font::family().into(),
        ..Default::default()
    };
    window
        .text_system()
        .shape_line(
            line.to_owned().into(),
            px(size),
            &[style.to_run(line.len())],
            None,
        )
        .width
}

fn wrap(message: &str, fits: impl Fn(&str) -> bool) -> String {
    let mut lines = Vec::new();
    let mut line = String::new();
    // Measure shaped lines without trailing break whitespace, as Chromium does.
    // GPUI's normal wrapper can count that space at the end of a candidate.
    for word in message.split_ascii_whitespace() {
        if !line.is_empty() {
            let end = line.len();
            line.push(' ');
            line.push_str(word);
            if fits(&line) {
                continue;
            }
            line.truncate(end);
            lines.push(std::mem::take(&mut line));
        }
        if fits(word) {
            line.push_str(word);
        } else {
            // HTML overflow-wrap:anywhere, without splitting a grapheme.
            for cluster in word.graphemes(true) {
                let end = line.len();
                line.push_str(cluster);
                if end != 0 && !fits(&line) {
                    line.truncate(end);
                    lines.push(std::mem::take(&mut line));
                    line.push_str(cluster);
                }
            }
        }
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::wrap;
    use unicode_segmentation::UnicodeSegmentation;

    #[test]
    fn wrap_keeps_a_last_word_that_fits_without_the_breaking_space() {
        assert_eq!(
            wrap("this  fits    afterward", |line| line.len() <= 9),
            "this fits\nafterward"
        );
    }

    #[test]
    fn oversized_words_split_at_grapheme_boundaries() {
        assert_eq!(
            wrap("a👩‍💻c", |line| line.graphemes(true).count() <= 1),
            "a\n👩‍💻\nc"
        );
    }
}
