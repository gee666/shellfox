use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecoveryInfo {
    pub cols: u16,
    pub rows: u16,
    pub alternate: bool,
    pub pending_wrap: bool,
    pub scrollback_rows: u64,
}
#[repr(C)]
#[derive(Default)]
struct RecoveryView {
    scrollback_rows: u64,
    cols: u16,
    rows: u16,
    alternate: u8,
    pending_wrap: u8,
}
unsafe extern "C" {
    fn sf_format_vt(vt: *mut c_void, extras: u32, out: *mut u8, cap: usize, len: *mut usize)
        -> i32;
    fn sf_continuation(vt: *mut c_void, out: *mut u8, cap: usize, len: *mut usize) -> i32;
    fn sf_recovery_info(vt: *mut c_void, out: *mut RecoveryView) -> i32;
}
impl Engine {
    pub fn format_vt(&mut self, extras: u32, max_bytes: usize) -> Result<Vec<u8>> {
        self.status()?;
        if extras & !0xFFF != 0 {
            return Err(-2);
        }
        if max_bytes > MAX_FORMAT_BYTES {
            return Err(-6);
        }
        // SAFETY: each synchronous call creates/frees only a formatter, never
        // mutates the terminal, and writes within the supplied allocation.
        owned_bytes(max_bytes, |out, cap, len| unsafe {
            sf_format_vt(self.ptr.as_ptr(), extras, out, cap, len)
        })
    }
    pub fn continuation(&mut self) -> Result<Vec<u8>> {
        // SAFETY: the public API copies bytes without changing the parser.
        // Tracking was enabled before any input and all access is serialized.
        owned_bytes(CONTINUATION_MAX_BYTES, |out, cap, len| unsafe {
            sf_continuation(self.ptr.as_ptr(), out, cap, len)
        })
    }
    pub fn recovery_info(&mut self) -> Result<RecoveryInfo> {
        let mut info = RecoveryView::default();
        // SAFETY: initialized output has the same C layout; all getters are read-only.
        unsafe {
            check(sf_recovery_info(self.ptr.as_ptr(), &mut info))?;
        }
        dimensions(info.cols, info.rows)?;
        if info.cols != self.cols
            || info.rows != self.rows
            || info.alternate > 1
            || info.pending_wrap > 1
        {
            return Err(-2);
        }
        Ok(RecoveryInfo {
            cols: info.cols,
            rows: info.rows,
            alternate: info.alternate != 0,
            pending_wrap: info.pending_wrap != 0,
            scrollback_rows: info.scrollback_rows,
        })
    }
}
