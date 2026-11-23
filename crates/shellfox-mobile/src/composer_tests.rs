//! Keyboard call sequences recorded from Gboard/Samsung/iOS behavior, applied
//! to an editor that behaves like Android's BaseInputConnection. The bytes go
//! through Ghostty into a line editor that acts like a shell prompt.
use super::*;
use shellfox_ghostty_vt::Terminal;
use std::ops::Range;

/// Android's editable: text, cursor and composing span, in chars.
#[derive(Default)]
struct Editor {
    text: Vec<char>,
    cursor: usize,
    composing: Option<Range<usize>>,
}
impl Editor {
    fn replace(&mut self, range: Range<usize>, text: &str) -> usize {
        let new: Vec<char> = text.chars().collect();
        let end = range.start + new.len();
        self.text.splice(range, new);
        end
    }
    fn region(&self) -> Range<usize> {
        self.composing.clone().unwrap_or(self.cursor..self.cursor)
    }
    fn set_composing_text(&mut self, text: &str) {
        let range = self.region();
        let start = range.start;
        self.cursor = self.replace(range, text);
        self.composing = (!text.is_empty()).then(|| start..self.cursor);
    }
    fn commit_text(&mut self, text: &str) {
        let range = self.region();
        self.cursor = self.replace(range, text);
        self.composing = None;
    }
    fn set_composing_region(&mut self, start: usize, end: usize) {
        self.composing = (start < end).then_some(start..end);
    }
    fn finish_composing_text(&mut self) {
        self.composing = None;
    }
    fn delete_surrounding_text(&mut self, before: usize, after: usize) {
        let end = (self.cursor + after).min(self.text.len());
        let start = self.cursor.saturating_sub(before);
        self.text.drain(self.cursor..end);
        self.text.drain(start..self.cursor);
        self.cursor = start;
        self.composing = None;
    }
    fn committed(&self) -> String {
        let range = self.composing.clone().unwrap_or(0..0);
        self.text
            .iter()
            .enumerate()
            .filter(|(i, _)| !range.contains(i))
            .map(|(_, c)| c)
            .collect()
    }
    fn clear(&mut self) {
        *self = Self::default();
    }
}

/// Keyboard, composer, Ghostty and a shell line, wired like the app.
struct Session {
    editor: Editor,
    composer: Composer,
    terminal: Terminal,
    line: Vec<char>,
    submitted: Vec<String>,
}
impl Session {
    fn new() -> Self {
        Self {
            editor: Editor::default(),
            composer: Composer::default(),
            terminal: Terminal::new(80, 24).unwrap(),
            line: Vec::new(),
            submitted: Vec::new(),
        }
    }
    /// One keyboard call, then the snapshot the platform reports.
    fn ime(&mut self, call: impl FnOnce(&mut Editor)) {
        call(&mut self.editor);
        let committed = self.editor.committed();
        let events = self.composer.update(&committed);
        self.send(events);
        // Enter ends the line: the platform clears its editor.
        if committed.contains('\n') {
            self.editor.clear();
            self.composer.reset();
        }
    }
    /// A key event (Gboard Backspace on an empty editor, the key bar).
    fn key(&mut self, physical: PhysicalKey) {
        self.editor.clear();
        self.composer.reset();
        let event = KeyEvent::press(physical, KeyModifiers::NONE);
        let release = event.release();
        self.send(vec![event, release]);
    }
    fn send(&mut self, events: Vec<KeyEvent>) {
        for event in events {
            for byte in String::from_utf8(self.terminal.encode_key_event(&event).unwrap())
                .unwrap()
                .chars()
            {
                match byte {
                    '\x7f' => drop(self.line.pop()),
                    '\r' => self.submitted.push(self.line.drain(..).collect()),
                    c if c.is_control() => panic!("unexpected control {c:?}"),
                    c => self.line.push(c),
                }
            }
        }
    }
    fn line(&self) -> String {
        self.line.iter().collect()
    }
    fn preedit(&self) -> String {
        let range = self.editor.composing.clone().unwrap_or(0..0);
        self.editor.text[range].iter().collect()
    }
}

#[test]
fn composing_text_stays_local_until_committed() {
    let mut s = Session::new();
    for prefix in ["g", "gi", "git"] {
        s.ime(|e| e.set_composing_text(prefix));
        assert_eq!(s.line(), "");
        assert_eq!(s.preedit(), prefix);
    }
    s.ime(|e| e.commit_text("git "));
    assert_eq!(s.line(), "git ");
    // A late finish after commit must not type the word again.
    s.ime(Editor::finish_composing_text);
    assert_eq!(s.line(), "git ");
}

#[test]
fn gboard_autocorrect_replaces_the_previous_word_after_space() {
    let mut s = Session::new();
    s.ime(|e| e.set_composing_text("teh"));
    // Gboard commits the correction with the space.
    s.ime(|e| e.commit_text("the "));
    assert_eq!(s.line(), "the ");
    // Typing "sl" then space: Gboard autocorrects an already committed word
    // by re-composing it (setComposingRegion) and committing the fix.
    s.ime(|e| e.commit_text("sl "));
    assert_eq!(s.line(), "the sl ");
    s.ime(|e| e.set_composing_region(4, 6));
    s.ime(|e| e.set_composing_text("ls"));
    s.ime(Editor::finish_composing_text);
    assert_eq!(s.line(), "the ls ");
}

