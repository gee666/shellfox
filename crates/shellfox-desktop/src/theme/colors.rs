// Port of the palette arithmetic in src/renderer/theme.ts. Keep f64 here:
// rounding HSL through f32 changes actual 8-bit colors, especially on light themes.
pub(super) fn mix(a: u32, b: u32, amount: f64) -> u32 {
    [16, 8, 0].into_iter().fold(0, |rgb, shift| {
        let a = ((a >> shift) & 255) as f64;
        let b = ((b >> shift) & 255) as f64;
        rgb | (((a + (b - a) * amount).round() as u32) << shift)
    })
}
fn luminance(color: u32) -> f64 {
    [16, 8, 0]
        .into_iter()
        .zip([0.2126, 0.7152, 0.0722])
        .map(|(shift, weight)| {
            let channel = ((color >> shift) & 255) as f64 / 255.;
            weight
                * if channel <= 0.03928 {
                    channel / 12.92
                } else {
                    ((channel + 0.055) / 1.055).powf(2.4)
                }
        })
        .sum()
}
pub(super) fn contrast(a: u32, b: u32) -> f64 {
    let a = luminance(a);
    let b = luminance(b);
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}
fn to_hsl(color: u32) -> (f64, f64, f64) {
    let r = ((color >> 16) & 255) as f64 / 255.;
    let g = ((color >> 8) & 255) as f64 / 255.;
    let b = (color & 255) as f64 / 255.;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.;
    let d = max - min;
    if d == 0. {
        return (0., 0., l);
    }
    let s = d / (1. - (2. * l - 1.).abs());
    let h = if max == r {
        ((g - b) / d + 6.) % 6.
    } else if max == g {
        (b - r) / d + 2.
    } else {
        (r - g) / d + 4.
    };
    (h * 60., s, l)
}
fn from_hsl(h: f64, s: f64, l: f64) -> u32 {
    let c = (1. - (2. * l - 1.).abs()) * s;
    let x = c * (1. - ((h / 60.) % 2. - 1.).abs());
    let m = l - c / 2.;
    let (r, g, b) = if h < 60. {
        (c, x, 0.)
    } else if h < 120. {
        (x, c, 0.)
    } else if h < 180. {
        (0., c, x)
    } else if h < 240. {
        (0., x, c)
    } else if h < 300. {
        (x, 0., c)
    } else {
        (c, 0., x)
    };
    let byte = |channel: f64| ((channel + m) * 255.).round().clamp(0., 255.) as u32;
    byte(r) << 16 | byte(g) << 8 | byte(b)
}
pub(super) fn lift(color: u32, toward: f64, amount: f64) -> u32 {
    let (h, s, l) = to_hsl(color);
    from_hsl(h, s, l + (toward - l) * amount.min(1.))
}
pub(super) fn ensure_contrast(color: u32, against: u32, minimum: f64, dark: bool) -> u32 {
    if contrast(color, against) >= minimum {
        return color;
    }
    let (h, s, l) = to_hsl(color);
    for target in if dark { [1., 0.] } else { [0., 1.] } {
        if contrast(from_hsl(h, s, target), against) < minimum {
            continue;
        }
        let mut lo = l;
        let mut hi = target;
        for _ in 0..24 {
            let mid = (lo + hi) / 2.;
            if contrast(from_hsl(h, s, mid), against) >= minimum {
                hi = mid;
            } else {
                lo = mid;
            }
        }
        return from_hsl(h, s, hi);
    }
    if contrast(0xffffff, against) >= contrast(0, against) {
        0xffffff
    } else {
        0
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hsl_rounding_matches_existing_typescript_without_one_pixel_tolerance() {
        assert_eq!(lift(0xffffff, 0., 0.9), 0x191919);
        assert_eq!(lift(0x111016, 1., 0.018), 0x15141b);
        assert_eq!(lift(0x111016, 1., 0.034), 0x18171f);
        assert_eq!(lift(0x111016, 1., 0.13), 0x2c2a3a);
        assert_eq!(ensure_contrast(0xec4899, 0xffffff, 4.5, false), 0xe2187c);
        assert_eq!(ensure_contrast(0xec4899, 0x667788, 3., true), 0xf9c1dc);
    }
}
