pub(super) type Rgb = [u8; 3];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Format {
    Rgb,
    Hsl,
    Hex,
}

impl Format {
    pub fn cycle(self, previous: bool) -> Self {
        match (self, previous) {
            (Self::Rgb, false) | (Self::Hex, true) => Self::Hsl,
            (Self::Hsl, false) | (Self::Rgb, true) => Self::Hex,
            (Self::Hex, false) | (Self::Hsl, true) => Self::Rgb,
        }
    }

    pub fn fields(self) -> &'static [(&'static str, &'static str, usize)] {
        match self {
            Self::Rgb => &[("Red", "R", 3), ("Green", "G", 3), ("Blue", "B", 3)],
            Self::Hsl => &[
                ("Hue", "H", 3),
                ("Saturation", "S", 4),
                ("Lightness", "L", 4),
            ],
            Self::Hex => &[("Hexadecimal", "HEX", 7)],
        }
    }

    pub fn channels(self, color: Rgb) -> [u16; 3] {
        match self {
            Self::Rgb => color.map(u16::from),
            Self::Hsl => hsl(color),
            Self::Hex => [0; 3],
        }
    }

    pub fn text(self, color: Rgb, channels: [u16; 3]) -> [String; 3] {
        match self {
            Self::Rgb => channels.map(|value| value.to_string()),
            Self::Hsl => [
                channels[0].to_string(),
                format!("{}%", channels[1]),
                format!("{}%", channels[2]),
            ],
            Self::Hex => [
                hex(color).to_ascii_uppercase(),
                String::new(),
                String::new(),
            ],
        }
    }

    pub fn edit(self, text: &str, channel: usize, values: &mut [u16; 3]) -> Option<Rgb> {
        if self == Self::Hex {
            let text = text.strip_prefix('#').unwrap_or(text);
            return decode_hex(text);
        }
        let limit = match (self, channel) {
            (Self::Rgb, _) => 255,
            (_, 0) => 359,
            (_, _) => 100,
        };
        let digits = if self == Self::Hsl && channel > 0 {
            text.strip_suffix('%').unwrap_or(text)
        } else {
            text
        };
        if !digits.bytes().all(|digit| digit.is_ascii_digit()) {
            return None;
        }
        let value = digits.parse::<u16>().ok().filter(|value| *value <= limit)?;
        *values.get_mut(channel)? = value;
        Some(if self == Self::Rgb {
            values.map(|value| value as u8)
        } else {
            from_hsl(*values)
        })
    }
}

pub(super) fn hex([r, g, b]: Rgb) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}

pub(super) fn decode_hex(text: &str) -> Option<Rgb> {
    if !(1..=6).contains(&text.len()) || !text.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let [_, r, g, b] = u32::from_str_radix(text, 16).ok()?.to_be_bytes();
    Some([r, g, b])
}

pub(super) fn hsv(rgb: Rgb) -> (f32, f32, f32) {
    let [r, g, b] = rgb.map(|value| f32::from(value) / 255.);
    let high = r.max(g).max(b);
    let low = r.min(g).min(b);
    let delta = high - low;
    let sector = if delta == 0. {
        0.
    } else if r == high {
        (g - b) / delta
    } else if g == high {
        2. + (b - r) / delta
    } else {
        4. + (r - g) / delta
    };
    (
        (sector * 60.).rem_euclid(360.),
        if high == 0. { 0. } else { delta / high },
        high,
    )
}

pub(super) fn hue_rgb(hue: f32) -> Rgb {
    [5., 3., 1.].map(|n| {
        let phase = (n + hue / 60.).rem_euclid(6.);
        component(1. - phase.min(4. - phase).clamp(0., 1.))
    })
}

pub(super) fn well_rgb(hue: f32, x: f32, y: f32) -> Rgb {
    well_sample(hue_rgb(hue), x, y)
}

pub(super) fn well_sample(base: Rgb, x: f32, y: f32) -> Rgb {
    let saturation = ((x - 0.01) / 0.98).clamp(0., 1.);
    let value = ((0.99 - y) / 0.98).clamp(0., 1.);
    base.map(|c| component(value * (1. - saturation * (1. - f32::from(c) / 255.))))
}

pub(super) fn hsl(color: Rgb) -> [u16; 3] {
    let (h, s, v) = hsv(color);
    let l = v * (1. - s / 2.);
    let s = if l <= 0. || l >= 1. {
        0.
    } else {
        (v - l) / l.min(1. - l)
    };
    [
        ((h.round() as u16) % 360),
        (s * 100.).round() as u16,
        (l * 100.).round() as u16,
    ]
}

