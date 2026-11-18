use super::{AdapterError, HelperFrame, MAX_FRAME};

/// Streaming NDJSON framing with one bounded partial frame. The consumer must
/// apply bounded backpressure; the decoder never builds a list of all frames.
#[derive(Default, PartialEq, Eq)]
enum State {
    #[default]
    Open,
    Failed,
    Finished,
}
#[derive(Default)]
pub struct LineDecoder {
    pending: Vec<u8>,
    state: State,
}
impl LineDecoder {
    pub fn feed(
        &mut self,
        mut bytes: &[u8],
        mut emit: impl FnMut(HelperFrame) -> Result<(), AdapterError>,
    ) -> Result<(), AdapterError> {
        if self.state != State::Open {
            return Err(AdapterError::InvalidProtocol);
        }
        while !bytes.is_empty() {
            let newline = bytes.iter().position(|b| *b == b'\n');
            let end = newline.unwrap_or(bytes.len());
            if self
                .pending
                .len()
                .checked_add(end)
                .is_none_or(|len| len > MAX_FRAME)
            {
                self.state = State::Failed;
                return Err(AdapterError::InvalidProtocol);
            }
            self.pending.extend_from_slice(&bytes[..end]);
            if newline.is_none() {
                break;
            }
            let frame = std::mem::take(&mut self.pending);
            if let Err(error) = emit(HelperFrame::Line(frame)) {
                self.state = State::Failed;
                return Err(error);
            }
            bytes = &bytes[end + 1..];
        }
        Ok(())
    }
    pub fn finish(
        &mut self,
        emit: impl FnOnce(HelperFrame) -> Result<(), AdapterError>,
    ) -> Result<(), AdapterError> {
        if self.state != State::Open || !self.pending.is_empty() {
            self.state = State::Failed;
            return Err(AdapterError::InvalidProtocol);
        }
        self.state = State::Finished;
        emit(HelperFrame::Eof)
    }
}
