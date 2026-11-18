//! Opt-in Native002 ABI. No symbol in this module is referenced by default builds.
use std::{ffi::c_void, marker::PhantomData, ptr::NonNull, rc::Rc};

mod ansi;
mod snapshot;
mod terminal;
mod verify;
pub use ansi::{AnsiLimits, NativeAnsi};
pub use snapshot::{
    CrossTargetNative0RawV1Snapshot, Native0RawV1Compatibility, NativeSnapshot, ProfileState,
    SnapshotLimits, SnapshotMetadata, SnapshotReport,
};
pub use terminal::{RestorationConfig, TerminalProfile};
pub use verify::{EngineIdentity, VerifiedNativeExtension};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    Native(i32),
    InvalidInput(&'static str),
    InvalidOutput(&'static str),
    AbiMismatch,
    IdentityMismatch,
    UnsupportedLegacy,
    UnsupportedAnsi(u64),
    Allocation,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for Error {}
pub type Result<T> = std::result::Result<T, Error>;
fn check(code: i32) -> Result<()> {
    match code {
        0 => Ok(()),
        -8..=-1 => Err(Error::Native(code)),
        _ => Err(Error::InvalidOutput("unknown result code")),
    }
}
fn allocation(len: usize, cap: usize) -> Result<Vec<u8>> {
    if len > cap {
        return Err(Error::InvalidInput("allocation exceeds binding limit"));
    }
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(len)
        .map_err(|_| Error::Allocation)?;
    bytes.resize(len, 0);
    Ok(bytes)
}
#[repr(C)]
#[derive(Clone, Copy)]
struct ByteString {
    ptr: *const u8,
    len: usize,
}
impl ByteString {
    fn from(bytes: &[u8]) -> Self {
        Self {
            ptr: bytes.as_ptr(),
            len: bytes.len(),
        }
    }
}
#[repr(C)]
struct ExtensionInfo {
    size: usize,
    abi_version: u32,
    ansi_abi_version: u32,
    terminal_profile_abi_version: u32,
    snapshot_budget_abi_version: u32,
    snapshot_format_version: u32,
    features: u64,
    engine_profile: ByteString,
    legacy_width_profile: ByteString,
}
#[repr(C)]
struct ProfileOptions {
    size: usize,
    abi_version: u32,
    engine_profile: ByteString,
    terminal_profile: u32,
    cols: u16,
    rows: u16,
}
#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct RawState {
    size: usize,
    abi_version: u32,
    terminal_profile: u32,
    preceding_join: u32,
}
impl RawState {
    fn native() -> Self {
        Self {
            size: std::mem::size_of::<Self>(),
            abi_version: 1,
            terminal_profile: 0,
            preceding_join: 0,
        }
    }
}
#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct RawReport {
    size: usize,
    abi_version: u32,
    reserved: u32,
    source_offset: u64,
    heap_bytes: u64,
    mapped_bytes: u64,
    work_units: u64,
}
impl RawReport {
    fn new() -> Self {
        Self {
            size: std::mem::size_of::<Self>(),
            abi_version: 1,
            reserved: 0,
            source_offset: 0,
            heap_bytes: 0,
            mapped_bytes: 0,
            work_units: 0,
        }
    }
}
#[repr(C)]
struct DecodeOptions {
    size: usize,
    abi_version: u32,
    profile: u32,
    ghostty_version: u32,
    snapshot_version: u32,
    engine_profile: ByteString,
    profile_state: RawState,
    flags: u32,
    reserved: u32,
    max_encoded_bytes: u64,
    max_allocation_bytes: u64,
    max_work_units: u64,
    max_cells: u64,
    max_history_rows: u64,
    max_continuation_bytes: u64,
    max_image_bytes: u64,
    max_decompressed_bytes: u64,
    runtime_image_storage_limit: u64,
    runtime_image_io_policy: u32,
    runtime_resize_pull_scrollback: u32,
    max_columns: u32,
    max_rows: u32,
    max_pixel_width: u32,
    max_pixel_height: u32,
    max_page_rows: u32,
    max_pages: u32,
    max_records: u32,
    max_images: u32,
}
#[repr(C)]
struct EncodeOptions {
    size: usize,
    abi_version: u32,
    profile: u32,
    ghostty_version: u32,
    snapshot_version: u32,
    engine_profile: ByteString,
    terminal_profile: u32,
    flags: u32,
    reserved: u32,
    max_encoded_bytes: u64,
    max_allocation_bytes: u64,
    max_work_units: u64,
    max_cells: u64,
    max_history_rows: u64,
    max_continuation_bytes: u64,
    max_image_bytes: u64,
    max_decompressed_bytes: u64,
    runtime_image_storage_limit: u64,
    runtime_image_io_policy: u32,
    runtime_resize_pull_scrollback: u32,
    max_columns: u32,
    max_rows: u32,
    max_pixel_width: u32,
    max_pixel_height: u32,
    max_page_rows: u32,
    max_pages: u32,
    max_records: u32,
    max_images: u32,
}
#[repr(C)]
struct AnsiOptions {
    size: usize,
    abi_version: u32,
    target: u32,
    target_features: u64,
    engine_profile: ByteString,
    max_output_bytes: usize,
    max_rows: usize,
    max_cells: usize,
    max_continuation_bytes: usize,
    default_foreground: *const u8,
    default_background: *const u8,
    default_cursor: *const u8,
    default_palette: *const u8,
    default_cursor_style: u32,
    default_cursor_blink: i32,
    runtime_resize_pull_scrollback: u32,
    target_is_fresh: u8,
    target_scrollback_max_bytes: usize,
    target_scrollback_max_rows: usize,
    runtime_image_storage_limit: u64,
}
unsafe extern "C" {
    fn sf_experimental_native_borrow(adapter: *mut c_void, out: *mut *mut c_void) -> i32;
    fn sf_experimental_native_adopt(owned: *mut *mut c_void, out: *mut *mut c_void) -> i32;
    fn ghostty_shellfox_extension_info(info: *mut ExtensionInfo) -> i32;
    fn ghostty_shellfox_terminal_new_profile(
        allocator: *const c_void,
        out: *mut *mut c_void,
        options: *const ProfileOptions,
    ) -> i32;
    fn ghostty_shellfox_terminal_get_profile(terminal: *mut c_void, out: *mut u32) -> i32;
    fn ghostty_shellfox_terminal_get_profile_state(
        terminal: *mut c_void,
        out: *mut RawState,
    ) -> i32;
    fn ghostty_shellfox_snapshot_budget_encode_buf(
        source: *mut c_void,
        options: *const EncodeOptions,
        output: *mut u8,
        capacity: usize,
        written: *mut usize,
        state: *mut RawState,
        report: *mut RawReport,
    ) -> i32;
    fn ghostty_shellfox_snapshot_budget_decode_buf(
        bytes: *const u8,
        length: usize,
        options: *const DecodeOptions,
        out: *mut *mut c_void,
        report: *mut RawReport,
    ) -> i32;
    fn ghostty_terminal_export_ansi_full_buf(
        source: *mut c_void,
        options: *const AnsiOptions,
        buffer: *mut u8,
        capacity: usize,
        written: *mut usize,
        unsupported: *mut u64,
    ) -> i32;
    fn ghostty_terminal_free(terminal: *mut c_void);
    fn ghostty_terminal_set(terminal: *mut c_void, option: i32, value: *const c_void) -> i32;
    fn ghostty_terminal_get(terminal: *mut c_void, data: i32, out: *mut c_void) -> i32;
}

#[cfg(test)]
mod layout_tests {
    use super::*;
    #[test]
    #[cfg(target_pointer_width = "64")]
    fn native002_layouts_match_checked_headers() {
        assert_eq!(std::mem::size_of::<ExtensionInfo>(), 72);
        assert_eq!(std::mem::size_of::<ProfileOptions>(), 40);
        assert_eq!(std::mem::size_of::<RawState>(), 24);
        assert_eq!(std::mem::size_of::<DecodeOptions>(), 184);
        assert_eq!(std::mem::size_of::<EncodeOptions>(), 168);
        assert_eq!(std::mem::size_of::<RawReport>(), 48);
        assert_eq!(std::mem::size_of::<AnsiOptions>(), 144);
        assert_eq!(
            std::mem::offset_of!(DecodeOptions, runtime_resize_pull_scrollback),
            148
        );
        assert_eq!(
            std::mem::offset_of!(EncodeOptions, runtime_resize_pull_scrollback),
            132
        );
    }
}
