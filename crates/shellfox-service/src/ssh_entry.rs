//! One early native-mode router. Footprints select a candidate, never grant trust.
use super::broker;
use anyhow::Result;
use std::ffi::OsString;
#[derive(Debug, PartialEq, Eq)]
enum Mode {
    SupervisorOrNormal,
    Callback,
    InvalidCallback,
}
fn classify(arguments: &[OsString], callback: bool) -> Mode {
    if !callback {
        Mode::SupervisorOrNormal
    } else if arguments.len() == 1 {
        Mode::Callback
    } else {
        Mode::InvalidCallback
    }
}
/// Call before normal arguments/logging/GUI. Genuine supervisor bootstrap is
/// authenticated by terminal's existing private gates. Target callback descriptors
/// must never leak into those modes. An arbitrary prompt is not a supervisor flag.
/// Some preserves the complete native u32 exit code; None is normal startup.
pub fn dispatch_early_entry(arguments: &[OsString]) -> Result<Option<u32>> {
    match classify(arguments, broker::has_askpass_environment()) {
        Mode::InvalidCallback => Ok(Some(1)),
        Mode::Callback => Ok(Some(u32::from_ne_bytes(
            broker::askpass_entry(arguments).unwrap_or(1).to_ne_bytes(),
        ))),
        Mode::SupervisorOrNormal => {
            if let Some(code) = shellfox_terminal::supervisor_entry()? {
                return Ok(Some(code));
            }
            if let Some(code) = shellfox_terminal::raw_process::supervisor_entry() {
                return Ok(Some(code));
            }
            Ok(None)
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn callback_prompt_text_is_not_mode_authority_and_mixed_extra_args_fail_closed() {
        for prompt in [
            "--shellfox-pty-supervisor",
            "--shellfox-raw-supervisor",
            "ordinary prompt",
        ] {
            assert_eq!(classify(&[prompt.into()], true), Mode::Callback);
            assert_eq!(classify(&[prompt.into()], false), Mode::SupervisorOrNormal);
        }
        assert_eq!(classify(&[], true), Mode::InvalidCallback);
        assert_eq!(
            classify(
                &[
                    "--shellfox-pty-supervisor".into(),
                    "private-bootstrap".into()
                ],
                true
            ),
            Mode::InvalidCallback
        );
        for code in [0u32, 1, 0x80000000, u32::MAX] {
            assert_eq!(
                u32::from_ne_bytes(i32::from_ne_bytes(code.to_ne_bytes()).to_ne_bytes()),
                code
            );
        }
    }
}
