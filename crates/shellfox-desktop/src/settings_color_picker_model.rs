//! Color text parsing and RGB/HSL conversion for the color popup.
pub(super) type Rgb = [u8; 3];

pub(super) fn hex([r, g, b]: Rgb) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}

/// Six hex digits, with or without a leading `#`.
pub(super) fn parse_hex(text: &str) -> Option<Rgb> {
    let text = text.trim();
    let digits = text.strip_prefix('#').unwrap_or(text);
    if digits.len() != 6 || !digits.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let value = u32::from_str_radix(digits, 16).ok()?;
    let [_, r, g, b] = value.to_be_bytes();
    Some([r, g, b])
}

/// A whole number clamped to `0..=max`. Anything else is not a value yet.
pub(super) fn parse_channel(text: &str, max: u16) -> Option<u16> {
    let text = text.trim();
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    Some(text.parse::<u32>().map_or(max, |value| value.min(u32::from(max)) as u16))
}

/// Hue in degrees, saturation and lightness in percent.
pub(super) fn to_hsl(rgb: Rgb) -> [u16; 3] {
    let [r, g, b] = rgb.map(|channel| f32::from(channel) / 255.);
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let lightness = (max + min) / 2.;
    let delta = max - min;
    if delta == 0. {
        return [0, 0, (lightness * 100.).round() as u16];
    }
    let saturation = delta / (1. - (2. * lightness - 1.).abs());
    let hue = if max == r {
        ((g - b) / delta).rem_euclid(6.)
    } else if max == g {
        (b - r) / delta + 2.
    } else {
        (r - g) / delta + 4.
    } * 60.;
    [
        (hue.round() as u16) % 360,
        (saturation * 100.).round() as u16,
        (lightness * 100.).round() as u16,
    ]
}

pub(super) fn from_hsl([h, s, l]: [u16; 3]) -> Rgb {
    let hue = f32::from(h % 360) / 60.;
    let (saturation, lightness) = (f32::from(s.min(100)) / 100., f32::from(l.min(100)) / 100.);
    let chroma = (1. - (2. * lightness - 1.).abs()) * saturation;
    let x = chroma * (1. - (hue.rem_euclid(2.) - 1.).abs());
    let (r, g, b) = match hue as u8 {
        0 => (chroma, x, 0.),
        1 => (x, chroma, 0.),
        2 => (0., chroma, x),
        3 => (0., x, chroma),
        4 => (x, 0., chroma),
        _ => (chroma, 0., x),
    };
    let offset = lightness - chroma / 2.;
    [r, g, b].map(|channel| ((channel + offset) * 255.).round().clamp(0., 255.) as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_accepts_six_digits_with_or_without_hash() {
        assert_eq!(parse_hex("#EC4899"), Some([0xec, 0x48, 0x99]));
        assert_eq!(parse_hex(" ec4899 "), Some([0xec, 0x48, 0x99]));
        for bad in ["", "#ec489", "#ec48999", "#ec48zz", "+c4899"] {
            assert_eq!(parse_hex(bad), None, "{bad}");
        }
        assert_eq!(hex([0xec, 0x48, 0x99]), "#ec4899");
    }

    #[test]
    fn channels_are_whole_numbers_clamped_to_their_range() {
        assert_eq!(parse_channel("42", 255), Some(42));
        assert_eq!(parse_channel("300", 255), Some(255));
        assert_eq!(parse_channel("999", 100), Some(100));
        for bad in ["", "-1", "1.5", "4a"] {
            assert_eq!(parse_channel(bad, 255), None, "{bad}");
        }
    }

    #[test]
    fn hsl_round_trips_presets_and_primaries() {
        assert_eq!(to_hsl([0xec, 0x48, 0x99]), [330, 81, 60]);
        assert_eq!(to_hsl([255, 0, 0]), [0, 100, 50]);
        assert_eq!(to_hsl([0, 0, 255]), [240, 100, 50]);
        assert_eq!(to_hsl([128, 128, 128]), [0, 0, 50]);
        assert_eq!(from_hsl([120, 100, 50]), [0, 255, 0]);
        assert_eq!(from_hsl([360, 100, 50]), [255, 0, 0]);
        assert_eq!(from_hsl([0, 0, 100]), [255, 255, 255]);
        for preset in ["#a78bfa", "#60a5fa", "#111016", "#fdf6e3"] {
            let rgb = parse_hex(preset).unwrap();
            let back = from_hsl(to_hsl(rgb));
            assert!(rgb.iter().zip(back).all(|(a, b)| a.abs_diff(b) <= 3), "{preset}: {back:?}");
        }
    }
}
