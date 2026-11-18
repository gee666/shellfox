//! Transport decoding only, before the ONE VT parser. Raw data/sequence is never changed.
use crate::ExternalStream;
use std::borrow::Cow;
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ExternalDisplayPolicy {
    #[default]
    RawBytes,
    LegacyAutomationV1,
}
pub(crate) struct Display {
    policy: ExternalDisplayPolicy,
    streams: [Utf8Transport; 2],
}
impl Display {
    pub(crate) fn new(policy: ExternalDisplayPolicy) -> Self {
        Self {
            policy,
            streams: Default::default(),
        }
    }
    pub(crate) fn data<'a>(&mut self, stream: ExternalStream, raw: &'a [u8]) -> Cow<'a, [u8]> {
        if self.policy == ExternalDisplayPolicy::RawBytes {
            return Cow::Borrowed(raw);
        }
        let stream = usize::from(stream == ExternalStream::Stderr);
        Cow::Owned(normalize(&self.streams[stream].write(raw)))
    }
    pub(crate) fn finish(&mut self) -> Vec<u8> {
        if self.policy == ExternalDisplayPolicy::RawBytes {
            return Vec::new();
        }
        // Original child.close flush order, NOT per-descriptor EOF arrival order.
        let text = self.streams[0].end() + &self.streams[1].end();
        normalize(&text)
    }
}
fn normalize(text: &str) -> Vec<u8> {
    text.replace("\r\n", "\n")
        .replace('\n', "\r\n")
        .into_bytes()
}
#[derive(Default)]
struct Utf8Transport {
    pending: [u8; 4],
    used: usize,
    total: usize,
}
fn leading(byte: u8) -> Option<usize> {
    match byte {
        0xc0..=0xdf => Some(2),
        0xe0..=0xef => Some(3),
        0xf0..=0xf7 => Some(4),
        _ => None,
    }
}
impl Utf8Transport {
    fn write(&mut self, raw: &[u8]) -> String {
        let mut text = String::new();
        let mut offset = 0;
        if self.used > 0 {
            while offset < raw.len() && self.used < self.total && raw[offset] & 0xc0 == 0x80 {
                self.pending[self.used] = raw[offset];
                self.used += 1;
                offset += 1;
            }
            if self.used < self.total && offset == raw.len() {
                return text;
            }
            text.push_str(&String::from_utf8_lossy(&self.pending[..self.used]));
            self.used = 0;
            self.total = 0;
        }
        let bytes = &raw[offset..];
        let mut cut = bytes.len();
        for distance in 1..=3.min(bytes.len()) {
            let start = bytes.len() - distance;
            let byte = bytes[start];
            if let Some(total) = leading(byte) {
                if distance < total {
                    self.pending[..distance].copy_from_slice(&bytes[start..]);
                    self.used = distance;
                    self.total = total;
                    cut = start;
                }
                break;
            }
            if byte & 0xc0 != 0x80 {
                break;
            }
        }
        text.push_str(&String::from_utf8_lossy(&bytes[..cut]));
        text
    }
    fn end(&mut self) -> String {
        let text = String::from_utf8_lossy(&self.pending[..self.used]).into_owned();
        self.used = 0;
        self.total = 0;
        text
    }
}
#[cfg(test)]
#[path = "external_display_tests.rs"]
mod tests;
