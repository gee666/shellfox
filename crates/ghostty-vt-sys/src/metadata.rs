use super::*;

pub const MAX_METADATA_TITLE_UNITS: usize = 200;
pub const MAX_METADATA_TITLE_BYTES: usize = 600;
pub const MAX_RAW_TITLE_BYTES: usize = 4096;
pub const MAX_METADATA_CWD_BYTES: usize = 32767;
pub const MAX_PENDING_BELLS: u32 = 65535;

/// Volatile, process-controlled metadata. Never a host permission or saved title.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalMetadata {
    pub version: u64,
    pub title: Option<String>,
    pub cwd_uri: Option<String>,
    /// Bell events since the previous successful take, capped at MAX_PENDING_BELLS.
    pub bell_count: u32,
    /// More bells arrived than bell_count can represent within the configured cap.
    pub bells_saturated: bool,
    /// Wrapper-observed rejected callback values, capped at MAX_PENDING_BELLS.
    /// Ghostty may reject an OSC before any callback, which cannot be counted.
    pub rejected_updates: u32,
}
#[repr(C)]
#[derive(Default)]
struct MetadataView {
    version: u64,
    title: *const u8,
    title_len: usize,
    cwd_uri: *const u8,
    cwd_uri_len: usize,
    bell_count: u32,
    rejected_updates: u32,
    bells_saturated: u8,
}
unsafe extern "C" {
    fn sf_metadata_peek(vt: *mut c_void, out: *mut MetadataView) -> i32;
    fn sf_metadata_ack(vt: *mut c_void, version: u64) -> i32;
}

fn copy_text(ptr: *const u8, len: usize, limit: usize) -> Result<Option<String>> {
    if len > limit {
        return Err(-6);
    }
    if len == 0 {
        return Ok(None);
    }
    if ptr.is_null() {
        return Err(-2);
    }
    // SAFETY: the private adapter returns one of its fixed C-owned metadata
    // buffers. Length is bounded here and no native call intervenes before copy.
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
    let text = std::str::from_utf8(bytes).map_err(|_| -2)?;
    if text.chars().any(char::is_control) {
        return Err(-2);
    }
    let mut owned = String::new();
    owned.try_reserve_exact(text.len()).map_err(|_| -1)?;
    owned.push_str(text);
    Ok(Some(owned))
}
impl Engine {
    /// Reads one coalesced snapshot and only acknowledges after every fallible
    /// allocation/validation succeeds. No operation drains the PTY reply queue.
    pub fn take_metadata(&mut self) -> Result<Option<TerminalMetadata>> {
        let mut view = MetadataView::default();
        // SAFETY: the unique, thread-confined engine owns all callback storage.
        let result = unsafe { sf_metadata_peek(self.ptr.as_ptr(), &mut view) };
        if result == -4 {
            return Ok(None);
        }
        check(result)?;
        if view.version == 0
            || view.bell_count > MAX_PENDING_BELLS
            || view.rejected_updates > MAX_PENDING_BELLS
            || view.bells_saturated > 1
            || (view.bells_saturated != 0 && view.bell_count != MAX_PENDING_BELLS)
        {
            return Err(-2);
        }
        let title = copy_text(view.title, view.title_len, MAX_METADATA_TITLE_BYTES)?;
        if title.as_ref().is_some_and(|text| {
            text.encode_utf16().count() > MAX_METADATA_TITLE_UNITS || text.trim() != text
        }) {
            return Err(-2);
        }
        let cwd_uri = copy_text(view.cwd_uri, view.cwd_uri_len, MAX_METADATA_CWD_BYTES)?;
        if cwd_uri.as_ref().is_some_and(|text| {
            !text
                .as_bytes()
                .get(..7)
                .is_some_and(|prefix| prefix.eq_ignore_ascii_case(b"file://"))
        }) {
            return Err(-2);
        }
        let snapshot = TerminalMetadata {
            version: view.version,
            title,
            cwd_uri,
            bell_count: view.bell_count,
            bells_saturated: view.bells_saturated != 0,
            rejected_updates: view.rejected_updates,
        };
        // SAFETY: no native call or concurrent mutation can change this version
        // between peek and ack. C clears only pending counters, not owned text.
        unsafe {
            check(sf_metadata_ack(self.ptr.as_ptr(), snapshot.version))?;
        }
        Ok(Some(snapshot))
    }
}

#[cfg(test)]
#[path = "metadata_tests.rs"]
mod tests;
