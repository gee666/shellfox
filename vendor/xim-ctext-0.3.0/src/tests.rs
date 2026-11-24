use crate::{compound_text_to_utf8, compound_text_to_utf8_lossy, utf8_to_compound_text};
use alloc::string::String;
use alloc::vec::Vec;

fn decodes(bytes: &[u8], text: &str) {
    assert_eq!(
        compound_text_to_utf8(bytes).unwrap(),
        text,
        "{:02X?}",
        bytes
    );
}

// What Xlib's XmbTextListToTextProperty(XCompoundTextStyle) produces in a
// UTF-8 locale, as ibus-x11 and other XIM servers send it.
#[test]
fn xlib_output_for_every_script() {
    decodes(&[0x1B, 0x2D, 0x4C, 0xE4], "ф");
    decodes(&[0x1B, 0x2D, 0x4C, 0xC4], "Ф");
    decodes(
        &[0x1B, 0x2D, 0x4C, 0xDF, 0xE0, 0xD8, 0xD2, 0xD5, 0xE2],
        "привет",
    );
    decodes(
        &[
            0x1B, 0x2D, 0x4C, 0xF7, 0x20, 0x1B, 0x25, 0x47, 0xD2, 0x90, 0x1B, 0x25, 0x40,
        ],
        "ї Ґ",
    );
    decodes(&[0x1B, 0x2D, 0x46, 0xE1, 0xE2, 0xE3], "αβγ");
    decodes(&[0xE9, 0x20, 0xF1, 0x20, 0xFC], "é ñ ü");
    decodes(&[0xDF], "ß");
    decodes(&[0x1B, 0x2D, 0x42, 0xB3], "ł");
    decodes(&[0x1B, 0x2D, 0x62, 0xA4], "€");
    decodes(&[0x1B, 0x2D, 0x62, 0xA4, 0x1B, 0x2D, 0x4C, 0xE4], "€ф");
    decodes(
        &[
            0x1B, 0x25, 0x47, 0xD7, 0xA9, 0xD7, 0x9C, 0xD7, 0x95, 0xD7, 0x9D, 0x1B, 0x25, 0x40,
        ],
        "שלום",
    );
    decodes(
        &[
            0x1B, 0x25, 0x47, 0xD9, 0x85, 0xD8, 0xB1, 0xD8, 0xAD, 0xD8, 0xA8, 0xD8, 0xA7, 0x1B,
            0x25, 0x40,
        ],
        "مرحبا",
    );
    decodes(&[0x1B, 0x24, 0x28, 0x42, 0x43, 0x66, 0x4A, 0x38], "中文");
    decodes(
        &[0x1B, 0x24, 0x28, 0x42, 0x46, 0x7C, 0x4B, 0x5C, 0x38, 0x6C],
        "日本語",
    );
    decodes(
        &[0x1B, 0x24, 0x28, 0x43, 0x47, 0x51, 0x31, 0x39, 0x3E, 0x6E],
        "한국어",
    );
    decodes(
        &[0x1B, 0x25, 0x47, 0xF0, 0x9F, 0xA6, 0x8A, 0x1B, 0x25, 0x40],
        "🦊",
    );
    decodes(
        &[
            0x61, 0x1B, 0x2D, 0x4C, 0xE4, 0x1B, 0x25, 0x47, 0xF0, 0x9F, 0xA6, 0x8A, 0x1B, 0x25,
            0x40, 0x1B, 0x24, 0x28, 0x42, 0x43, 0x66, 0x1B, 0x28, 0x42, 0x62,
        ],
        "aф🦊中b",
    );
}

#[test]
fn other_designations() {
    // GB2312 and KS C 5601 in GR, i.e. plain EUC.
    decodes(&[0x1B, 0x24, 0x29, 0x41, 0xD6, 0xD0, 0xCE, 0xC4], "中文");
    decodes(&[0x1B, 0x24, 0x29, 0x43, 0xC7, 0xD1], "한");
    decodes(&[0x1B, 0x24, 0x41, 0x56, 0x50], "中");
    // Hebrew, Arabic and Latin-5 sets.
    decodes(&[0x1B, 0x2D, 0x48, 0xF9], "ש");
    decodes(&[0x1B, 0x2D, 0x47, 0xC7], "ا");
    decodes(&[0x1B, 0x2D, 0x4D, 0xFD], "ı");
    // JIS-Roman and half-width katakana.
    decodes(&[0x1B, 0x28, 0x4A, 0x5C], "¥");
    decodes(&[0x1B, 0x29, 0x49, 0xB1], "ｱ");
    // A designation applies until the next one.
    decodes(
        &[0x1B, 0x2D, 0x4C, 0xE4, 0x61, 0xE4, 0x1B, 0x2D, 0x41, 0xE4],
        "фaфä",
    );
}

