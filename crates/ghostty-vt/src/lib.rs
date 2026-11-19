//! Thread-confined terminal emulation and input encoding. No GUI or PTY dependencies.
use shellfox_ghostty_vt_sys as sys;
mod colors;
#[cfg(feature = "experimental-shellfox-native")]
pub mod extension;
mod input;
mod input_v2;
mod xterm_keys;
pub use colors::TerminalColors;
mod metadata;
mod mouse;
mod presentation;
mod recovery;
mod selection;
pub use input::{Key, KeyAction, KeyEvent, Modifiers};
pub use input_v2::{InvalidKeyV2, KeyActionV2, KeyEventV2, ModifiersV2, PhysicalKey};
pub use metadata::{
    TerminalMetadata, MAX_METADATA_CWD_BYTES, MAX_METADATA_TITLE_BYTES, MAX_METADATA_TITLE_UNITS,
    MAX_PENDING_BELLS, MAX_RAW_TITLE_BYTES,
};
pub use mouse::{MouseAction, MouseButton, MouseEvent, MouseGeometry};
pub use presentation::{
    CursorShape, CursorVisual, HistoryPoint, PresentationBuffer, PresentationFrame,
    PresentationSelection, PresentationSelectionKind, PresentationView, RowMetadata,
    HARD_PRESENTATION_CELLS, MAX_PRESENTATION_CELLS, MAX_PRESENTATION_VIEWS,
};
pub use recovery::{RecoveryInfo, CONTINUATION_MAX_BYTES, FORMAT_EXTRAS_ALL, MAX_FORMAT_BYTES};
pub use selection::{Selection, ViewportPoint};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Error(pub i32);
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let description = match self.0 {
            -1 => "allocation failed",
            -2 => "invalid value",
            -3 => "buffer too small",
            -4 => "no value",
            -5 => "I/O failed",
            -6 => "resource limit exceeded",
            -7 => "unsafe paste rejected",
            -8 => "unsupported library or operation",
            _ => "unknown libghostty-vt error",
        };
        write!(f, "{description} ({})", self.0)
    }
}
impl std::error::Error for Error {}
pub type Result<T> = std::result::Result<T, Error>;
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}
impl From<[u8; 3]> for Rgb {
    fn from([r, g, b]: [u8; 3]) -> Self {
        Self { r, g, b }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cell {
    pub text: String,
    pub foreground: Rgb,
    pub background: Rgb,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub inverse: bool,
    pub faint: bool,
    pub invisible: bool,
    pub strikethrough: bool,
    /// Zero means a wide-character spacer, two means the leading wide cell.
    pub width: u8,
    pub selected: bool,
    /// Whether Ghostty associates OSC 8 metadata with this cell.
    pub has_hyperlink: bool,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cursor {
    pub x: u16,
    pub y: u16,
    pub visible: bool,
}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Screen {
    pub cols: u16,
    pub rows: u16,
    pub cells: Vec<Cell>,
    pub cursor: Cursor,
}
/// The sole authoritative parser for one view. Never feed historical data into
/// this instance if its replies will be sent back to a live PTY.
///
/// A terminal cannot cross threads, even when wrapped in a mutex.
/// ```compile_fail
/// fn require_send<T: Send>() {}
/// require_send::<shellfox_ghostty_vt::Terminal>();
/// ```
/// ```compile_fail
/// fn require_sync<T: Sync>() {}
/// require_sync::<shellfox_ghostty_vt::Terminal>();
/// ```
pub struct Terminal {
    engine: sys::Engine,
    cols: u16,
    rows: u16,
}
impl Terminal {
    pub fn new(cols: u16, rows: u16) -> Result<Self> {
        Ok(Self {
            engine: sys::Engine::new(cols, rows).map_err(Error)?,
            cols,
            rows,
        })
    }
    /// Input chunks are limited to 1 MiB. Call repeatedly for larger PTY reads.
    /// A callback overflow poisons the engine; stop its PTY on that error.
    pub fn feed(&mut self, bytes: &[u8]) -> Result<()> {
        self.engine.feed(bytes).map_err(Error)
    }
    pub fn resize(&mut self, cols: u16, rows: u16) -> Result<()> {
        self.engine.resize(cols, rows).map_err(Error)?;
        self.cols = cols;
        self.rows = rows;
        Ok(())
    }
    /// Drains copied query replies exactly once. Also drain after resize.
    pub fn drain_replies(&mut self) -> Result<Vec<u8>> {
        self.engine.drain_replies().map_err(Error)
    }
    pub fn render(&mut self) -> Result<Screen> {
        let (raw, cursor) = self.engine.render().map_err(Error)?;
        let mut cells = Vec::new();
        cells.try_reserve_exact(raw.len()).map_err(|_| Error(-1))?;
        cells.extend(raw.into_iter().map(Cell::from_native));
        Ok(Screen {
            cols: self.cols,
            rows: self.rows,
            cells,
            cursor: Cursor {
                x: cursor.x,
                y: cursor.y,
                visible: cursor.visible != 0,
            },
        })
    }
    pub fn encode_key(&mut self, event: &KeyEvent) -> Result<Vec<u8>> {
        self.engine.status().map_err(Error)?;
        self.engine.encode_key(event.native()?).map_err(Error)
    }
    /// Encodes with the terminal's current bracketed-paste mode. Unsafe
    /// unbracketed multiline input is rejected, not silently confirmed.
    pub fn encode_paste(&mut self, text: &str) -> Result<Vec<u8>> {
        self.engine.encode_paste(text).map_err(Error)
    }
    pub fn scroll(&mut self, delta: isize) -> Result<()> {
        self.engine.scroll(delta).map_err(Error)
    }
}
