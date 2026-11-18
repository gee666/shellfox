//! Passive display filter, ported from src/cli/terminal-queries.ts.
//! This frames controls only. The host runtime remains the terminal parser.
use crate::{
    query_rules as rules,
    vt_bytes::{Decoder, Token},
};

const MAX_CONTROL: usize = 4096;
#[derive(Clone, Copy, Default, PartialEq)]
enum State {
    #[default]
    Text,
    Escape,
    Csi,
    Osc,
    Dcs,
    OtherString,
}

#[derive(Default)]
struct Controls {
    state: State,
    pending: Vec<u8>,
    header: usize,
    historical: bool,
    escape: bool,
    passthrough: bool,
    discard: bool,
}

#[derive(Default)]
pub struct QueryFilter {
    decoder: Decoder,
    controls: Controls,
}
impl QueryFilter {
    pub fn reset(&mut self) {
        *self = Self::default();
    }
    pub fn push(&mut self, bytes: &[u8], historical: bool) -> Vec<u8> {
        let mut output = Vec::with_capacity(bytes.len());
        for byte in bytes {
            self.decoder.push(*byte, historical, |token| {
                self.controls.accept(token, &mut output)
            });
        }
        output
    }
}

impl Controls {
    fn reset(&mut self) {
        *self = Self::default();
    }
    fn start(&mut self, state: State, token: &Token) {
        self.reset();
        self.state = state;
        self.pending.extend_from_slice(token.bytes());
        self.header = token.bytes().len();
        self.historical = token.historical;
    }
    fn accept(&mut self, token: Token, output: &mut Vec<u8>) {
        let code = token.control;
        if self.state != State::Text && matches!(code, Some(0x18 | 0x1a)) {
            self.reset();
            output.extend_from_slice(token.bytes());
            return;
        }
        if self.state == State::Text {
            let state = match code {
                Some(0x1b) => State::Escape,
                Some(0x9b) => State::Csi,
                Some(0x9d) => State::Osc,
                Some(0x90) => State::Dcs,
                Some(0x98 | 0x9e | 0x9f) => State::OtherString,
                _ => {
                    output.extend_from_slice(token.bytes());
                    return;
                }
            };
            self.start(state, &token);
            return;
        }
        if matches!(self.state, State::Escape | State::Csi) {
            if code == Some(0x1b) {
                self.start(State::Escape, &token);
                return;
            }
            if matches!(code, Some(0..=0x1f | 0x7f)) {
                output.extend_from_slice(token.bytes());
                return;
            }
            if code == Some(0x9b) {
                self.start(State::Csi, &token);
                return;
            }
            if !self.discard {
                self.pending.extend_from_slice(token.bytes());
            }
            if self.state == State::Escape {
                self.state = match code {
                    Some(b'[') => State::Csi,
                    Some(b']') => State::Osc,
                    Some(b'P') => State::Dcs,
                    Some(b'X' | b'^' | b'_') => State::OtherString,
                    _ => {
                        output.extend_from_slice(&self.pending);
                        self.reset();
                        return;
                    }
                };
                self.header = self.pending.len();
            } else if matches!(code, Some(b'@'..=b'~')) {
                if !self.discard {
                    output.extend_from_slice(&rules::csi(&self.pending, self.header));
                }
                self.reset();
            } else if self.pending.len() > MAX_CONTROL {
                // Unknown overlong CSI can still end in a query. Do not leak it.
                self.pending.clear();
                self.discard = true;
            }
            return;
        }
        if self.escape {
            self.escape = false;
            if code == Some(b'\\') {
                self.end_string(b"\x1b\\", output);
                return;
            }
            // Cancel a streamed string before interpreting a restarted ESC.
            if self.passthrough {
                output.push(0x18);
            }
            self.reset();
            self.state = State::Escape;
            self.pending.push(0x1b);
            self.header = 1;
            self.historical = token.historical;
            self.accept(token, output);
            return;
        }
        if code == Some(0x1b) {
            self.escape = true;
            return;
        }
        if code == Some(0x9c) || self.state == State::Osc && code == Some(7) {
            self.end_string(token.bytes(), output);
            return;
        }
        if self.discard {
            return;
        }
        if self.passthrough {
            output.extend_from_slice(token.bytes());
            return;
        }
        self.pending.extend_from_slice(token.bytes());
        if self.state == State::Osc && code == Some(b';')
            || self.state == State::Dcs && code == Some(b'q')
        {
            let body = rules::string_body(&self.pending, self.header);
            if rules::denied_string(&body, self.state == State::Osc) {
                self.discard = true;
                self.pending.clear();
                return;
            }
        }
        if self.pending.len() > MAX_CONTROL {
            if matches!(self.state, State::Osc | State::Dcs)
                && rules::potential_query_string(
                    &rules::string_body(&self.pending, self.header),
                    self.state == State::Osc,
                )
            {
                // A later query suffix must not escape an oversized header.
                self.discard = true;
            } else {
                output.extend_from_slice(&self.pending);
                self.passthrough = true;
            }
            self.pending.clear();
        }
    }
    fn end_string(&mut self, ending: &[u8], output: &mut Vec<u8>) {
        if !self.discard {
            if self.passthrough {
                output.extend_from_slice(ending);
            } else if self.state == State::Osc {
                output.extend_from_slice(&rules::osc(
                    &self.pending,
                    self.header,
                    ending,
                    self.historical,
                ));
            } else {
                output.extend_from_slice(&self.pending);
                output.extend_from_slice(ending);
            }
        }
        self.reset();
    }
}

#[cfg(test)]
#[path = "queries_tests.rs"]
mod tests;
