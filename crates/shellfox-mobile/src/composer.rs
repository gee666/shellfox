//! Soft-keyboard text as terminal edits, shared by Android and iOS.
//!
//! The platform keeps a small editor for the keyboard and reports it after
//! every keyboard call: the committed text and the composing (marked) text.
//! Committed text is what the terminal shows; composing text stays a local
//! preedit. When a keyboard rewrites text it already committed (autocorrect,
//! a tapped suggestion, swipe replacement), the terminal gets backspaces back
//! to the first changed character and then the new text. The platform clears
//! its editor after Enter or any key event and then sends a reset.
//!
//! A key or paste that reaches the terminal before that reset moves the
//! terminal's cursor away from the editor's text. Until the reset, appended
//! text still goes through, but nothing is erased: the old text may no
//! longer be where backspaces would reach.
use shellfox_ghostty_vt::key::{validate_text, KeyEvent, KeyModifiers, PhysicalKey};

#[derive(Default)]
pub(crate) struct Composer {
    sent: String,
    detached: bool,
}

impl Composer {
    /// The keyboard's committed text changed to `committed`.
    pub(crate) fn update(&mut self, committed: &str) -> Vec<KeyEvent> {
        if self.detached {
            let mut events = Vec::new();
            if let Some(appended) = committed.strip_prefix(self.sent.as_str()) {
                text(&mut events, appended);
                self.sent = committed.to_owned();
            }
            return events;
        }
        let common = self
            .sent
            .char_indices()
            .zip(committed.chars())
            .find(|((_, a), b)| a != b)
            .map_or_else(|| self.sent.len().min(committed.len()), |((i, _), _)| i);
        // `common` is a char boundary in both strings: both share that prefix.
        let erased = self.sent[common..].chars().count();
        let typed = &committed[common..];
        let mut events = Vec::new();
        for _ in 0..erased {
            press(
                &mut events,
                KeyEvent::press(PhysicalKey::Backspace, KeyModifiers::NONE),
            );
        }
        text(&mut events, typed);
        self.sent = committed.to_owned();
        events
    }
    /// A key or paste went to the terminal outside the editor.
    pub(crate) fn detach(&mut self) {
        self.detached = true;
    }
    /// The platform cleared its editor.
    pub(crate) fn reset(&mut self) {
        *self = Self::default();
    }
}

fn press(events: &mut Vec<KeyEvent>, event: KeyEvent) {
    let release = event.release();
    events.extend([event, release]);
}

/// Text with line breaks and tabs as their keys. Other controls are dropped:
/// a keyboard cannot type them as text.
fn text(events: &mut Vec<KeyEvent>, text: &str) {
    let mut run = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        let key = match c {
            '\r' | '\n' => {
                if c == '\r' && chars.peek() == Some(&'\n') {
                    chars.next();
                }
                Some(PhysicalKey::Enter)
            }
            '\t' => Some(PhysicalKey::Tab),
            c if validate_text(c.encode_utf8(&mut [0; 4])).is_err() => continue,
            c => {
                run.push(c);
                None
            }
        };
        if let Some(key) = key {
            flush(events, &mut run);
            press(events, KeyEvent::press(key, KeyModifiers::NONE));
        }
    }
    flush(events, &mut run);
}

fn flush(events: &mut Vec<KeyEvent>, run: &mut String) {
    if !run.is_empty() {
        events.push(KeyEvent::text(std::mem::take(run)));
    }
}

#[cfg(test)]
#[path = "composer_tests.rs"]
mod tests;