#[test]
fn utf8_segments_anywhere_and_unterminated() {
    let mut bytes = Vec::from(&b"ab"[..]);
    bytes.extend(utf8_to_compound_text("ф🦊"));
    bytes.extend(b"cd");
    decodes(&bytes, "abф🦊cd");
    decodes(&[0x1B, 0x25, 0x47, 0xD1, 0x84], "ф");
    decodes(&[0x1B, 0x25, 0x47], "");
    decodes(&[0x1B, 0x25, 0x47, 0x1B, 0x25, 0x40], "");
}

#[test]
fn extended_segments() {
    let segment = |width: u8, name: &str, data: &[u8]| {
        let len = name.len() + 1 + data.len();
        let mut v = Vec::from(&[0x1B, 0x25, 0x2F, width][..]);
        v.push(0x80 + (len / 128) as u8);
        v.push(0x80 + (len % 128) as u8);
        v.extend(name.as_bytes());
        v.push(0x02);
        v.extend(data);
        v
    };
    decodes(&segment(b'0', "ISO10646-1", "ф🦊".as_bytes()), "ф🦊");
    decodes(&segment(b'2', "iso10646-1", &[0x04, 0x44]), "ф");
    decodes(&segment(b'2', "big5-0", &[0xA4, 0xA4]), "中");
    let mut skipped = segment(b'2', "nonexistent-0", &[1, 2, 3, 4]);
    skipped.push(b'x');
    assert_eq!(compound_text_to_utf8_lossy(&skipped), "\u{FFFD}x");
    assert!(compound_text_to_utf8(&skipped).is_err());
}

#[test]
fn bare_text_controls_and_directions() {
    decodes(b"", "");
    decodes(b"plain", "plain");
    decodes("фу".as_bytes(), "фу");
    decodes(b"a\tb\nc", "a\tb\nc");
    // CSI direction changes carry no text.
    decodes(&[0x9B, 0x32, 0x5D, 0x1B, 0x2D, 0x47, 0xC7, 0x9B, 0x5D], "ا");
}

#[test]
fn round_trip_for_every_script() {
    for text in [
        "привет",
        "Ґанок їжак",
        "αβγ",
        "éàüñ",
        "שלום",
        "مرحبا",
        "中文",
        "日本語",
        "한국어",
        "🦊👍🏽",
        "aф中🦊b",
    ] {
        decodes(&utf8_to_compound_text(text), text);
    }
}

#[test]
fn malformed_input_is_lossy_not_fatal() {
    for bytes in [
        &[0x1B][..],
        &[0x1B, 0x2D],
        &[0x1B, 0x2D, 0x5A, 0xE4],
        &[0x1B, 0x25, 0x2F],
        &[0x1B, 0x25, 0x2F, 0x31, 0x80],
        &[0x1B, 0x25, 0x2F, 0x31, 0xFF, 0xFF, 0x41],
        &[0x1B, 0x25, 0x47, 0xFF, 0xFE],
        &[0x1B, 0x24, 0x28, 0x41, 0x21],
    ] {
        let lossy = compound_text_to_utf8_lossy(bytes);
        assert!(lossy.contains('\u{FFFD}'), "{:02X?} -> {:?}", bytes, lossy);
        assert_eq!(
            compound_text_to_utf8(bytes).unwrap_err().into_lossy(),
            lossy
        );
    }
}

#[test]
fn random_bytes_never_panic() {
    let mut seed = 0x2545_F491_4F6C_DD1Du64;
    let mut next = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    // Bias towards bytes that start or shape escape sequences.
    const HOT: &[u8] = &[
        0x1B, 0x24, 0x25, 0x28, 0x29, 0x2D, 0x2F, 0x40, 0x41, 0x42, 0x47, 0x4C, 0x9B, 0x80, 0x02,
    ];
    for _ in 0..200_000 {
        let len = (next() % 24) as usize;
        let bytes: Vec<u8> = (0..len)
            .map(|_| {
                let r = next();
                if r % 3 == 0 {
                    HOT[(r >> 8) as usize % HOT.len()]
                } else {
                    (r >> 16) as u8
                }
            })
            .collect();
        let text: String = compound_text_to_utf8_lossy(&bytes);
        assert_eq!(
            compound_text_to_utf8(&bytes).unwrap_or_else(|e| e.into_lossy()),
            text
        );
    }
}
