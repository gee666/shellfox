//! ANSI recovery is a negotiated projection, never a binary Ghostty snapshot.
use crate::RuntimeError;
use shellfox_ghostty_vt::{Terminal, FORMAT_EXTRAS_ALL};

pub(crate) const MAX_RECOVERY_BYTES: usize = 4 * 1024 * 1024;

#[cfg(feature = "experimental-shellfox-native")]
impl crate::Runtime {
    /// Explicit new-source factory for trusted remote-eligible composition.
    /// This requests non-elevation on Unix before any PTY/supervisor creation.
    /// It grants no remote authority and never upgrades an existing source.
    /// Ordinary/local constructors keep their caller's Local/sudo behavior.
    pub fn start_remote_eligible_native0_program_with_limits_and_hooks(
        identity: crate::Identity,
        config: shellfox_terminal::PtyConfig,
        limits: crate::GridLimits,
        program_env: Option<shellfox_terminal::TrustedProgramEnv>,
        hooks: crate::RuntimeHooks,
        native: crate::Native0RuntimeConfig,
    ) -> Result<Self, crate::StartError> {
        #[cfg(unix)]
        let config = {
            let mut config = config;
            config.unix_privilege = crate::UnixPrivilegeMode::NonElevating;
            config
        };
        Self::start_native0_program_with_limits_and_hooks(
            identity,
            config,
            limits,
            program_env,
            hooks,
            native,
        )
    }
}

// CAN cancels an unfinished receiver sequence before RIS clears old content,
// history, margins and modes. Do not complete an old OSC with ST: that could
// execute a clipboard request left in the receiver's parser.
const RESET: &[u8] = b"\x18\x1bc";
// Public vt-1.0.1 formatter extras, excluding process-controlled OSC 7 metadata.
// Cell hyperlinks remain untrusted metadata; the client must apply its normal
// link policy. Cursor hyperlinks are restored for subsequent live output.
const FORMATTER_EXTRAS: u32 = FORMAT_EXTRAS_ALL & !(1 << 4);

/// Builds FormattedActiveBufferV1 from public getters only, without replaying history
/// into any parser or draining the authoritative terminal's pending PTY replies.
/// The caller freezes output/resize ordering at one sequence, sends this payload
/// before later chunks, and requires a recovery ACK before resuming delivery.
///
/// This includes active-buffer history and the formatter's supported extras,
/// followed by exact unfinished UTF-8/VT continuation bytes. It is not a claim
/// that every terminal mode, inactive buffer or saved cursor was restored.
/// vt-1.0.1's formatter can itself overwrite the saved cursor when restoring
/// pending wrap. FullState must not be advertised using this function.
/// Receivers must be passive mirrors: no query replies or clipboard effects.
pub(crate) fn active_buffer_ansi(terminal: &mut Terminal) -> Result<Vec<u8>, RuntimeError> {
    bounded_active_buffer_ansi(terminal, MAX_RECOVERY_BYTES)
}

fn bounded_active_buffer_ansi(
    terminal: &mut Terminal,
    max_bytes: usize,
) -> Result<Vec<u8>, RuntimeError> {
    // Read continuation before formatting. If tracking was disabled or exceeded
    // its bound, refuse the whole payload, not a prefix that loses parser state.
    let continuation = terminal.continuation().map_err(RuntimeError::unavailable)?;
    let budget = max_bytes
        .checked_sub(RESET.len())
        .and_then(|remaining| remaining.checked_sub(continuation.len()))
        .ok_or_else(|| RuntimeError::validation("ANSI continuation exceeds recovery limit"))?;
    let formatted = terminal
        .format_vt(FORMATTER_EXTRAS, budget)
        .map_err(RuntimeError::unavailable)?;
    let len = RESET
        .len()
        .checked_add(formatted.len())
        .and_then(|len| len.checked_add(continuation.len()))
        .filter(|len| *len <= max_bytes)
        .ok_or_else(|| RuntimeError::validation("ANSI recovery exceeds 4 MiB"))?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(len)
        .map_err(RuntimeError::unavailable)?;
    bytes.extend_from_slice(RESET);
    bytes.extend_from_slice(&formatted);
    // This must remain last. No SGR, title, cursor or mode escape may follow an
    // unfinished parser suffix before the next ordered live output chunk.
    bytes.extend_from_slice(&continuation);
    Ok(bytes)
}

