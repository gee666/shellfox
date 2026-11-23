//! Activity policy only. The service owns clocks, persistence and process handles.
pub(crate) mod service;
pub(crate) mod hooks;
pub(crate) mod native;
mod inspection;
mod matching;
mod process;
mod session;
mod status;
pub use inspection::*;
pub use process::*;
pub use session::*;
pub use status::*;

pub const TERMINAL_QUIET_MS: u64 = 2500;
pub const TERMINAL_ECHO_MS: u64 = 250;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TerminalOutputActivity {
    pub busy_changed: Option<bool>,
    pub meaningful_at_ms: Option<u64>,
}
#[derive(Default)]
pub struct TerminalActivity {
    busy: bool,
    input_at: Option<u64>,
    quiet_at: Option<u64>,
    last_meaningful_output_ms: Option<u64>,
    pending: Vec<u8>,
}
impl TerminalActivity {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn is_busy(&self) -> bool {
        self.busy
    }
    /// When [`Self::tick`] next ends a busy period, if one is pending.
    pub fn quiet_at(&self) -> Option<u64> {
        self.quiet_at
    }
    /// Call before a meaningful user write, so synchronous echo is suppressed.
    pub fn input(&mut self, now_ms: u64) {
        self.input_at = Some(now_ms);
    }
    pub fn last_meaningful_output_ms(&self) -> Option<u64> {
        self.last_meaningful_output_ms
    }
    /// Transitions only. Input echo and idle cursor/sync traffic do not extend work.
    pub fn output(&mut self, data: &[u8], now_ms: u64) -> Option<bool> {
        self.output_activity(data, now_ms).busy_changed
    }
    /// The same parser reports meaningful output even while already busy.
    pub fn output_activity(&mut self, data: &[u8], now_ms: u64) -> TerminalOutputActivity {
        let previous = std::mem::take(&mut self.pending);
        let length = previous.len() + data.len();
        let at = |i: usize| {
            if i < previous.len() {
                previous[i]
            } else {
                data[i - previous.len()]
            }
        };
        let mut end = length;
        if let Some(escape) = (0..length).rev().find(|i| at(*i) == 27) {
            let partial = escape + 1 == length
                || escape + 2 <= length
                    && at(escape + 1) == b'['
                    && length - escape <= 18
                    && (escape + 2..length)
                        .all(|i| at(i).is_ascii_digit() || b"?;".contains(&at(i)));
            if partial {
                self.pending.extend((escape..length).map(at));
                end = escape;
            }
        }
        let mut i = 0;
        let mut meaningful = false;
        while i < end {
            if at(i) == 27 && i + 1 < end {
                if b"78".contains(&at(i + 1)) {
                    i += 2;
                    continue;
                }
                if at(i + 1) == b'[' {
                    let mut j = i + 2;
                    while j < end && (at(j).is_ascii_digit() || at(j) == b';') {
                        j += 1;
                    }
                    if j < end && b"HfABCDEFGdsu".contains(&at(j)) {
                        i = j + 1;
                        continue;
                    }
                    let modes: [&[u8]; 4] =
                        [b"\x1b[?25h", b"\x1b[?25l", b"\x1b[?2026h", b"\x1b[?2026l"];
                    if let Some(mode) = modes.iter().find(|mode| {
                        i + mode.len() <= end
                            && mode
                                .iter()
                                .enumerate()
                                .all(|(offset, byte)| at(i + offset) == *byte)
                    }) {
                        i += mode.len();
                        continue;
                    }
                }
            }
            meaningful = true;
            i += 1;
        }
        if !meaningful
            || self
                .input_at
                .is_some_and(|input| now_ms.saturating_sub(input) < TERMINAL_ECHO_MS)
        {
            return TerminalOutputActivity::default();
        }
        self.last_meaningful_output_ms = Some(now_ms);
        self.quiet_at = Some(now_ms.saturating_add(TERMINAL_QUIET_MS));
        let busy_changed = if self.busy {
            None
        } else {
            self.busy = true;
            Some(true)
        };
        TerminalOutputActivity {
            busy_changed,
            meaningful_at_ms: Some(now_ms),
        }
    }
    pub fn tick(&mut self, now_ms: u64) -> Option<bool> {
        if self.quiet_at.is_some_and(|deadline| now_ms >= deadline) {
            self.stop()
        } else {
            None
        }
    }
    pub fn stop(&mut self) -> Option<bool> {
        self.quiet_at = None;
        self.pending.clear();
        if self.busy {
            self.busy = false;
            Some(false)
        } else {
            None
        }
    }
}

/// Focus and button-free motion reach TUIs without resuming settled work.
pub fn meaningful_input(data: &[u8], user_initiated: bool) -> bool {
    if !user_initiated || data.is_empty() {
        return false;
    }
    let mut rest = data;
    while !rest.is_empty() {
        if rest.starts_with(b"\x1b[I") || rest.starts_with(b"\x1b[O") {
            rest = &rest[3..];
            continue;
        }
        if rest.starts_with(b"\x1b[M")
            && rest.len() >= 6
            && rest[3] >= 32
            && hover(u32::from(rest[3] - 32))
            && rest[4] > 32
            && rest[5] > 32
        {
            rest = &rest[6..];
            continue;
        }
        if let Some(end) = passive_sgr(rest) {
            rest = &rest[end..];
            continue;
        }
        return true;
    }
    false
}
fn hover(button: u32) -> bool {
    (35..=63).contains(&button) && button & 35 == 35
}
fn passive_sgr(bytes: &[u8]) -> Option<usize> {
    let mut i = 3;
    if !bytes.starts_with(b"\x1b[<") {
        return None;
    }
    let start = i;
    let mut button = 0u32;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        button = button
            .checked_mul(10)?
            .checked_add(u32::from(bytes[i] - b'0'))?;
        i += 1;
    }
    if i == start || !hover(button) {
        return None;
    }
    for _ in 0..2 {
        if bytes.get(i) != Some(&b';') {
            return None;
        }
        i += 1;
        let start = i;
        while bytes.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
        }
        if i == start {
            return None;
        }
    }
    (bytes.get(i) == Some(&b'M')).then_some(i + 1)
}

/// Outer None is a legacy client; Some(None) means input captured before settling.
pub fn should_unsettle(current: Option<&str>, observed: Option<Option<&str>>) -> bool {
    current.is_some() && observed.is_none_or(|marker| marker == current)
}
