use crate::{Error, Result, Terminal};

pub use shellfox_ghostty_vt_sys::{RecoveryInfo, CONTINUATION_MAX_BYTES, MAX_FORMAT_BYTES};
/// All public vt-1.0.1 formatter extras. This does not mean full terminal state.
pub const FORMAT_EXTRAS_ALL: u32 = 0xFFF;
impl Terminal {
    /// Formats the active screen as VT through Ghostty's public formatter.
    /// Trimming and unwrapping are disabled. Extras must be a subset of 0xFFF.
    /// The byte limit must be at most MAX_FORMAT_BYTES. Exceeding either limit
    /// fails without truncation or changing the source terminal/reply queue.
    ///
    /// This is not a full-state snapshot. The public formatter does not expose
    /// the inactive screen or saved cursors. It does not append unfinished parser
    /// state. Capability negotiation must account for these gaps.
    pub fn format_vt(&mut self, extras: u32, max_bytes: usize) -> Result<Vec<u8>> {
        self.engine.format_vt(extras, max_bytes).map_err(Error)
    }
    /// Copies Ghostty's replay-safe unfinished parser/UTF-8 suffix, not terminal
    /// text. Tracking starts at creation and is capped at 64 KiB. An unavailable
    /// continuation is an error, never silently treated as ground. The returned
    /// bytes may be incomplete UTF-8 and are not converted to a string.
    pub fn continuation(&mut self) -> Result<Vec<u8>> {
        self.engine.continuation().map_err(Error)
    }
    /// Reads the public recovery indicators without changing either screen,
    /// parser, viewport, selection or pending query replies.
    pub fn recovery_info(&mut self) -> Result<RecoveryInfo> {
        self.engine.recovery_info().map_err(Error)
    }
}
