use crate::{Error, Result, Rgb, Terminal};

/// Parser defaults, not renderer-only colors. Values are bounded RGB bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalColors {
    pub foreground: Rgb,
    pub background: Rgb,
    /// None clears the configured cursor default.
    pub cursor: Option<Rgb>,
    /// Exactly 256 entries, or None to restore Ghostty's built-in palette.
    pub palette: Option<[Rgb; 256]>,
}
impl From<Rgb> for [u8; 3] {
    fn from(value: Rgb) -> Self {
        [value.r, value.g, value.b]
    }
}
impl Terminal {
    /// Sets Ghostty's configured defaults for current and future terminal text.
    /// Application OSC color overrides remain authoritative until the application
    /// resets them. Explicit truecolor cells are not recolored by a theme change.
    pub fn set_colors(&mut self, colors: &TerminalColors) -> Result<()> {
        let palette = colors.palette.as_ref().map(|p| p.map(Into::into));
        self.engine
            .set_colors(
                colors.foreground.into(),
                colors.background.into(),
                colors.cursor.map(Into::into),
                palette.as_ref(),
            )
            .map_err(Error)
    }
}
