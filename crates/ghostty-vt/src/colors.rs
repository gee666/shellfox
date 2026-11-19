use crate::{Error, Result, Rgb, Terminal};

/// Parser defaults, not renderer-only colors. Values are bounded RGB bytes.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TerminalColors {
    pub foreground: Rgb,
    pub background: Rgb,
    /// None clears the configured cursor default.
    pub cursor: Option<Rgb>,
    /// Exactly 256 entries, or None to restore Ghostty's built-in palette.
    #[serde(default, with = "palette_wire")]
    pub palette: Option<[Rgb; 256]>,
}

mod palette_wire {
    use super::Rgb;
    use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error};

    pub(super) fn serialize<S: Serializer>(palette: &Option<[Rgb; 256]>, serializer: S) -> Result<S::Ok, S::Error> {
        palette.as_ref().map(<[Rgb; 256]>::as_slice).serialize(serializer)
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<[Rgb; 256]>, D::Error> {
        Option::<Vec<Rgb>>::deserialize(deserializer)?
            .map(|colors| colors.try_into().map_err(|colors: Vec<Rgb>| D::Error::invalid_length(colors.len(), &"exactly 256 RGB entries")))
            .transpose()
    }
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
