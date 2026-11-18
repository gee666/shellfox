use super::*;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Mouse {
    pub action: u32,
    /// Zero means no button; otherwise a public GhosttyMouseButton value.
    pub button: u32,
    pub mods: u32,
    pub cell_width: u32,
    pub cell_height: u32,
    pub x: f32,
    pub y: f32,
    pub any_button_pressed: u8,
}
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Selection {
    /// Range, word, line or all, numbered 0 through 3.
    pub kind: u32,
    pub start_x: u16,
    pub start_y: u16,
    pub end_x: u16,
    pub end_y: u16,
    pub rectangle: u8,
}
unsafe extern "C" {
    fn sf_mouse_tracking(vt: *mut c_void, out: *mut u8) -> i32;
    fn sf_mouse(
        vt: *mut c_void,
        event: *const Mouse,
        out: *mut u8,
        cap: usize,
        len: *mut usize,
    ) -> i32;
    fn sf_selection(vt: *mut c_void, selection: *const Selection) -> i32;
    fn sf_copy_selection(vt: *mut c_void, out: *mut u8, cap: usize, len: *mut usize) -> i32;
    fn sf_hyperlink(
        vt: *mut c_void,
        x: u16,
        y: u16,
        out: *mut u8,
        cap: usize,
        len: *mut usize,
    ) -> i32;
}

/// Copies bounded UTF-8 output. Neither call may mutate the terminal or retain refs.
fn owned_text(call: impl FnMut(*mut u8, usize, &mut usize) -> i32) -> Result<String> {
    String::from_utf8(owned_bytes(LIMIT, call)?).map_err(|_| -2)
}

impl Engine {
    pub fn mouse_tracking(&mut self) -> Result<bool> {
        let mut out = 0;
        // SAFETY: initialized output and a unique live terminal owner.
        unsafe {
            check(sf_mouse_tracking(self.ptr.as_ptr(), &mut out))?;
        }
        match out {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(-2),
        }
    }
    pub fn encode_mouse(&mut self, event: Mouse) -> Result<Vec<u8>> {
        self.status()?;
        // The public encoder's SGR pixel path rounds to signed 32-bit integers.
        // Keep finite positions and geometry well within that conversion range.
        let valid_dimension = |cell: u32, count: u16| {
            cell != 0
                && cell
                    .checked_mul(u32::from(count))
                    .is_some_and(|v| v <= 1_000_000_000)
        };
        if event.action > 2
            || event.button > 11
            || event.mods & !15 != 0
            || event.any_button_pressed > 1
            || (event.action != 2 && event.button == 0)
            || !valid_dimension(event.cell_width, self.cols)
            || !valid_dimension(event.cell_height, self.rows)
            || !event.x.is_finite()
            || !event.y.is_finite()
            || event.x.abs() > 1_000_000_000.0
            || event.y.abs() > 1_000_000_000.0
        {
            return Err(-2);
        }
        // A single encode avoids consuming state during a two-pass size query.
        // All public formats fit within 128 bytes with the bounded numeric inputs.
        let mut out = buffer(128)?;
        let mut len = 0;
        // SAFETY: C reads the event synchronously and writes only within the buffer.
        unsafe {
            check(sf_mouse(
                self.ptr.as_ptr(),
                &event,
                out.as_mut_ptr(),
                out.len(),
                &mut len,
            ))?;
        }
        if len > out.len() {
            return Err(-2);
        }
        out.truncate(len);
        Ok(out)
    }
    pub fn set_selection(&mut self, selection: Option<Selection>) -> Result<()> {
        self.status()?;
        if let Some(s) = selection {
            if s.kind > 3 || s.rectangle > 1 {
                return Err(-2);
            }
            if s.kind != 3 {
                self.viewport_point(s.start_x, s.start_y)?;
                if s.kind == 0 {
                    self.viewport_point(s.end_x, s.end_y)?;
                }
            }
        }
        // SAFETY: C creates and installs fresh refs before returning; no raw ref escapes.
        unsafe {
            check(sf_selection(
                self.ptr.as_ptr(),
                selection
                    .as_ref()
                    .map_or(std::ptr::null(), std::ptr::from_ref),
            ))
        }
    }
    pub fn copy_selection(&mut self) -> Result<Option<String>> {
        // SAFETY: only the unique owner accesses the terminal; each call creates and
        // frees its formatter while its fresh selection snapshot remains valid.
        let result = owned_text(|out, cap, len| unsafe {
            sf_copy_selection(self.ptr.as_ptr(), out, cap, len)
        });
        match result {
            Err(-4) => Ok(None),
            result => result.map(Some),
        }
    }
    fn viewport_point(&self, x: u16, y: u16) -> Result<()> {
        if x >= self.cols || y >= self.rows {
            Err(-2)
        } else {
            Ok(())
        }
    }
    pub fn hyperlink_at(&mut self, x: u16, y: u16) -> Result<Option<String>> {
        self.status()?;
        self.viewport_point(x, y)?;
        // SAFETY: C resolves a fresh ref each time and copies within the supplied buffer.
        let uri = owned_text(|out, cap, len| unsafe {
            sf_hyperlink(self.ptr.as_ptr(), x, y, out, cap, len)
        })?;
        Ok(if uri.is_empty() { None } else { Some(uri) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oversize_query_never_attempts_copy() {
        let mut calls = 0;
        assert_eq!(
            owned_text(|_, _, len| {
                calls += 1;
                *len = LIMIT + 1;
                -3
            }),
            Err(-6)
        );
        assert_eq!(calls, 1);
    }

    #[test]
    fn empty_owned_text_and_native_errors() {
        assert_eq!(
            owned_text(|_, _, len| {
                *len = 0;
                0
            }),
            Ok(String::new())
        );
        assert_eq!(owned_text(|_, _, _| -4), Err(-4));
        assert_eq!(owned_text(|_, _, _| -6), Err(-6));
    }

    #[test]
    fn changed_native_length_is_rejected() {
        assert_eq!(
            owned_text(|out, _, len| {
                *len = if out.is_null() { 1 } else { 2 };
                if out.is_null() {
                    -3
                } else {
                    0
                }
            }),
            Err(-2)
        );
    }

    #[test]
    fn invalid_utf8_is_rejected_without_lossy_conversion() {
        assert_eq!(
            owned_text(|out, cap, len| {
                *len = 1;
                if out.is_null() {
                    return -3;
                }
                assert_eq!(cap, 1);
                // SAFETY: owned_text supplied a live one-byte output allocation.
                unsafe {
                    out.write(0xff);
                }
                0
            }),
            Err(-2)
        );
    }
}
