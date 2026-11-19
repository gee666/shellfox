mod colors;
use colors::{contrast, ensure_contrast, lift, mix};
use gpui::{Hsla, rgb};

#[derive(Clone, Copy, PartialEq)]
pub struct Palette {
    pub background: Hsla,
    pub foreground: Hsla,
    pub sidebar: Hsla,
    pub panel: Hsla,
    pub border: Hsla,
    pub surface: Hsla,
    pub hover: Hsla,
    pub row_hover: Hsla,
    pub input_bg: Hsla,
    pub input_border: Hsla,
    pub placeholder: Hsla,
    pub muted: Hsla,
    pub accent: Hsla,
    pub background_rgb: u32,
    pub foreground_rgb: u32,
    pub accent_rgb: u32,
    pub ansi: [u32; 16],
    pub dark: bool,
}
pub(crate) struct ProgressColors {
    pub(crate) track: u32,
    pub(crate) value: u32,
    pub(crate) track_edge: u32,
    pub(crate) value_edge: u32,
}

fn composite(paper: u32, ink: u32, coverage: u32) -> u32 {
    [16, 8, 0].into_iter().fold(0, |out, shift| {
        let part = |rgb: u32, weight| (((rgb >> shift) & 255) * weight + 127) / 255;
        out | ((part(ink, coverage) + part(paper, 255 - coverage)) << shift)
    })
}

impl Palette {
    pub(crate) fn on_accent(self) -> Hsla {
        let white = contrast(0xffffff, self.accent_rgb);
        rgb(if white >= 3. || white >= contrast(0, self.accent_rgb) {
            0xffffff
        } else {
            0
        })
        .into()
    }

    pub(crate) fn progress_colors(self) -> ProgressColors {
        // Blink chooses the native control scheme against the unpainted
        // track colors, which need not follow the panel's scheme.
        let light_ratio = contrast(self.accent_rgb, 0xefefef);
        let dark_ratio = contrast(self.accent_rgb, 0x3b3b3b);
        let dark = if self.dark { dark_ratio >= 3. || dark_ratio >= light_ratio }
            else { light_ratio < 3. && light_ratio < dark_ratio };
        let (track, rim) = if dark { (0x3b3b3b, 0x858585) } else { (0xefefef, 0x767676) };
        let paper = gpui::Rgba::from(self.panel);
        let channel = |value: f32| (value * 255.).round() as u32;
        let paper = (channel(paper.r) << 16) | (channel(paper.g) << 8) | channel(paper.b);

        // Chromium centers a 2.5 px track in the 5 px widget. Its outer
        // rows have 3/4 coverage; the clipped value and inset rim have 9/16.
        // The light rim has half the opacity of the dark rim.
        let edge = composite(paper, track, 191);
        let rim_center = if dark { 64 } else { 32 };
        let rim_edge = if dark { 143 } else { 72 };
        ProgressColors {
            track: composite(track, rim, rim_center),
            value: composite(self.accent_rgb, rim, rim_center),
            track_edge: composite(edge, rim, rim_edge),
            value_edge: composite(composite(edge, self.accent_rgb, 143), rim, rim_edge),
        }
    }

    pub(crate) fn status_color(self, rgb: u32) -> Hsla {
        gpui::rgb(ensure_contrast(rgb, self.background_rgb, 3., self.dark)).into()
    }

