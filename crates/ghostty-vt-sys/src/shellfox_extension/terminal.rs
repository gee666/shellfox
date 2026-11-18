use super::*;
use crate::Engine;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalProfile {
    Native,
    LegacyXterm6,
}
/// Required configuration, not resource budgets. No default or IO callback is supplied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RestorationConfig {
    pub runtime_image_storage_limit: u64,
    pub runtime_resize_pull_scrollback: bool,
}
impl RestorationConfig {
    pub(super) fn validate(self) -> Result<()> {
        if self.runtime_image_storage_limit > usize::MAX as u64 {
            return Err(Error::InvalidInput("runtime image limit"));
        }
        Ok(())
    }
}
/// Private RAII staging, never a second public terminal abstraction.
pub(super) struct Staging {
    handle: Option<NonNull<c_void>>,
}
impl Drop for Staging {
    fn drop(&mut self) {
        if let Some(handle) = self.handle {
            // SAFETY: still owns the newly returned public handle, not transferred to SfVt.
            unsafe { ghostty_terminal_free(handle.as_ptr()) }
        }
    }
}
impl Staging {
    pub(super) fn result(code: i32, raw: *mut c_void) -> Result<Self> {
        let pending = Self {
            handle: NonNull::new(raw),
        };
        check(code)?;
        if pending.handle.is_none() {
            return Err(Error::InvalidOutput("null terminal on success"));
        }
        Ok(pending)
    }
    pub(super) fn ptr(&self) -> Result<*mut c_void> {
        self.handle
            .map(NonNull::as_ptr)
            .ok_or(Error::InvalidOutput("consumed terminal"))
    }
    pub(super) fn dimensions(&self) -> Result<(u16, u16)> {
        grid(self.ptr()?)
    }
    pub(super) fn state(&self) -> Result<ProfileState> {
        state(self.ptr()?)
    }
    pub(super) fn adopt(mut self) -> Result<Engine> {
        let (cols, rows) = self.dimensions()?;
        self.state()?;
        let original = self.ptr()?;
        let mut owned = original;
        let mut adapter = std::ptr::null_mut();
        // SAFETY: this exclusively owned fresh constructor/decoder handle has no
        // callbacks. C allocates all storage before its infallible transfer commit.
        let code = unsafe { sf_experimental_native_adopt(&mut owned, &mut adapter) };
        if code != 0 {
            if owned != original || !adapter.is_null() {
                self.handle = None;
                // SAFETY: an unexpected adapter output is owned by this failed transfer.
                if !adapter.is_null() {
                    unsafe { crate::sf_free(adapter) }
                } else if !owned.is_null() {
                    unsafe { ghostty_terminal_free(owned) }
                }
                return Err(Error::InvalidOutput("failed adoption ownership contract"));
            }
            check(code)?;
        }
        if !owned.is_null() || adapter.is_null() {
            self.handle = None;
            // SAFETY: no terminal is published; clean the sole returned ownership path.
            if !adapter.is_null() {
                unsafe { crate::sf_free(adapter) }
            } else if !owned.is_null() {
                unsafe { ghostty_terminal_free(owned) }
            }
            return Err(Error::InvalidOutput(
                "successful adoption ownership contract",
            ));
        }
        self.handle = None;
        let mut engine = Engine {
            ptr: NonNull::new(adapter).ok_or(Error::InvalidOutput("null adapter"))?,
            cols,
            rows,
            _thread: PhantomData,
            presentation_owner: Rc::new(()),
        };
        engine.recovery_info().map_err(Error::Native)?;
        Ok(engine)
    }
}
pub(super) fn borrow(engine: &mut Engine) -> Result<*mut c_void> {
    engine.status().map_err(Error::Native)?;
    let mut out = std::ptr::null_mut();
    // SAFETY: unique Engine borrow remains live throughout the immediate FFI call.
    unsafe {
        check(sf_experimental_native_borrow(engine.ptr.as_ptr(), &mut out))?;
    }
    if out.is_null() {
        return Err(Error::InvalidOutput("null borrowed terminal"));
    }
    Ok(out)
}
pub(super) fn grid(terminal: *mut c_void) -> Result<(u16, u16)> {
    let mut cols = 0u16;
    let mut rows = 0u16;
    // SAFETY: called only with a live borrowed/staged handle; outputs are initialized u16.
    unsafe {
        check(ghostty_terminal_get(
            terminal,
            1,
            std::ptr::from_mut(&mut cols).cast(),
        ))?;
        check(ghostty_terminal_get(
            terminal,
            2,
            std::ptr::from_mut(&mut rows).cast(),
        ))?;
    }
    if cols == 0 || rows == 0 || usize::from(cols) * usize::from(rows) > 1_048_576 {
        return Err(Error::InvalidOutput("terminal dimensions"));
    }
    Ok((cols, rows))
}
fn state(terminal: *mut c_void) -> Result<ProfileState> {
    let mut profile = u32::MAX;
    let mut out = RawState::native();
    // SAFETY: live handle and exact, initialized public outputs.
    unsafe {
        check(ghostty_shellfox_terminal_get_profile(
            terminal,
            &mut profile,
        ))?;
        check(ghostty_shellfox_terminal_get_profile_state(
            terminal, &mut out,
        ))?;
    }
    if profile == 1 {
        return Err(Error::UnsupportedLegacy);
    }
    if profile != 0 {
        return Err(Error::InvalidOutput("unknown terminal profile"));
    }
    ProfileState::checked(out)
}
impl VerifiedNativeExtension {
    pub fn new_native_engine(
        &self,
        cols: u16,
        rows: u16,
        profile: TerminalProfile,
        config: RestorationConfig,
        continuation_limit: usize,
    ) -> Result<Engine> {
        if profile != TerminalProfile::Native {
            return Err(Error::UnsupportedLegacy);
        }
        config.validate()?;
        if cols == 0
            || rows == 0
            || usize::from(cols) * usize::from(rows) > 1_048_576
            || continuation_limit > 65536
        {
            return Err(Error::InvalidInput("native dimensions/continuation limit"));
        }
        let options = ProfileOptions {
            size: std::mem::size_of::<ProfileOptions>(),
            abi_version: 1,
            engine_profile: ByteString::from(&self.identity.0),
            terminal_profile: 0,
            cols,
            rows,
        };
        let mut raw = std::ptr::null_mut();
        // SAFETY: fresh handle only, no callbacks and no live-terminal profile setter.
        let code =
            unsafe { ghostty_shellfox_terminal_new_profile(std::ptr::null(), &mut raw, &options) };
        let pending = Staging::result(code, raw)?;
        let ptr = pending.ptr()?;
        let style: i32 = 1;
        let blink = false;
        let pull = config.runtime_resize_pull_scrollback;
        // SAFETY: configuration is applied ONLY to this pristine constructor,
        // never to a decoded terminal. Every value has its public option type.
        unsafe {
            check(ghostty_terminal_set(
                ptr,
                32,
                std::ptr::from_ref(&continuation_limit).cast(),
            ))?;
            check(ghostty_terminal_set(
                ptr,
                16,
                std::ptr::from_ref(&config.runtime_image_storage_limit).cast(),
            ))?;
            check(ghostty_terminal_set(
                ptr,
                41,
                std::ptr::from_ref(&pull).cast(),
            ))?;
            check(ghostty_terminal_set(
                ptr,
                23,
                std::ptr::from_ref(&style).cast(),
            ))?;
            check(ghostty_terminal_set(
                ptr,
                24,
                std::ptr::from_ref(&blink).cast(),
            ))?;
        }
        pending.adopt()
    }
    pub fn profile_state(&self, engine: &mut Engine) -> Result<ProfileState> {
        state(borrow(engine)?)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn failed_bridge_retains_staged_ownership_and_can_then_adopt() -> Result<()> {
        let verified = VerifiedNativeExtension::verify()?;
        let options = ProfileOptions {
            size: std::mem::size_of::<ProfileOptions>(),
            abi_version: 1,
            engine_profile: ByteString::from(&verified.identity.0),
            terminal_profile: 0,
            cols: 8,
            rows: 3,
        };
        let mut raw = std::ptr::null_mut();
        let code =
            unsafe { ghostty_shellfox_terminal_new_profile(std::ptr::null(), &mut raw, &options) };
        let pending = Staging::result(code, raw)?;
        let original = pending.ptr()?;
        let mut same = original;
        // SAFETY: intentional invalid NULL output exercises the documented no-transfer path.
        assert_eq!(
            unsafe { sf_experimental_native_adopt(&mut same, std::ptr::null_mut()) },
            -2
        );
        assert_eq!(same, original);
        assert_eq!(pending.state()?.preceding_join, 0);
        let mut engine = pending.adopt()?;
        assert!(engine.drain_replies().map_err(Error::Native)?.is_empty());
        Ok(())
    }
}