#[cfg(feature = "experimental-shellfox-native")]
mod native0 {
    use super::*;
    use shellfox_ghostty_vt::extension::{
        NativeSnapshot, RestorationConfig, SnapshotLimits, TerminalProfile, VerifiedNativeExtension,
    };
    /// Required, explicit source-creation configuration. Not inferred from an
    /// incoming recovery request or retroactively applied to an existing source.
    #[derive(Debug, Clone, Copy)]
    pub struct Native0RuntimeConfig {
        configuration: RestorationConfig,
        limits: SnapshotLimits,
    }
    impl Native0RuntimeConfig {
        pub fn new(
            configuration: RestorationConfig,
            limits: SnapshotLimits,
        ) -> Result<Self, RuntimeError> {
            if limits.max_encoded_bytes > MAX_RECOVERY_BYTES
                || limits.max_continuation_bytes > 65536
                || limits.max_allocation_bytes > 512 * 1024 * 1024
                || limits.max_work_units > 100_000_000_000
                || limits.max_cells > 16 * 1024 * 1024
                || limits.max_history_rows > 1_048_576
                || limits.max_columns > 4096
                || limits.max_rows > 4096
                || limits.max_pixel_width > 65536
                || limits.max_pixel_height > 65536
                || limits.max_page_rows > 65535
                || limits.max_pages > 65536
                || limits.max_records > 1_048_576
                || configuration.runtime_image_storage_limit > usize::MAX as u64
            {
                return Err(RuntimeError::validation(
                    "Native0 creation/configuration budget is unsupported.",
                ));
            }
            Ok(Self {
                configuration,
                limits,
            })
        }
        pub fn configuration(self) -> RestorationConfig {
            self.configuration
        }
        pub fn limits(self) -> SnapshotLimits {
            self.limits
        }
    }
    pub(crate) struct Native0Creation {
        verified: VerifiedNativeExtension,
        config: Native0RuntimeConfig,
    }
    impl Native0Creation {
        pub(crate) fn create(
            cols: u16,
            rows: u16,
            config: Native0RuntimeConfig,
        ) -> Result<(Terminal, Self), RuntimeError> {
            let verified = VerifiedNativeExtension::verify().map_err(unavailable)?;
            let terminal = verified
                .new_native_terminal(
                    cols,
                    rows,
                    TerminalProfile::Native,
                    config.configuration,
                    config.limits.max_continuation_bytes,
                )
                .map_err(unavailable)?;
            Ok((terminal, Self { verified, config }))
        }
        pub(crate) fn capture(
            &self,
            terminal: &mut Terminal,
        ) -> Result<(NativeSnapshot, SnapshotLimits), RuntimeError> {
            let (snapshot, _report) = terminal
                .encode_native_snapshot(
                    &self.verified,
                    self.config.limits,
                    self.config.configuration,
                )
                .map_err(unavailable)?;
            Ok((snapshot, self.config.limits))
        }
    }
    fn unavailable(_error: impl std::fmt::Display) -> RuntimeError {
        RuntimeError{code:crate::ErrorCode::Unsupported,message:"Native0 recovery is unsupported under the verified source profile/configuration/budgets.".into()}
    }
}
#[cfg(feature = "experimental-shellfox-native")]
pub(crate) use native0::Native0Creation;
#[cfg(feature = "experimental-shellfox-native")]
pub use native0::Native0RuntimeConfig;

#[cfg(test)]
#[path = "recovery_tests.rs"]
mod tests;
