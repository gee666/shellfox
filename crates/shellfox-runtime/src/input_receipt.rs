use crate::{owner::Owner, *};
use shellfox_ghostty_vt::{KeyAction, MouseAction, MouseButton};

/// Mechanical encoding facts only. These are not settlement tokens, user intent,
/// activity policy, or permission to resume a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputKind {
    Key {
        action: KeyAction,
        has_committed_text: bool,
    },
    Paste {
        nonempty: bool,
    },
    Mouse {
        action: MouseAction,
        button: Option<MouseButton>,
        any_button_pressed: bool,
    },
    Raw,
}
/// No raw input, password, coordinates or recaptured UI context is returned.
/// Nonzero confirms the complete encoded write, not shell read/execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InputReceipt {
    pub accepted_bytes: u32,
    pub kind: InputKind,
}
impl Owner {
    pub(crate) fn accept_input(
        &mut self,
        request_id: RequestId,
        bytes: &[u8],
        kind: InputKind,
    ) -> Result<Response, RuntimeError> {
        self.active()?;
        if bytes.len() > MAX_ENCODED_INPUT_BYTES {
            let stamped = self
                .delivery
                .lease()
                .is_some_and(|lease| self.layouts.controller_opted(lease));
            return Err(RuntimeError {
                code: if stamped { ErrorCode::ResourceLimit } else { ErrorCode::Validation },
                message: if stamped { "Encoded PTY input exceeds the 64 KiB single-write budget; no input was written." } else { "encoded PTY input exceeds 64 KiB" }.into(),
            });
        }
        if !bytes.is_empty() {
            self.live_output.before_input(
                self.identity,
                request_id,
                kind,
                bytes,
                &self.shared.live_output_status,
            );
            self.write(bytes)?;
        }
        // Ignored encodings still return Input(0); they are not Applied and do
        // not invoke the supplier or even enter the PTY write helper.
        let accepted_bytes = u32::try_from(bytes.len())
            .map_err(|_| RuntimeError::validation("encoded input length is invalid"))?;
        Ok(Response::Input(InputReceipt {
            accepted_bytes,
            kind,
        }))
    }
    pub(crate) fn follow_accepted_input(&mut self, lease: Lease) {
        if let Err(error) = self.follow_input(lease) {
            // A display failure cannot erase an already-confirmed write receipt.
            self.set_phase(self.phase, Some(error.message));
        }
    }
}
