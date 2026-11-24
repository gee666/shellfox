//! COMPOUND_TEXT (X11 CTEXT) encoder and decoder.
//!
//! Encoding always wraps the text in a UTF-8 segment. Decoding handles what
//! XIM servers really send: ISO 2022 designations for GL/GR (ISO 8859 sets,
//! GB2312, JIS X0208, KS C 5601, JIS X0201), UTF-8 segments anywhere, and
//! extended segments. Unknown parts become U+FFFD instead of failing.

#![no_std]

extern crate alloc;

#[cfg(feature = "std")]
extern crate std;

use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;
use encoding_rs::Encoding;

#[cfg(feature = "std")]
use std::io::{self, Write};

const ESC: u8 = 0x1B;
const CSI: u8 = 0x9B;
const UTF8_START: &[u8] = &[ESC, 0x25, 0x47];
const UTF8_END: &[u8] = &[ESC, 0x25, 0x40];

/// Wrapper for reduce allocation
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct CText<'s> {
    utf8: &'s str,
}

impl<'s> fmt::Debug for CText<'s> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.utf8)
    }
}

impl<'s> fmt::Display for CText<'s> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.utf8)
    }
}

impl<'s> CText<'s> {
    pub const fn new(utf8: &'s str) -> Self {
        Self { utf8 }
    }

    pub const fn len(self) -> usize {
        self.utf8.len() + UTF8_START.len() + UTF8_END.len()
    }

    #[cfg(feature = "std")]
    pub fn write(self, mut out: impl Write) -> io::Result<usize> {
        let mut writed = 0;
        writed += out.write(UTF8_START)?;
        writed += out.write(self.utf8.as_bytes())?;
        writed += out.write(UTF8_END)?;
        Ok(writed)
    }
}

/// Encoding utf8 to COMPOUND_TEXT with utf8 escape
pub fn utf8_to_compound_text(text: &str) -> Vec<u8> {
    let mut ret = Vec::with_capacity(text.len() + 6);
    ret.extend_from_slice(UTF8_START);
    ret.extend_from_slice(text.as_bytes());
    ret.extend_from_slice(UTF8_END);
    ret
}

#[derive(Debug, Clone)]
pub enum DecodeError {
    /// Some bytes were malformed or in an unsupported charset. The lossy
    /// text, with U+FFFD in their place, is attached.
    InvalidEncoding(String),
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidEncoding(_) => write!(f, "Invalid or unsupported compound text"),
        }
    }
}

impl DecodeError {
    /// The text decoded so far, with U+FFFD for what could not be.
    pub fn into_lossy(self) -> String {
        match self {
            Self::InvalidEncoding(text) => text,
        }
    }
}

/// Decodes COMPOUND_TEXT. Fails only when part of it had to be replaced.
pub fn compound_text_to_utf8(bytes: &[u8]) -> Result<String, DecodeError> {
    let mut decoder = Decoder::default();
    let text = decoder.run(bytes);
    if decoder.lossy {
        Err(DecodeError::InvalidEncoding(text))
    } else {
        Ok(text)
    }
}

/// Decodes COMPOUND_TEXT, replacing what cannot be decoded with U+FFFD.
pub fn compound_text_to_utf8_lossy(bytes: &[u8]) -> String {
    Decoder::default().run(bytes)
}