pub(super) fn from_hsl([h, s, l]: [u16; 3]) -> Rgb {
    let s = f32::from(s) / 100.;
    let l = f32::from(l) / 100.;
    let amplitude = s * l.min(1. - l);
    [0., 8., 4.].map(|n| {
        let k = (n + f32::from(h) / 30.).rem_euclid(12.);
        component(l - amplitude * (k - 3.).min(9. - k).clamp(-1., 1.))
    })
}

const HUE_STOPS: [(f32, f32); 7] = [
    (0.01, 360.),
    (0.17, 300.),
    (0.33, 240.),
    (0.50, 180.),
    (0.67, 120.),
    (0.83, 60.),
    (0.99, 0.),
];

pub(super) fn hue_at(position: f32) -> f32 {
    if position <= HUE_STOPS[0].0 {
        return 0.;
    }
    for stop in HUE_STOPS.windows(2) {
        let [(start, hue), (end, next)] = [stop[0], stop[1]];
        if position <= end {
            return hue + (next - hue) * ((position - start) / (end - start));
        }
    }
    0.
}

pub(super) fn nearest_hue_x(hue: f32) -> f32 {
    use super::SLIDER_WIDTH;
    let hue = hue.round() as u16 % 360;
    (0..SLIDER_WIDTH as u16)
        .min_by_key(|x| {
            let position = (f32::from(*x) + 0.5) / SLIDER_WIDTH;
            hsl(hue_rgb(hue_at(position)))[0].abs_diff(hue)
        })
        .map_or(0., f32::from)
}

pub(super) fn position_of(color: Rgb) -> (f32, (f32, f32)) {
    use super::{WELL_HEIGHT, WELL_WIDTH};
    let [hue, saturation, lightness] = hsl(color);
    let hue = f32::from(hue);
    let base = hue_rgb(hue);
    let mut best = (u32::MAX, (0., 0.));
    // Chromium takes the first raster pixel with the nearest HSL values.
    for y in 0..WELL_HEIGHT as u16 {
        for x in 0..WELL_WIDTH as u16 {
            let (x, y) = (f32::from(x), f32::from(y));
            let [_, s, l] = hsl(well_sample(
                base,
                (x + 0.5) / WELL_WIDTH,
                (y + 0.5) / WELL_HEIGHT,
            ));
            let distance =
                u32::from(s.abs_diff(saturation)).pow(2) + u32::from(l.abs_diff(lightness)).pow(2);
            let offset = (x / WELL_WIDTH, y / WELL_HEIGHT);
            if distance == 0 {
                return (hue, offset);
            }
            if distance < best.0 {
                best = (distance, offset);
            }
        }
    }
    (hue, best.1)
}

fn component(value: f32) -> u8 {
    (value.clamp(0., 1.) * 255.).round() as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_keys_cycle_and_wrap() {
        assert_eq!(Format::Rgb.cycle(false), Format::Hsl);
        assert_eq!(Format::Rgb.cycle(true), Format::Hex);
        assert_eq!(Format::Hex.cycle(false), Format::Rgb);
    }

    #[test]
    fn channels_keep_last_valid_value_until_valid_input() {
        let mut rgb = [236, 72, 153];
        assert_eq!(Format::Rgb.edit("256", 0, &mut rgb), None);
        assert_eq!(Format::Rgb.edit("0", 0, &mut rgb), Some([0, 72, 153]));
        assert_eq!(Format::Hex.edit("#f0", 0, &mut rgb), Some([0, 0, 240]));
        assert_eq!(Format::Hex.edit("#12é45", 0, &mut rgb), None);
        assert_eq!(
            Format::Hsl.edit("100%", 1, &mut [240, 0, 50]),
            Some([0, 0, 255])
        );
    }

    #[test]
    fn hue_and_well_match_primary_endpoints() {
        assert_eq!(hue_rgb(0.), [255, 0, 0]);
        assert_eq!(hue_rgb(120.), [0, 255, 0]);
        assert_eq!(hue_rgb(240.), [0, 0, 255]);
        assert_eq!(hue_rgb(hue_at(0.17)), [255, 0, 255]);
        assert_eq!(hue_rgb(hue_at(0.33)), [0, 0, 255]);
        assert_eq!(hue_rgb(hue_at(0.67)), [0, 255, 0]);
        assert_eq!(nearest_hue_x(0.), 0.);
        assert_eq!(well_rgb(120., 0., 0.), [255; 3]);
        assert_eq!(well_rgb(120., 1., 1.), [0; 3]);
    }
}