    pub fn new(accent: &str, background: &str) -> Self {
        let background_rgb = parse_hex(background).unwrap_or(0x111016);
        let accent = parse_hex(accent).unwrap_or(0xec4899);
        let dark = contrast(0xffffff, background_rgb) >= contrast(0, background_rgb);
        let pole = if dark { 1. } else { 0. };
        let surface = |amount| lift(background_rgb, pole, amount * if dark { 1. } else { 1.5 });
        let foreground_rgb =
            ensure_contrast(lift(background_rgb, pole, 0.9), background_rgb, 7., dark);
        let muted = ensure_contrast(
            mix(background_rgb, foreground_rgb, 0.7),
            background_rgb,
            4.5,
            dark,
        );
        let accent_rgb = ensure_contrast(accent, background_rgb, if dark { 3. } else { 4.5 }, dark);
        let input_bg = lift(background_rgb, 1., if dark { 0.004 } else { 0.6 });
        let placeholder = ensure_contrast(
            mix(background_rgb, foreground_rgb, 0.45),
            input_bg,
            3.,
            dark,
        );
        let mut ansi = if dark { DARK_ANSI } else { LIGHT_ANSI }
            .map(|color| ensure_contrast(color, background_rgb, 4.5, dark));
        if dark {
            ansi[0] = mix(background_rgb, foreground_rgb, 0.15);
        } else {
            ansi[7] = mix(background_rgb, foreground_rgb, 0.15);
            ansi[15] = mix(background_rgb, foreground_rgb, 0.06);
        }
        Self {
            background: rgb(background_rgb).into(),
            foreground: rgb(foreground_rgb).into(),
            sidebar: rgb(surface(0.018)).into(),
            panel: rgb(surface(0.034)).into(),
            border: rgb(surface(0.13)).into(),
            surface: rgb(surface(0.085)).into(),
            hover: rgb(surface(0.15)).into(),
            row_hover: rgb(surface(0.05)).into(),
            input_bg: rgb(input_bg).into(),
            input_border: rgb(surface(0.2)).into(),
            placeholder: rgb(placeholder).into(),
            muted: rgb(muted).into(),
            accent: rgb(accent_rgb).into(),
            background_rgb,
            foreground_rgb,
            accent_rgb,
            ansi,
            dark,
        }
    }
}
const DARK_ANSI: [u32; 16] = [
    0x000000, 0xff7b72, 0x7ee787, 0xe3b341, 0x79c0ff, 0xd2a8ff, 0x56d4dd, 0xd0d7de, 0x6e7681,
    0xffa198, 0x56d364, 0xf2cc60, 0xa5d6ff, 0xe2c5ff, 0xb3f0ff, 0xffffff,
];
const LIGHT_ANSI: [u32; 16] = [
    0x1f2328, 0xcf222e, 0x116329, 0x7d4e00, 0x0550ae, 0x8250df, 0x1b7c83, 0x6e7781, 0x57606a,
    0xa40e26, 0x1a7f37, 0x633c01, 0x0969da, 0x6639ba, 0x136874, 0x8c959f,
];
fn parse_hex(text: &str) -> Option<u32> {
    (text.len() == 7 && text.starts_with('#'))
        .then(|| u32::from_str_radix(&text[1..], 16).ok())
        .flatten()
}
#[cfg(test)]
mod tests {
    use super::*;
    fn to_rgb(color: Hsla) -> u32 {
        let value: gpui::Rgba = color.into();
        ((value.r * 255.).round() as u32) << 16
            | ((value.g * 255.).round() as u32) << 8
            | (value.b * 255.).round() as u32
    }
    #[test]
    fn matches_shellfox_default_background_and_surfaces() {
        let palette = Palette::new("#ec4899", "#111016");
        assert_eq!(palette.background_rgb, 0x111016);
        assert_eq!(palette.accent_rgb, 0xec4899);
        assert_eq!(to_rgb(palette.sidebar), 0x15141b);
        assert_eq!(to_rgb(palette.panel), 0x18171f);
        assert_eq!(palette.foreground_rgb, 0xe5e4eb);
        assert_eq!(to_rgb(palette.border), 0x2c2a3a);
    }
    #[test]
    fn matches_existing_renderer_palette_samples_exactly() {
        for (background, foreground, accent, red, black, white) in [
            ("#111016", 0xe5e4eb, 0xec4899, 0xff7b72, 0x313036, 0xffffff),
            ("#ffffff", 0x191919, 0xe2187c, 0xcf222e, 0x1f2328, 0xf1f1f1),
            ("#667788", 0xffffff, 0xf9c1dc, 0xfffcfb, 0x7d8b9a, 0xffffff),
        ] {
            let palette = Palette::new("#ec4899", background);
            assert_eq!(
                (
                    palette.foreground_rgb,
                    palette.accent_rgb,
                    palette.ansi[1],
                    palette.ansi[0],
                    palette.ansi[15]
                ),
                (foreground, accent, red, black, white)
            );
        }
    }
    #[test]
    fn all_ansi16_entries_match_existing_renderer_fixtures() {
        assert_eq!(
            Palette::new("#ec4899", "#111016").ansi,
            [
                0x313036, 0xff7b72, 0x7ee787, 0xe3b341, 0x79c0ff, 0xd2a8ff, 0x56d4dd, 0xd0d7de,
                0x747d88, 0xffa198, 0x56d364, 0xf2cc60, 0xa5d6ff, 0xe2c5ff, 0xb3f0ff, 0xffffff
            ]
        );
        assert_eq!(
            Palette::new("#ec4899", "#ffffff").ansi,
            [
                0x1f2328, 0xcf222e, 0x116329, 0x7d4e00, 0x0550ae, 0x8250df, 0x1b7c83, 0xdddddd,
                0x57606a, 0xa40e26, 0x1a7f37, 0x633c01, 0x0969da, 0x6639ba, 0x136874, 0xf1f1f1
            ]
        );
    }
    #[test]
    fn light_background_uses_dark_text() {
        let palette = Palette::new("#ec4899", "#ffffff");
        assert!(!palette.dark);
        assert!(contrast(palette.foreground_rgb, palette.background_rgb) > 7.);
        assert_eq!(Palette::new("bad", "bad").background_rgb, 0x111016);
    }
}