/// A graphic character set designated to GL or GR.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Charset {
    Ascii,
    JisRoman,
    Katakana,
    /// A 96-character set whose GR half `encoding` decodes, e.g. ISO 8859-5.
    Single(&'static Encoding),
    /// A 94x94 set decoded from its EUC form, e.g. GB2312 as EUC-CN.
    Double(&'static Encoding),
    Unknown,
}

struct Decoder {
    gl: Charset,
    gr: Charset,
    out: String,
    lossy: bool,
}

impl Default for Decoder {
    fn default() -> Self {
        Self {
            gl: Charset::Ascii,
            gr: Charset::Single(encoding_rs::WINDOWS_1252),
            out: String::new(),
            lossy: false,
        }
    }
}

fn is_gl(b: u8) -> bool {
    (0x21..=0x7E).contains(&b)
}

fn is_gr(b: u8) -> bool {
    b >= 0xA0
}

impl Decoder {
    fn run(&mut self, bytes: &[u8]) -> String {
        // Some servers send bare UTF-8. Without escapes it is unambiguous
        // enough: real Latin-1 high bytes are rarely valid UTF-8.
        if !bytes.iter().any(|&b| b == ESC || b == CSI) {
            if let Ok(text) = core::str::from_utf8(bytes) {
                return String::from(text);
            }
        }
        let mut i = 0;
        while i < bytes.len() {
            let b = bytes[i];
            if b == ESC {
                i = self.escape(bytes, i);
            } else if b == CSI {
                i = skip_csi(bytes, i);
            } else if is_gl(b) {
                let end = run_end(bytes, i, is_gl);
                self.graphic(self.gl, &bytes[i..end], false);
                i = end;
            } else if is_gr(b) {
                let end = run_end(bytes, i, is_gr);
                self.graphic(self.gr, &bytes[i..end], true);
                i = end;
            } else {
                // Space, C0 and C1 controls stand for themselves.
                self.out.push(char::from(b));
                i += 1;
            }
        }
        core::mem::take(&mut self.out)
    }

    fn replace(&mut self) {
        self.out.push(char::REPLACEMENT_CHARACTER);
        self.lossy = true;
    }

    fn decode(&mut self, encoding: &'static Encoding, bytes: &[u8]) {
        let (text, had_errors) = encoding.decode_without_bom_handling(bytes);
        self.out.push_str(&text);
        self.lossy |= had_errors;
    }

    fn graphic(&mut self, set: Charset, run: &[u8], gr: bool) {
        let seven: Vec<u8> = run.iter().map(|b| b & 0x7F).collect();
        match set {
            Charset::Ascii => self.out.extend(seven.iter().map(|&b| char::from(b))),
            Charset::JisRoman => self.out.extend(seven.iter().map(|&b| match b {
                0x5C => '\u{A5}',
                0x7E => '\u{203E}',
                b => char::from(b),
            })),
            Charset::Katakana => {
                for &b in &seven {
                    match b {
                        0x21..=0x5F => self.out.push(
                            char::from_u32(0xFF61 + u32::from(b - 0x21))
                                .unwrap_or(char::REPLACEMENT_CHARACTER),
                        ),
                        _ => self.replace(),
                    }
                }
            }
            Charset::Single(encoding) => {
                if gr {
                    self.decode(encoding, run);
                } else {
                    // A 96-set cannot sit in GL in compound text.
                    for _ in run {
                        self.replace();
                    }
                }
            }
            Charset::Double(encoding) => {
                let euc: Vec<u8> = run.iter().map(|b| b | 0x80).collect();
                self.decode(encoding, &euc);
            }
            Charset::Unknown => {
                for _ in run {
                    self.replace();
                }
            }
        }
    }

    /// Handles the escape sequence at `start`; returns the index after it.
    fn escape(&mut self, bytes: &[u8], start: usize) -> usize {
        let mut i = start + 1;
        while i < bytes.len() && (0x20..=0x2F).contains(&bytes[i]) {
            i += 1;
        }
        let Some(&final_byte) = bytes.get(i).filter(|b| (0x30..=0x7E).contains(*b)) else {
            self.replace();
            return i.max(start + 1);
        };
        let end = i + 1;
        match (&bytes[start + 1..i], final_byte) {
            (b"(", f) => self.gl = single94(f),
            (b")", f) => self.gr = single94(f),
            (b"-", f) => self.gr = single96(f),
            (b"$(", f) | (b"$", f) => self.gl = double94(f),
            (b"$)", f) => self.gr = double94(f),
            (b"%", b'G') => return self.utf8_segment(bytes, end),
            // Already in the default encoding: nothing to switch back from.
            (b"%", b'@') => {}
            (b"%/", b'0'..=b'4') => return self.extended_segment(bytes, end, final_byte),
            _ => self.replace(),
        }
        end
    }

    /// UTF-8 up to `ESC % @` or the end of the text.
    fn utf8_segment(&mut self, bytes: &[u8], start: usize) -> usize {
        let rest = &bytes[start..];
        let len = rest
            .windows(UTF8_END.len())
            .position(|w| w == UTF8_END)
            .unwrap_or(rest.len());
        self.decode(encoding_rs::UTF_8, &rest[..len]);
        (start + len + UTF8_END.len()).min(bytes.len())
    }

    /// `ESC % / F M L name STX data`, where M and L give the length of
    /// name, STX and data. Known charsets are decoded, others skipped.
    fn extended_segment(&mut self, bytes: &[u8], start: usize, width: u8) -> usize {
        let (Some(&m), Some(&l)) = (bytes.get(start), bytes.get(start + 1)) else {
            self.replace();
            return bytes.len();
        };
        if m < 0x80 || l < 0x80 {
            self.replace();
            return start;
        }
        let len = usize::from(m - 0x80) * 128 + usize::from(l - 0x80);
        let body_start = start + 2;
        let end = (body_start + len).min(bytes.len());
        let body = &bytes[body_start..end];
        let Some(stx) = body.iter().position(|&b| b == 0x02) else {
            self.replace();
            return end;
        };
        let (name, data) = (&body[..stx], &body[stx + 1..]);
        match extended_encoding(name, width) {
            Some(encoding) => self.decode(encoding, data),
            None => self.replace(),
        }
        end
    }
}

fn run_end(bytes: &[u8], start: usize, class: fn(u8) -> bool) -> usize {
    start + bytes[start..].iter().take_while(|&&b| class(b)).count()
}

/// Skips a CSI sequence (direction changes), which carries no text.
fn skip_csi(bytes: &[u8], start: usize) -> usize {
    let mut i = start + 1;
    while i < bytes.len() && (0x20..=0x3F).contains(&bytes[i]) {
        i += 1;
    }
    if i < bytes.len() && (0x40..=0x7E).contains(&bytes[i]) {
        i += 1;
    }
    i
}

fn single94(f: u8) -> Charset {
    match f {
        b'B' => Charset::Ascii,
        b'J' => Charset::JisRoman,
        b'I' => Charset::Katakana,
        _ => Charset::Unknown,
    }
}

fn single96(f: u8) -> Charset {
    use encoding_rs as e;
    // encoding_rs names ISO 8859-1 and -9 after their Windows supersets,
    // which agree on 0xA0-0xFF; TIS-620 likewise maps to windows-874.
    Charset::Single(match f {
        b'A' => e::WINDOWS_1252,
        b'B' => e::ISO_8859_2,
        b'C' => e::ISO_8859_3,
        b'D' => e::ISO_8859_4,
        b'F' => e::ISO_8859_7,
        b'G' => e::ISO_8859_6,
        b'H' => e::ISO_8859_8,
        b'L' => e::ISO_8859_5,
        b'M' => e::WINDOWS_1254,
        b'T' => e::WINDOWS_874,
        b'V' => e::ISO_8859_10,
        b'Y' => e::ISO_8859_13,
        b'_' => e::ISO_8859_14,
        b'b' => e::ISO_8859_15,
        b'f' => e::ISO_8859_16,
        _ => return Charset::Unknown,
    })
}

fn double94(f: u8) -> Charset {
    Charset::Double(match f {
        b'A' => encoding_rs::GBK,
        b'B' => encoding_rs::EUC_JP,
        b'C' => encoding_rs::EUC_KR,
        _ => return Charset::Unknown,
    })
}

fn extended_encoding(name: &[u8], width: u8) -> Option<&'static Encoding> {
    let name = core::str::from_utf8(name).ok()?.to_ascii_lowercase();
    if name == "iso10646-1" {
        return Some(if width == b'2' {
            encoding_rs::UTF_16BE
        } else {
            encoding_rs::UTF_8
        });
    }
    Encoding::for_label(name.as_bytes())
        .or_else(|| Encoding::for_label(name.strip_suffix("-0")?.as_bytes()))
}

#[cfg(test)]
mod tests;
