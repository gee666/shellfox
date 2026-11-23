//! The single bounded admission book lives in LiveActivitySink's short mutex.
use super::InputActivity;
#[cfg(test)]
mod tests;
use crate::activity::meaningful_input;
use shellfox_ghostty_vt::MouseAction;
use shellfox_host::CapturedSettlement;
use shellfox_runtime::{
    Identity, InputKind, InputReceipt, RequestId, COMMAND_CAPACITY, MAX_ENCODED_INPUT_BYTES,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InputContextFailure {
    Full,
    Duplicate,
    Missing,
    Stale,
    InvalidReceipt,
    Unavailable,
    Bounds,
}
impl std::fmt::Display for InputContextFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Full => "Input admission queue is full.",
            Self::Duplicate => "Input request is already admitted.",
            Self::Missing => "Captured input admission is missing.",
            Self::Stale => "Input generation is stale.",
            Self::InvalidReceipt => "Input receipt does not match the admitted write.",
            Self::Unavailable => "Input activity metadata is unavailable.",
            Self::Bounds => "Input admission exceeds its bound.",
        })
    }
}
impl std::error::Error for InputContextFailure {}
/// Original UI admission context. No encoded text/password or recaptured marker.
pub struct InputAdmission {
    pub user_initiated: bool,
    pub hint: bool,
    pub captured_settlement: CapturedSettlement,
    raw: Option<(u32, bool)>,
}
impl InputAdmission {
    pub fn raw(
        bytes: &[u8],
        user_initiated: bool,
        hint: bool,
        captured_settlement: CapturedSettlement,
    ) -> Result<Self, InputContextFailure> {
        if bytes.len() > MAX_ENCODED_INPUT_BYTES {
            return Err(InputContextFailure::Bounds);
        }
        let length = u32::try_from(bytes.len()).map_err(|_| InputContextFailure::Bounds)?;
        Ok(Self {
            user_initiated,
            hint,
            captured_settlement,
            raw: Some((length, meaningful_input(bytes, true))),
        })
    }
    pub fn native(
        user_initiated: bool,
        hint: bool,
        captured_settlement: CapturedSettlement,
    ) -> Self {
        Self {
            user_initiated,
            hint,
            captured_settlement,
            raw: None,
        }
    }
}
pub struct AcceptedInputContext {
    pub identity: Identity,
    pub request_id: RequestId,
    pub admission: InputAdmission,
    pub receipt: InputReceipt,
    pub activity: InputActivity,
}
struct PendingInput {
    request: RequestId,
    admission: InputAdmission,
    prepared: Option<(InputKind, u32, bool)>,
}
#[derive(Default)]
pub(super) struct InputContexts {
    pending: Vec<PendingInput>,
}
impl InputContexts {
    pub(super) fn register(
        &mut self,
        request: RequestId,
        admission: InputAdmission,
    ) -> Result<(), InputContextFailure> {
        if self
            .pending
            .iter()
            .any(|pending| pending.request.0 == request.0)
        {
            return Err(InputContextFailure::Duplicate);
        }
        if self.pending.len() >= COMMAND_CAPACITY {
            return Err(InputContextFailure::Full);
        }
        self.pending.push(PendingInput {
            request,
            admission,
            prepared: None,
        });
        Ok(())
    }
    pub(super) fn prepare(
        &mut self,
        request: RequestId,
        kind: InputKind,
        encoded: &[u8],
    ) -> Result<bool, InputContextFailure> {
        if encoded.is_empty() || encoded.len() > MAX_ENCODED_INPUT_BYTES {
            return Err(InputContextFailure::Bounds);
        }
        let pending = self
            .pending
            .iter_mut()
            .find(|pending| pending.request.0 == request.0)
            .ok_or(InputContextFailure::Missing)?;
        if pending.prepared.is_some() {
            return Err(InputContextFailure::Duplicate);
        }
        let length = u32::try_from(encoded.len()).map_err(|_| InputContextFailure::Bounds)?;
        let meaningful = meaningful_input(encoded, pending.admission.user_initiated);
        pending.prepared = Some((kind, length, meaningful));
        Ok(meaningful)
    }
    pub(super) fn finish(
        &mut self,
        identity: Identity,
        request: RequestId,
        receipt: Option<InputReceipt>,
    ) -> Result<Option<AcceptedInputContext>, InputContextFailure> {
        let index = self
            .pending
            .iter()
            .position(|pending| pending.request.0 == request.0)
            .ok_or(InputContextFailure::Missing)?;
        let pending = self.pending.remove(index);
        let Some(receipt) = receipt else {
            return Ok(None);
        };
        if usize::try_from(receipt.accepted_bytes).map_err(|_| InputContextFailure::Bounds)?
            > MAX_ENCODED_INPUT_BYTES
        {
            return Err(InputContextFailure::InvalidReceipt);
        }
        let meaningful = if receipt.accepted_bytes == 0 {
            if pending.prepared.is_some() {
                return Err(InputContextFailure::InvalidReceipt);
            }
            false
        } else if let Some((kind, length, meaningful)) = pending.prepared {
            if kind != receipt.kind || length != receipt.accepted_bytes {
                return Err(InputContextFailure::InvalidReceipt);
            }
            meaningful
        } else {
            // Reporting may be disabled, but original admission intent remains.
            // Mechanical receipts authorize no input/control and contain no text.
            let mechanical = match receipt.kind {
                InputKind::Raw => match pending.admission.raw {
                    Some((length, meaningful)) if length == receipt.accepted_bytes => meaningful,
                    _ => return Err(InputContextFailure::InvalidReceipt),
                },
                InputKind::Key { .. } | InputKind::Paste { .. } => true,
                InputKind::Mouse {
                    action,
                    button,
                    any_button_pressed,
                } => !(action == MouseAction::Motion && button.is_none() && !any_button_pressed),
            };
            pending.admission.user_initiated && mechanical
        };
        Ok(Some(AcceptedInputContext {
            identity,
            request_id: request,
            admission: pending.admission,
            receipt,
            activity: InputActivity {
                touch_session: meaningful,
                unsettle: false,
            },
        }))
    }
    pub(super) fn cancel(&mut self, request: RequestId) -> bool {
        if let Some(index) = self
            .pending
            .iter()
            .position(|pending| pending.request.0 == request.0)
        {
            self.pending.remove(index);
            true
        } else {
            false
        }
    }
    pub(super) fn clear(&mut self) -> usize {
        let count = self.pending.len();
        self.pending.clear();
        count
    }
}
