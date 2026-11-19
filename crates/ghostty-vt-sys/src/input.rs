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
    fn sf_xterm_input_mode(vt: *mut c_void, mode: u8, out: *mut u8) -> i32;
    fn sf_paste(
        vt: *mut c_void,
        text: *mut u8,
        text_len: usize,
        out: *mut u8,
        cap: usize,
        len: *mut usize,
    ) -> i32;
}

/// The public key encoder is stateless between its size query and copy.
/// Reject changed lengths rather than publishing a partial encoding.
fn encoded_key_output(mut call: impl FnMut(*mut u8, usize, &mut usize) -> i32) -> Result<Vec<u8>> {
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

#[derive(Clone, Copy)]
#[repr(u8)]
pub enum XtermInputMode {
    ApplicationCursorKeys = 0,
    BracketedPaste = 1,
    FocusReporting = 2,
}

impl Engine {
    pub fn encode_key(&mut self, key: Key<'_>) -> Result<Vec<u8>> {
        self.status()?;
        key_v2::validate_raw(&key).map_err(InvalidKeyV2::native_code)?;
        self.encode_checked_key(key)
    }
    fn encode_checked_key(&mut self, key: Key<'_>) -> Result<Vec<u8>> {
        encoded_key_output(|out, cap, len| {
            // SAFETY: event text outlives both synchronous calls and is freed by C before return.
            unsafe {
                sf_key(
                    self.ptr.as_ptr(),
                    key.key,
                    key.action,
                    key.mods,
                    key.consumed_mods,
                    key.unshifted,
                    u8::from(key.composing),
                    key.text.as_ptr(),
                    key.text.len(),
                    out,
                    cap,
                    len,
                )
            }
        })
    }
    pub fn encode_key_v2(&mut self, event: &KeyV2<'_>) -> Result<Vec<u8>> {
        self.status()?;
        event.validate().map_err(InvalidKeyV2::native_code)?;
        self.encode_checked_key(event.raw())
    }
    pub fn xterm_input_mode(&self, mode: XtermInputMode) -> Result<bool> {
        self.status()?;
        let mut enabled = 0;
        // SAFETY: the mode query borrows the live terminal and one writable byte.
        unsafe { check(sf_xterm_input_mode(self.ptr.as_ptr(), mode as u8, &mut enabled))? };
        Ok(enabled != 0)
    }
    pub fn encode_paste(&mut self, text: &str) -> Result<Vec<u8>> {
        self.status()?;
        let mut data = buffer(text.len())?;
        data.copy_from_slice(text.as_bytes());
        let mut len = 0;
        // SAFETY: C may modify only the temporary input copy; it never retains either buffer.
        let r = unsafe {
            sf_paste(
                self.ptr.as_ptr(),
                data.as_mut_ptr(),
                data.len(),
                std::ptr::null_mut(),
                0,
                &mut len,
            )
        };
        if r != 0 && r != -3 {
            return Err(r);
        }
        let mut out = buffer(len)?;
        // SAFETY: both input and output allocations have their specified lengths.
        unsafe {
            check(sf_paste(
                self.ptr.as_ptr(),
                data.as_mut_ptr(),
                data.len(),
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_copy_rejects_shrinking_or_growing_native_lengths() {
        for changed in [0, 2] {
            assert_eq!(
                encoded_key_output(|out, _, len| {
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
    fn key_copy_bounds_query_and_discards_partial_error() {
        let mut calls = 0;
        assert_eq!(
            encoded_key_output(|_, _, len| {
                calls += 1;
                *len = LIMIT + 1;
                -3
            }),
            Err(-6)
        );
        assert_eq!(calls, 1);
        assert_eq!(
            encoded_key_output(|out, _, len| {
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
            encoded_key_output(|_, _, len| {
                *len = 0;
                0
            }),
            Ok(Vec::new())
        );
        assert_eq!(encoded_key_output(|_, _, _| -8), Err(-8));
    }
}
