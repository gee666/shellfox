use crate::{CursorError, ViewCursor};
use shellfox_protocol::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UploadError {
    Cursor(CursorError),
    Invalid,
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
/// One upload under the exact view/generation/epoch which began it.
/// This object never sends input, cancels a shell or retries a mutation.
pub struct UploadCursor {
    begin: BeginTerminalUpload,
    upload_id: UploadId,
    offset: u64,
    phase: Phase,
    cancelled: bool,
}
impl UploadCursor {
    pub fn new(
        begin: BeginTerminalUpload,
        started: TerminalUploadStarted,
        view: &ViewCursor,
        connection_id: u64,
    ) -> Result<Self, UploadError> {
        begin.validate_wire().map_err(|_| UploadError::Invalid)?;
        started.validate().map_err(|_| UploadError::Invalid)?;
        let cursor = Self {
            begin,
            upload_id: started.upload_id,
            offset: 0,
            phase: Phase::Ready,
            cancelled: false,
        };
        cursor.current(view, connection_id)?;
        Ok(cursor)
    }
    fn current(&self, view: &ViewCursor, connection_id: u64) -> Result<(), UploadError> {
        let current = view
            .begin_upload(self.begin.file_name.clone(), self.begin.size, connection_id)
            .map_err(UploadError::Cursor)?;
        if current.tab_id != self.begin.tab_id
            || current.generation != self.begin.generation
            || current.control_epoch != self.begin.control_epoch
        {
            return Err(UploadError::Cursor(CursorError::NotOwned));
        }
        Ok(())
    }
    fn ready(&self, view: &ViewCursor, connection_id: u64) -> Result<(), UploadError> {
        self.current(view, connection_id)?;
        if self.cancelled {
            return Err(UploadError::Cancelled);
        }
        match self.phase {
            Phase::Ready => Ok(()),
            Phase::Uncertain => Err(UploadError::Uncertain),
            Phase::Complete => Err(UploadError::Complete),
            _ => Err(UploadError::Busy),
        }
    }
    pub fn write(
        &mut self,
        bytes: &[u8],
        view: &ViewCursor,
        connection_id: u64,
    ) -> Result<WriteTerminalUpload, UploadError> {
        let data = encode_chunk(bytes).map_err(|_| UploadError::Invalid)?;
        let request = WriteTerminalUpload {
            tab_id: self.begin.tab_id,
            generation: self.begin.generation,
            control_epoch: self.begin.control_epoch,
            upload_id: self.upload_id,
            offset: self.offset,
            data,
        };
        self.accept_write(&request, view, connection_id)?;
        Ok(request)
    }
    /// Validate a presentation command against this native-created lease before sending it.
    pub fn accept_write(
        &mut self,
        request: &WriteTerminalUpload,
        view: &ViewCursor,
        connection_id: u64,
    ) -> Result<(), UploadError> {
        self.ready(view, connection_id)?;
        request.validate().map_err(|_| UploadError::Invalid)?;
        self.identity(
            request.tab_id,
            request.generation,
            request.control_epoch,
            request.upload_id,
        )?;
        let bytes = chunk_size(&request.data, false).map_err(|_| UploadError::Invalid)?;
        if request.offset != self.offset || self.offset + bytes as u64 > self.begin.size {
            return Err(UploadError::Invalid);
        }
        self.phase = Phase::Pending(bytes);
        Ok(())
    }
    fn identity(
        &self,
        tab_id: TabId,
        generation: Generation,
        epoch: u64,
        upload_id: UploadId,
    ) -> Result<(), UploadError> {
        if tab_id == self.begin.tab_id
            && generation == self.begin.generation
            && epoch == self.begin.control_epoch
            && upload_id == self.upload_id
        {
            Ok(())
        } else {
            Err(UploadError::Invalid)
        }
    }
    pub fn written(
        &mut self,
        response: &TransferOffset,
        view: &ViewCursor,
        connection_id: u64,
    ) -> Result<u64, UploadError> {
        self.current(view, connection_id)?;
        let Phase::Pending(bytes) = self.phase else {
            return Err(UploadError::Busy);
        };
        if response.validate().is_err() || response.offset != self.offset + bytes as u64 {
            self.phase = Phase::Uncertain;
            return Err(UploadError::Invalid);
        }
        self.offset = response.offset;
        self.phase = Phase::Ready;
        Ok(self.offset)
    }
    pub fn finish(
        &mut self,
        view: &ViewCursor,
        connection_id: u64,
    ) -> Result<FinishTerminalUpload, UploadError> {
        let request = FinishTerminalUpload {
            tab_id: self.begin.tab_id,
            generation: self.begin.generation,
            control_epoch: self.begin.control_epoch,
            upload_id: self.upload_id,
        };
        self.accept_finish(&request, view, connection_id)?;
        Ok(request)
    }
    pub fn accept_finish(
        &mut self,
        request: &FinishTerminalUpload,
        view: &ViewCursor,
        connection_id: u64,
    ) -> Result<(), UploadError> {
        self.ready(view, connection_id)?;
        request.validate().map_err(|_| UploadError::Invalid)?;
        self.identity(
            request.tab_id,
            request.generation,
            request.control_epoch,
            request.upload_id,
        )?;
        if self.offset != self.begin.size {
            return Err(UploadError::Invalid);
        }
        self.phase = Phase::Finishing;
        Ok(())
    }
    pub fn accept_cancel(&mut self, request: &CancelTerminalUpload) -> Result<(), UploadError> {
        request.validate().map_err(|_| UploadError::Invalid)?;
        self.identity(
            request.tab_id,
            request.generation,
            request.control_epoch,
            request.upload_id,
        )?;
        self.cancelled = true;
        Ok(())
    }
    /// A positive commit ACK wins over cancellation racing an already-issued finish.
    pub fn finished(&mut self, response: &TerminalUploadResult) -> Result<(), UploadError> {
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
    pub fn offset(&self) -> u64 {
        self.offset
    }
    pub fn is_complete(&self) -> bool {
        self.phase == Phase::Complete
    }
    /// Cleanup remains possible after takeover. It retains the original fenced identity.
    pub fn cancel_request(&mut self) -> CancelTerminalUpload {
        self.cancelled = true;
        CancelTerminalUpload {
            tab_id: self.begin.tab_id,
            generation: self.begin.generation,
            control_epoch: self.begin.control_epoch,
            upload_id: self.upload_id,
        }
    }
}
