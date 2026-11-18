use super::*;

unsafe extern "C" {
    fn sf_colors(
        vt: *mut c_void,
        fg: *const u8,
        bg: *const u8,
        cursor: *const u8,
        palette: *const u8,
    ) -> i32;
}
impl Engine {
    pub fn set_colors(
        &mut self,
        foreground: [u8; 3],
        background: [u8; 3],
        cursor: Option<[u8; 3]>,
        palette: Option<&[[u8; 3]; 256]>,
    ) -> Result<()> {
        // SAFETY: fixed-size byte arrays match the adapter's documented lengths.
        // C copies all values during this synchronous call. No cast assumes the
        // layout of a native GhosttyColorRgb struct.
        unsafe {
            check(sf_colors(
                self.ptr.as_ptr(),
                foreground.as_ptr(),
                background.as_ptr(),
                cursor.as_ref().map_or(std::ptr::null(), |v| v.as_ptr()),
                palette.map_or(std::ptr::null(), |v| v.as_flattened().as_ptr()),
            ))
        }
    }
}
