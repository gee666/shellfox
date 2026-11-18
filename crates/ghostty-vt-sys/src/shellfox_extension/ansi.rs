use super::*;
use crate::Engine;
#[derive(Debug, Clone, Copy)]
pub struct AnsiLimits {
    pub max_output_bytes: usize,
    pub max_rows: usize,
    pub max_cells: usize,
    pub max_continuation_bytes: usize,
}
impl Default for AnsiLimits {
    fn default() -> Self {
        Self {
            max_output_bytes: 4 * 1024 * 1024,
            max_rows: 65536,
            max_cells: 1_048_576,
            max_continuation_bytes: 65536,
        }
    }
}
impl AnsiLimits {
    fn validate(self) -> Result<()> {
        if self.max_output_bytes > 16 * 1024 * 1024
            || self.max_rows == 0
            || self.max_rows > 1_048_576
            || self.max_cells == 0
            || self.max_cells > 16 * 1024 * 1024
            || self.max_continuation_bytes > 65536
        {
            return Err(Error::InvalidInput("ANSI binding limits"));
        }
        Ok(())
    }
}
/// Native accepted-profile BYTE projection, never universal FullState or Legacy.
/// Applying it is offered only through a newly constructed passive Engine.
#[derive(Debug)]
pub struct NativeAnsi {
    bytes: Vec<u8>,
    identity: EngineIdentity,
    config: RestorationConfig,
    cols: u16,
    rows: u16,
    scrollback_bytes: usize,
    scrollback_rows: usize,
    continuation_limit: usize,
}
impl NativeAnsi {
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }
    pub fn dimensions(&self) -> (u16, u16) {
        (self.cols, self.rows)
    }
    pub fn restore(&self, verified: &VerifiedNativeExtension) -> Result<Engine> {
        if self.identity != verified.identity {
            return Err(Error::IdentityMismatch);
        }
        let mut target = verified.new_native_engine(
            self.cols,
            self.rows,
            TerminalProfile::Native,
            self.config,
            self.continuation_limit,
        )?;
        let raw = terminal::borrow(&mut target)?;
        // SAFETY: this is ONLY the fresh target just created above. Set declared
        // policy before any input; no decoded/current terminal is reset.
        unsafe {
            check(ghostty_terminal_set(
                raw,
                28,
                std::ptr::from_ref(&self.scrollback_bytes).cast(),
            ))?;
            check(ghostty_terminal_set(
                raw,
                29,
                std::ptr::from_ref(&self.scrollback_rows).cast(),
            ))?;
        }
        for chunk in self.bytes.chunks(1_048_576) {
            target.feed(chunk).map_err(Error::Native)?;
            terminal::borrow(&mut target)?;
        }
        verified.profile_state(&mut target)?;
        Ok(target)
    }
}
fn scrollback(raw: *mut c_void, key: i32) -> Result<usize> {
    let mut value = 0usize;
    // SAFETY: public scrollback queries return size_t; NO_VALUE means unlimited.
    let code = unsafe { ghostty_terminal_get(raw, key, std::ptr::from_mut(&mut value).cast()) };
    if code == -4 {
        return Ok(usize::MAX);
    }
    check(code)?;
    Ok(value)
}
fn result(code: i32, reasons: u64) -> Result<()> {
    if reasons & !0x1fff != 0 {
        return Err(Error::InvalidOutput("unknown ANSI reason bits"));
    }
    if code == -8 && reasons != 0 {
        return Err(Error::UnsupportedAnsi(reasons));
    }
    if reasons != 0 {
        return Err(Error::InvalidOutput(
            "ANSI reasons without unsupported result",
        ));
    }
    check(code)
}
impl VerifiedNativeExtension {
    pub fn export_native_ansi(
        &self,
        engine: &mut Engine,
        limits: AnsiLimits,
        config: RestorationConfig,
    ) -> Result<NativeAnsi> {
        limits.validate()?;
        config.validate()?;
        self.profile_state(engine)?;
        let raw = terminal::borrow(engine)?;
        let (cols, rows) = terminal::grid(raw)?;
        let scrollback_bytes = scrollback(raw, 34)?;
        let scrollback_rows = scrollback(raw, 35)?;
        // Canonical constructor reset baseline: callers cannot silently export
        // different reset defaults. The native preflight checks these values.
        let options = AnsiOptions {
            size: std::mem::size_of::<AnsiOptions>(),
            abi_version: 1,
            target: 0,
            target_features: 1,
            engine_profile: ByteString::from(&self.identity.0),
            max_output_bytes: limits.max_output_bytes,
            max_rows: limits.max_rows,
            max_cells: limits.max_cells,
            max_continuation_bytes: limits.max_continuation_bytes,
            default_foreground: std::ptr::null(),
            default_background: std::ptr::null(),
            default_cursor: std::ptr::null(),
            default_palette: std::ptr::null(),
            default_cursor_style: 1,
            default_cursor_blink: 0,
            runtime_resize_pull_scrollback: u32::from(config.runtime_resize_pull_scrollback),
            target_is_fresh: 1,
            target_scrollback_max_bytes: scrollback_bytes,
            target_scrollback_max_rows: scrollback_rows,
            runtime_image_storage_limit: config.runtime_image_storage_limit,
        };
        let mut needed = 0usize;
        let mut reasons = 0u64;
        // SAFETY: one exclusive Engine borrow spans both read-only passes; every
        // borrowed option/profile value remains live and is never retained.
        let code = unsafe {
            ghostty_terminal_export_ansi_full_buf(
                raw,
                &options,
                std::ptr::null_mut(),
                0,
                &mut needed,
                &mut reasons,
            )
        };
        if code != -3 {
            result(code, reasons)?;
            return Err(Error::InvalidOutput("ANSI size-query result"));
        }
        if reasons != 0 || needed == 0 {
            return Err(Error::InvalidOutput("ANSI size-query outputs"));
        }
        let mut bytes = allocation(needed, limits.max_output_bytes)?;
        reasons = 0;
        let code = unsafe {
            ghostty_terminal_export_ansi_full_buf(
                raw,
                &options,
                bytes.as_mut_ptr(),
                bytes.len(),
                &mut needed,
                &mut reasons,
            )
        };
        result(code, reasons)?;
        if needed != bytes.len() {
            return Err(Error::InvalidOutput("ANSI copy length"));
        }
        Ok(NativeAnsi {
            bytes,
            identity: self.identity,
            config,
            cols,
            rows,
            scrollback_bytes,
            scrollback_rows,
            continuation_limit: limits.max_continuation_bytes,
        })
    }
}
