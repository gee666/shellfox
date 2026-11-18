use crate::{Error, Result, Terminal};

pub use shellfox_ghostty_vt_sys::{
    TerminalMetadata, MAX_METADATA_CWD_BYTES, MAX_METADATA_TITLE_BYTES, MAX_METADATA_TITLE_UNITS,
    MAX_PENDING_BELLS, MAX_RAW_TITLE_BYTES,
};

impl Terminal {
    /// Takes the latest volatile title/CWD snapshot with bell/rejection counters
    /// since the previous successful take. None means no pending metadata.
    /// Version increases independently of render frames. Reads, presentation
    /// views, resize and snapshot formatting do not manufacture or drain updates.
    ///
    /// Titles follow the existing 200-UTF-16-unit presentation limit, with C0/C1
    /// controls removed and Unicode whitespace trimmed. Over-4KiB or invalid-UTF-8
    /// raw title updates are rejected. CWD accepts bounded file:// URI-shaped
    /// values, rejecting credentials, ports, query/fragment and raw/decoded
    /// controls. Invalid metadata keeps the last accepted value and increments
    /// the bounded rejection counter, without poisoning the parser or replies.
    ///
    /// CWD remains an untrusted URI, never an authorized filesystem path. Public
    /// Ghostty PWD callbacks do not distinguish OSC 7 from other PWD protocols.
    /// The host must validate host/profile/path policy before using it. Neither
    /// field is a user title, command record, persistent setting or permission.
    /// This method never opens a path, launches a link, rings an OS bell, accesses
    /// the clipboard or emits PTY input. Historical mirrors must not publish
    /// their replayed effects as authoritative live events.
    pub fn take_metadata(&mut self) -> Result<Option<TerminalMetadata>> {
        self.engine.take_metadata().map_err(Error)
    }
}
