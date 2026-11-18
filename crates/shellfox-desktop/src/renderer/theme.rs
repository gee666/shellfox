use shellfox_ghostty_vt::{Rgb, TerminalColors};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TerminalTheme {
    pub background: u32,
    pub foreground: u32,
    pub accent: u32,
    ansi: Option<[u32; 16]>,
}

impl TerminalTheme {
    pub fn from_colors(background: u32, foreground: u32, accent: u32) -> Self {
        Self {
            background,
            foreground,
            accent,
            ansi: None,
        }
    }

    pub fn with_ansi_palette(mut self, ansi: [u32; 16]) -> Self {
        self.ansi = Some(ansi);
        self
    }

    pub(crate) fn valid(self) -> bool {
        [self.background, self.foreground, self.accent]
            .into_iter()
            .chain(self.ansi.into_iter().flatten())
            .all(|color| color <= 0xffffff)
    }

    pub(crate) fn colors(self) -> TerminalColors {
        let rgb = |color: u32| Rgb {
            r: (color >> 16) as u8,
            g: (color >> 8) as u8,
            b: color as u8,
        };
        TerminalColors {
            foreground: rgb(self.foreground),
            background: rgb(self.background),
            cursor: Some(rgb(self.accent)),
            palette: self.ansi.map(|ansi| {
                std::array::from_fn(|index| {
                    if index < 16 {
                        return rgb(ansi[index]);
                    }
                    if index >= 232 {
                        let gray = 8 + (index - 232) as u8 * 10;
                        return Rgb {
                            r: gray,
                            g: gray,
                            b: gray,
                        };
                    }
                    let cube = [0, 95, 135, 175, 215, 255];
                    let offset = index - 16;
                    Rgb {
                        r: cube[offset / 36],
                        g: cube[offset / 6 % 6],
                        b: cube[offset % 6],
                    }
                })
            }),
        }
    }
}

impl Default for TerminalTheme {
    fn default() -> Self {
        Self::from_colors(0x111016, 0xe5e4eb, 0xec4899)
    }
}
