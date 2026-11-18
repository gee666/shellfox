use super::CursorError;
use shellfox_protocol::{manager_v2::*, Validate};
pub struct AcceptedContext {
    pub receipt: AcceptedInput,
    pub hints: InputHints,
}
struct Pending {
    input_id: Id,
    kind: InputKind,
    bytes: Option<usize>,
    hints: InputHints,
    unknown: bool,
}
#[derive(Default)]
pub(super) struct InputBook {
    pending: Vec<Pending>,
}
impl InputBook {
    pub fn admit(&mut self, id: Id, input: &Input, hints: &InputHints) -> Result<(), CursorError> {
        input.validate().map_err(|_| CursorError::InvalidMessage)?;
        hints.validate().map_err(|_| CursorError::InvalidMessage)?;
        if self.pending.iter().any(|v| v.input_id == id) {
            return Err(CursorError::DuplicateInput);
        }
        if self.pending.len() >= 16 {
            return Err(CursorError::Busy);
        }
        let bytes = match input {
            Input::Bytes { data, .. } => Some(
                crate::protocol::decode_chunk(data)
                    .map_err(|_| CursorError::InvalidMessage)?
                    .len(),
            ),
            _ => None,
        };
        self.pending.push(Pending {
            input_id: id,
            kind: input.kind(),
            bytes,
            hints: hints.clone(),
            unknown: false,
        });
        Ok(())
    }
    pub fn accepted(&mut self, receipt: AcceptedInput) -> Result<AcceptedContext, CursorError> {
        receipt
            .validate()
            .map_err(|_| CursorError::InvalidReceipt)?;
        let index = self
            .pending
            .iter()
            .position(|v| v.input_id == receipt.input_id)
            .ok_or(CursorError::UnknownInput)?;
        let record = &self.pending[index];
        if receipt.kind != record.kind
            || record.bytes.is_some_and(|len| {
                receipt.accepted_bytes as usize != len
                    || receipt.disposition != Disposition::Accepted
            })
        {
            return Err(CursorError::InvalidReceipt);
        }
        let record = self.pending.remove(index);
        Ok(AcceptedContext {
            receipt,
            hints: record.hints,
        })
    }
    pub fn unknown(&mut self, id: Id) -> Result<(), CursorError> {
        let record = self
            .pending
            .iter_mut()
            .find(|v| v.input_id == id)
            .ok_or(CursorError::UnknownInput)?;
        record.unknown = true;
        Ok(())
    }
    pub fn cancel_before_send(&mut self, id: Id) -> Result<(), CursorError> {
        let i = self
            .pending
            .iter()
            .position(|v| v.input_id == id)
            .ok_or(CursorError::UnknownInput)?;
        if self.pending[i].unknown {
            return Err(CursorError::OutcomeUnknown);
        }
        self.pending.remove(i);
        Ok(())
    }
    pub fn len(&self) -> usize {
        self.pending.len()
    }
}