#[test]
fn tapping_a_suggestion_replaces_a_word_mid_line() {
    let mut s = Session::new();
    s.ime(|e| e.commit_text("git stauts --short"));
    assert_eq!(s.line(), "git stauts --short");
    // The user taps "stauts"; the strip offers "status".
    s.ime(|e| e.set_composing_region(4, 10));
    assert_eq!(s.preedit(), "stauts");
    s.ime(|e| e.commit_text("status"));
    assert_eq!(s.line(), "git status --short");
    s.key(PhysicalKey::Enter);
    assert_eq!(s.submitted, ["git status --short"]);
}

#[test]
fn samsung_replaces_with_delete_surrounding_then_commit() {
    let mut s = Session::new();
    s.ime(|e| e.commit_text("cd projetcs"));
    s.ime(|e| e.delete_surrounding_text(8, 0));
    s.ime(|e| e.commit_text("projects"));
    assert_eq!(s.line(), "cd projects");
}

#[test]
fn swipe_typing_a_word_then_replacing_it_from_the_strip() {
    let mut s = Session::new();
    s.ime(|e| e.set_composing_text("hello"));
    s.ime(Editor::finish_composing_text);
    // The next swipe adds its own space.
    s.ime(|e| e.commit_text(" "));
    s.ime(|e| e.set_composing_text("word"));
    s.ime(Editor::finish_composing_text);
    assert_eq!(s.line(), "hello word");
    // Picking "world" from the strip re-composes the swiped word.
    s.ime(|e| e.set_composing_region(6, 10));
    s.ime(|e| e.commit_text("world"));
    assert_eq!(s.line(), "hello world");
}

#[test]
fn voice_input_commits_only_the_final_phrase() {
    let mut s = Session::new();
    for partial in ["echo", "echo hell", "echo hello", "echo hello wor", "echo hello world"] {
        s.ime(|e| e.set_composing_text(partial));
        assert_eq!(s.line(), "");
    }
    s.ime(|e| e.commit_text("echo hello world"));
    assert_eq!(s.line(), "echo hello world");
}

#[test]
fn emoji_and_backspace_through_committed_and_preedit_text() {
    let mut s = Session::new();
    s.ime(|e| e.commit_text("ok 👍🏽"));
    assert_eq!(s.line(), "ok 👍🏽");
    // Deleting the emoji removes both of its code points.
    s.ime(|e| e.delete_surrounding_text(2, 0));
    assert_eq!(s.line(), "ok ");
    s.ime(|e| e.set_composing_text("wo"));
    // Backspace inside the preedit edits only the preedit.
    s.ime(|e| e.set_composing_text("w"));
    s.ime(|e| e.set_composing_text(""));
    assert_eq!((s.line(), s.preedit()), ("ok ".into(), String::new()));
    // Then through committed text, one character at a time.
    s.ime(|e| e.delete_surrounding_text(1, 0));
    s.ime(|e| e.delete_surrounding_text(1, 0));
    assert_eq!(s.line(), "o");
    // With an empty editor the keyboard sends Backspace keys.
    s.key(PhysicalKey::Backspace);
    assert_eq!(s.line(), "");
}

#[test]
fn enter_in_the_middle_of_composition_commits_then_submits() {
    let mut s = Session::new();
    s.ime(|e| e.commit_text("echo "));
    s.ime(|e| e.set_composing_text("hel"));
    // Gboard's Enter: finish the composition, then the Enter key.
    s.ime(Editor::finish_composing_text);
    s.key(PhysicalKey::Enter);
    assert_eq!(s.submitted, ["echo hel"]);
    // Some keyboards commit a newline instead.
    s.ime(|e| e.set_composing_text("ls"));
    s.ime(|e| e.commit_text("ls\n"));
    assert_eq!(s.submitted, ["echo hel", "ls"]);
    // The next word starts from a clean editor, not the old line.
    s.ime(|e| e.commit_text("pwd"));
    assert_eq!(s.line(), "pwd");
}

#[test]
fn cjk_and_korean_composition_send_only_the_result() {
    let mut s = Session::new();
    for step in ["n", "ni", "nih", "niha", "nihao"] {
        s.ime(|e| e.set_composing_text(step));
    }
    s.ime(|e| e.commit_text("你好"));
    for step in ["ㅎ", "하", "한"] {
        s.ime(|e| e.set_composing_text(step));
    }
    s.ime(|e| e.commit_text("한"));
    assert_eq!(s.line(), "你好한");
}

#[test]
fn tabs_and_controls_in_committed_text() {
    let mut composer = Composer::default();
    let events = composer.update("a\tb\u{7}c");
    let kinds: Vec<_> = events.iter().map(|e| (e.physical, e.text.as_str())).collect();
    assert_eq!(
        kinds,
        [
            (PhysicalKey::Unidentified, "a"),
            (PhysicalKey::Tab, ""),
            (PhysicalKey::Tab, ""),
            (PhysicalKey::Unidentified, "bc"),
        ]
    );
    assert!(composer.update("a\tb\u{7}c").is_empty());
}
