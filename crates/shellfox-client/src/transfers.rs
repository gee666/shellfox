use shellfox_protocol::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransferError {
    Invalid,
    Busy,
    Complete,
    Uncertain,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Phase {
    Ready,
    Pending(usize),
    Finishing,
    Complete,
    Uncertain,
}

/// Streaming download bookkeeping. Advance only after the sink consumes a validated chunk.
/// No whole-file allocation and no client-local path or sink is opened here.
pub struct FileReadCursor {
    header: FileReadTransfer,
    offset: u64,
    requested: bool,
    pending: Option<(u64, bool)>,
    complete: bool,
    failed: bool,
}
impl FileReadCursor {
    pub fn new(header: FileReadTransfer) -> Result<Self, TransferError> {
        header.validate().map_err(|_| TransferError::Invalid)?;
        Ok(Self {
            header,
            offset: 0,
            requested: false,
            pending: None,
            complete: false,
            failed: false,
        })
    }
    pub fn request(&mut self) -> Result<ReadFileChunk, TransferError> {
        if self.failed {
            return Err(TransferError::Uncertain);
        }
        if self.complete {
            return Err(TransferError::Complete);
        }
        if self.requested || self.pending.is_some() {
            return Err(TransferError::Busy);
        }
        self.requested = true;
        Ok(ReadFileChunk {
            transfer_id: self.header.transfer_id,
            offset: self.offset,
        })
    }
    pub fn receive(&mut self, chunk: &FileChunk) -> Result<Vec<u8>, TransferError> {
        if self.failed {
            return Err(TransferError::Uncertain);
        }
        if !self.requested || self.pending.is_some() {
            return Err(TransferError::Busy);
        }
        let checked = || {
            chunk.validate().map_err(|_| TransferError::Invalid)?;
            let bytes = decode_chunk(&chunk.data).map_err(|_| TransferError::Invalid)?;
            if self.offset + bytes.len() as u64 != chunk.offset
                || chunk.offset > self.header.size
                || chunk.eof != (chunk.offset == self.header.size)
                || bytes.is_empty() && !chunk.eof
            {
                return Err(TransferError::Invalid);
            }
            Ok(bytes)
        };
        match checked() {
            Ok(bytes) => {
                self.pending = Some((chunk.offset, chunk.eof));
                Ok(bytes)
            }
            Err(error) => {
                self.failed = true;
                Err(error)
            }
        }
    }
    /// Caller has written the bytes to its chosen sink. This does not send a host ACK.
    pub fn consumed(&mut self) -> Result<u64, TransferError> {
        if self.failed {
            return Err(TransferError::Uncertain);
        }
        let (offset, eof) = self.pending.take().ok_or(TransferError::Busy)?;
        self.offset = offset;
        self.complete = eof;
        self.requested = false;
        Ok(offset)
    }
    pub fn failed(&mut self) {
        self.failed = true;
    }
    pub fn is_complete(&self) -> bool {
        self.complete
    }
    pub fn offset(&self) -> u64 {
        self.offset
    }
    pub fn header(&self) -> &FileReadTransfer {
        &self.header
    }
    pub fn cancel_request(&self) -> CancelFileTransfer {
        CancelFileTransfer {
            transfer_id: self.header.transfer_id,
        }
    }
}

/// A write request is issued once. An uncertain response must not be retried.
pub struct FileWriteCursor {
    input: BeginFileWrite,
    transfer_id: TransferId,
    offset: u64,
    phase: Phase,
}
impl FileWriteCursor {
    pub fn new(input: BeginFileWrite, started: FileWriteTransfer) -> Result<Self, TransferError> {
        input.validate_wire().map_err(|_| TransferError::Invalid)?;
        started.validate().map_err(|_| TransferError::Invalid)?;
        Ok(Self {
            input,
            transfer_id: started.transfer_id,
            offset: 0,
            phase: Phase::Ready,
        })
    }
    fn ready(&self) -> Result<(), TransferError> {
        match self.phase {
            Phase::Ready => Ok(()),
            Phase::Uncertain => Err(TransferError::Uncertain),
            Phase::Complete => Err(TransferError::Complete),
            _ => Err(TransferError::Busy),
        }
    }
    pub fn write(&mut self, bytes: &[u8]) -> Result<WriteFileChunk, TransferError> {
        self.ready()?;
        if bytes.is_empty()
            || bytes.len() > FILE_CHUNK_BYTES
            || self.offset + bytes.len() as u64 > self.input.size
        {
            return Err(TransferError::Invalid);
        }
        let data = encode_chunk(bytes).map_err(|_| TransferError::Invalid)?;
        self.phase = Phase::Pending(bytes.len());
        Ok(WriteFileChunk {
            transfer_id: self.transfer_id,
            offset: self.offset,
            data,
        })
    }
    pub fn written(&mut self, response: &TransferOffset) -> Result<u64, TransferError> {
        let Phase::Pending(bytes) = self.phase else {
            return Err(TransferError::Busy);
        };
        if response.validate().is_err() || response.offset != self.offset + bytes as u64 {
            self.phase = Phase::Uncertain;
            return Err(TransferError::Invalid);
        }
        self.offset = response.offset;
        self.phase = Phase::Ready;
        Ok(self.offset)
    }
    pub fn finish(&mut self) -> Result<FinishFileWrite, TransferError> {
        self.ready()?;
        if self.offset != self.input.size {
            return Err(TransferError::Invalid);
        }
        self.phase = Phase::Finishing;
        Ok(FinishFileWrite {
            transfer_id: self.transfer_id,
        })
    }
    pub fn finished(&mut self, response: &FileTransferResult) -> Result<(), TransferError> {
        if self.phase != Phase::Finishing {
            return Err(TransferError::Busy);
        }
        if response.validate().is_err() || response.size != self.input.size {
            self.phase = Phase::Uncertain;
            return Err(TransferError::Invalid);
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
    pub fn cancel_request(&self) -> CancelFileTransfer {
        CancelFileTransfer {
            transfer_id: self.transfer_id,
        }
    }
}
