use super::*;

unsafe extern "C" {
    fn sf_key(
        vt: *mut c_void,
        key: u32,
        action: u32,
        mods: u32,
        consumed_mods: u32,
        unshifted: u32,
        composing: u8,
        text: *const u8,
        text_len: usize,
        out: *mut u8,
        cap: usize,
        len: *mut usize,
    ) -> i32;
    fn sf_paste(
        vt: *mut c_void,
        text: *mut u8,
        text_len: usize,
        out: *mut u8,
        cap: usize,
        len: *mut usize,
    ) -> i32;
    fn sf_kitty_key_flags(vt: *mut c_void, out: *mut u8) -> i32;
    fn sf_focus(vt: *mut c_void, focused: u8, out: *mut u8, cap: usize, len: *mut usize) -> i32;
}

/// The public encoders are stateless between their size query and copy.
/// Reject changed lengths rather than publishing a partial encoding.
fn encoded_output(mut call: impl FnMut(*mut u8, usize, &mut usize) -> i32) -> Result<Vec<u8>> {
    let mut len = 0;
    let r = call(std::ptr::null_mut(), 0, &mut len);
    if r != 0 && r != -3 {
        return Err(r);
    }
    let mut out = buffer(len)?;
    check(call(out.as_mut_ptr(), out.len(), &mut len))?;
    if len != out.len() {
        return Err(-2);
    }
    Ok(out)
}

impl Engine {
    /// Encodes with the terminal's live modes: DECCKM, keypad, kitty flags,
    /// modifyOtherKeys and alt-esc-prefix.
    pub fn encode_key(&mut self, event: &KeyEvent<'_>) -> Result<Vec<u8>> {
        self.status()?;
        event.validate().map_err(InvalidKey::native_code)?;
        encoded_output(|out, cap, len| {
            // SAFETY: event text outlives both synchronous calls and is freed by C before return.
            unsafe {
                sf_key(
                    self.ptr.as_ptr(),
                    event.physical.code(),
                    event.action as u32,
                    u32::from(event.modifiers),
                    u32::from(event.consumed_modifiers),
                    event.unshifted_codepoint.map_or(0, u32::from),
                    u8::from(event.composing),
                    event.text.as_ptr(),
                    event.text.len(),
                    out,
                    cap,
                    len,
                )
            }
        })
    }
    /// The kitty keyboard flags the application pushed; zero is legacy encoding.
    pub fn kitty_key_flags(&mut self) -> Result<u8> {
        self.status()?;
        let mut out = 0;
        // SAFETY: initialized output and a unique live terminal owner.
        unsafe {
            check(sf_kitty_key_flags(self.ptr.as_ptr(), &mut out))?;
        }
        Ok(out)
    }
    /// User paste framed per mode 2004. Multi-line text is never refused.
    pub fn encode_paste(&mut self, text: &str) -> Result<Vec<u8>> {
        self.status()?;
        let mut data = buffer(text.len())?;
        data.copy_from_slice(text.as_bytes());
        // Ghostty may rewrite unsafe bytes in the temporary copy; the rewrite is
        // idempotent, so the size query and copy produce the same encoding.
        encoded_output(|out, cap, len| {
            // SAFETY: both allocations have their specified lengths; C retains neither.
            unsafe {
                sf_paste(
                    self.ptr.as_ptr(),
                    data.as_mut_ptr(),
                    data.len(),
                    out,
                    cap,
                    len,
                )
            }
        })
    }
    /// Focus report, or nothing when the application has not enabled mode 1004.
    pub fn encode_focus(&mut self, focused: bool) -> Result<Vec<u8>> {
        self.status()?;
        encoded_output(|out, cap, len| {
            // SAFETY: the output allocation has its specified length.
            unsafe { sf_focus(self.ptr.as_ptr(), u8::from(focused), out, cap, len) }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copy_rejects_shrinking_or_growing_native_lengths() {
        for changed in [0, 2] {
            assert_eq!(
                encoded_output(|out, _, len| {
                    *len = if out.is_null() { 1 } else { changed };
                    if out.is_null() {
                        -3
                    } else {
                        0
                    }
                }),
                Err(-2)
            );
        }
    }

    #[test]
    fn copy_bounds_query_and_discards_partial_error() {
        let mut calls = 0;
        assert_eq!(
            encoded_output(|_, _, len| {
                calls += 1;
                *len = LIMIT + 1;
                -3
            }),
            Err(-6)
        );
        assert_eq!(calls, 1);
        assert_eq!(
            encoded_output(|out, _, len| {
                *len = 1;
                if !out.is_null() {
                    // SAFETY: the queried one-byte output allocation is live.
                    unsafe {
                        out.write(b'x');
                    }
                }
                -3
            }),
            Err(-3)
        );
    }

    #[test]
    fn no_output_and_native_failures_stay_distinct() {
        assert_eq!(
            encoded_output(|_, _, len| {
                *len = 0;
                0
            }),
            Ok(Vec::new())
        );
        assert_eq!(encoded_output(|_, _, _| -8), Err(-8));
    }
}
