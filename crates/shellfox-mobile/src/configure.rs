use crate::{
    abi::{boundary, bytes},
    engine::{self, Status},
};
use serde::Deserialize;
use shellfox_ghostty_vt::{Rgb, TerminalColors};

#[derive(Clone, Copy, Deserialize)]
#[serde(try_from = "String")]
struct Hex(Rgb);
impl TryFrom<String> for Hex {
    type Error = &'static str;
    fn try_from(value: String) -> std::result::Result<Self, Self::Error> {
        if value.len() != 7
            || !value.starts_with('#')
            || !value.as_bytes()[1..].iter().all(u8::is_ascii_hexdigit)
        {
            return Err("expected #RRGGBB");
        }
        let value = u32::from_str_radix(&value[1..], 16).map_err(|_| "invalid RGB")?;
        Ok(Self(Rgb {
            r: (value >> 16) as u8,
            g: (value >> 8) as u8,
            b: value as u8,
        }))
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Config {
    foreground: Hex,
    background: Hex,
    cursor: Option<Hex>,
    palette: Option<Vec<Hex>>,
    selection_background: Option<Hex>,
    selection_foreground: Option<Hex>,
}
impl Config {
    fn colors(self) -> std::result::Result<TerminalColors, Status> {
        let palette = self
            .palette
            .map(|colors| {
                let colors: Vec<_> = colors.into_iter().map(|c| c.0).collect();
                colors.try_into().map_err(|_| Status::Invalid)
            })
            .transpose()?;
        // Native renderer reads these same strictly validated RGB values after
        // configure succeeds. Selection styling is not terminal parser state.
        let _renderer_colors = (self.selection_background, self.selection_foreground);
        Ok(TerminalColors {
            foreground: self.foreground.0,
            background: self.background.0,
            cursor: self.cursor.map(|c| c.0),
            palette,
        })
    }
}
/// # Safety
/// json must reference length readable bytes for this call.
#[no_mangle]
pub unsafe extern "C" fn sfm_terminal_configure(
    handle: u64,
    json: *const u8,
    length: usize,
) -> i32 {
    boundary(|| {
        // SAFETY: forwarded C input memory contract.
        let data = unsafe { bytes(json, length, 16384)? };
        let config = serde_json::from_slice::<Config>(data).map_err(|_| Status::Invalid)?;
        let colors = config.colors()?;
        engine::with_engine(handle, |owned| {
            owned
                .terminal
                .set_colors(&colors)
                .map_err(|_| Status::Engine)?;
            owned.colors = Some(colors);
            Ok(())
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn strict_rgb_and_palette_limits() {
        for value in [
            r##"{"foreground":"red","background":"#000000"}"##,
            r##"{"foreground":"#+00000","background":"#000000"}"##,
            r##"{"foreground":"#ffffff","background":"#000000","secret":"bad"}"##,
        ] {
            assert!(serde_json::from_str::<Config>(value).is_err());
        }
        let config = serde_json::from_str::<Config>(
            r##"{"foreground":"#ffffff","background":"#000000","palette":["#000000"]}"##,
        )
        .unwrap();
        assert!(config.colors().is_err());
    }
}
