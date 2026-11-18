use super::ManagerCursor;
use shellfox_protocol::{manager_v2::*, Validate};
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UploadError {
    Invalid,
    ControlRequired,
    Busy,
    Complete,
    Uncertain,
    Cancelled,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Phase {
    Ready,
    Pending(usize),
    Finishing,
    Complete,
    Uncertain,
}
pub struct UploadCursor {
    begin: BeginTerminalUpload,
    id: Id,
    offset: u64,
    phase: Phase,
    cancelled: bool,
}
impl UploadCursor {
    pub fn new(
        begin: BeginTerminalUpload,
        started: TerminalUploadStarted,
        view: &ManagerCursor,
        binding: &Binding,
    ) -> Result<Self, UploadError> {
        begin.validate().map_err(|_| UploadError::Invalid)?;
        started.validate().map_err(|_| UploadError::Invalid)?;
        let value = Self {
            begin,
            id: started.upload_id,
            offset: 0,
            phase: Phase::Ready,
            cancelled: false,
        };
        value.current(view, binding)?;
        Ok(value)
    }
    fn current(&self, view: &ManagerCursor, binding: &Binding) -> Result<(), UploadError> {
        if view
            .control(binding)
            .map_err(|_| UploadError::ControlRequired)?
            == &self.begin.control
        {
            Ok(())
        } else {
            Err(UploadError::ControlRequired)
        }
    }
    fn ready(&self, view: &ManagerCursor, binding: &Binding) -> Result<(), UploadError> {
        self.current(view, binding)?;
        if self.cancelled {
            return Err(UploadError::Cancelled);
        }
        match self.phase {
            Phase::Ready => Ok(()),
            Phase::Complete => Err(UploadError::Complete),
            Phase::Uncertain => Err(UploadError::Uncertain),
            _ => Err(UploadError::Busy),
        }
    }
    pub fn write(
        &mut self,
        bytes: &[u8],
        view: &ManagerCursor,
        binding: &Binding,
    ) -> Result<WriteTerminalUpload, UploadError> {
        self.ready(view, binding)?;
        if bytes.is_empty()
            || bytes.len() > crate::protocol::FILE_CHUNK_BYTES
            || self.offset + bytes.len() as u64 > self.begin.size
        {
            return Err(UploadError::Invalid);
        }
        let request = WriteTerminalUpload {
            control: self.begin.control.clone(),
            upload_id: self.id,
            offset: self.offset,
            data: crate::protocol::encode_chunk(bytes).map_err(|_| UploadError::Invalid)?,
        };
        self.phase = Phase::Pending(bytes.len());
        Ok(request)
    }
    pub fn written(
        &mut self,
        response: &crate::protocol::TransferOffset,
        view: &ManagerCursor,
        binding: &Binding,
    ) -> Result<(), UploadError> {
        self.current(view, binding)?;
        let Phase::Pending(bytes) = self.phase else {
            return Err(UploadError::Busy);
        };
        if response.validate().is_err() || response.offset != self.offset + bytes as u64 {
            self.phase = Phase::Uncertain;
            return Err(UploadError::Invalid);
        }
        self.offset = response.offset;
        self.phase = Phase::Ready;
        Ok(())
    }
    pub fn finish(
        &mut self,
        view: &ManagerCursor,
        binding: &Binding,
    ) -> Result<FinishTerminalUpload, UploadError> {
        self.ready(view, binding)?;
        if self.offset != self.begin.size {
            return Err(UploadError::Invalid);
        }
        self.phase = Phase::Finishing;
        Ok(FinishTerminalUpload {
            control: self.begin.control.clone(),
            upload_id: self.id,
        })
    }
    /// A valid already-issued commit ACK wins a concurrent cleanup request, but never pastes.
    pub fn finished(
        &mut self,
        response: &crate::protocol::TerminalUploadResult,
    ) -> Result<(), UploadError> {
        if self.phase != Phase::Finishing {
            return Err(UploadError::Busy);
        }
        if response.validate().is_err() || response.size != self.begin.size {
            self.phase = Phase::Uncertain;
            return Err(UploadError::Invalid);
        }
        self.phase = Phase::Complete;
        Ok(())
    }
    pub fn failed(&mut self) {
        self.phase = Phase::Uncertain;
    }
    pub fn cancel_request(&mut self) -> CancelTerminalUpload {
        self.cancelled = true;
        CancelTerminalUpload {
            control: self.begin.control.clone(),
            upload_id: self.id,
        }
    }
    pub fn offset(&self) -> u64 {
        self.offset
    }
    pub fn is_complete(&self) -> bool {
        self.phase == Phase::Complete
    }
}
