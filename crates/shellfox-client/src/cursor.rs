use shellfox_protocol::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EventDisposition {
    Deliver,
    Ignore,
    Reattach,
    Blocked,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CursorError {
    InvalidMessage,
    StaleConnection,
    NotOwned,
    NotConsumedInOrder,
}

/// Per-view delivery bookkeeping only. The host owns epochs, parser state and replay.
/// A new successful pairing invalidates this cursor, even for the same tab/generation.
#[derive(Clone, Debug)]
pub struct ViewCursor {
    connection_id: u64,
    tab_id: TabId,
    generation: Generation,
    control_epoch: u64,
    owned: bool,
    needs_recovery: bool,
    received: u64,
    consumed: u64,
    settled_at: Option<Option<String>>,
}
impl ViewCursor {
    pub fn from_attachment(
        attachment: &TerminalAttachment,
        connection_id: u64,
    ) -> Result<Self, CursorError> {
        attachment
            .validate()
            .map_err(|_| CursorError::InvalidMessage)?;
        match attachment {
            TerminalAttachment::Owned(a) => Ok(Self {
                connection_id,
                tab_id: a.tab_id,
                generation: a.generation,
                control_epoch: a.control_epoch,
                owned: true,
                needs_recovery: false,
                received: a.last_sequence,
                consumed: a.first_sequence - 1,
                settled_at: a.settled_at.clone(),
            }),
            TerminalAttachment::Blocked {
                tab_id,
                generation,
                control_epoch,
                ..
            } => Ok(Self {
                connection_id,
                tab_id: *tab_id,
                generation: *generation,
                control_epoch: *control_epoch,
                owned: false,
                needs_recovery: false,
                received: 0,
                consumed: 0,
                settled_at: None,
            }),
        }
    }
    pub fn observe(
        &mut self,
        event: &TerminalEvent,
        connection_id: u64,
    ) -> Result<EventDisposition, CursorError> {
        self.connection(connection_id)?;
        event.validate().map_err(|_| CursorError::InvalidMessage)?;
        let (tab_id, generation, epoch) = match event {
            TerminalEvent::Data {
                tab_id,
                generation,
                control_epoch,
                ..
            }
            | TerminalEvent::Exit {
                tab_id,
                generation,
                control_epoch,
                ..
            }
            | TerminalEvent::Error {
                tab_id,
                generation,
                control_epoch,
                ..
            }
            | TerminalEvent::Control {
                tab_id,
                generation,
                control_epoch,
                ..
            } => (*tab_id, *generation, *control_epoch),
            TerminalEvent::Activity { tab_id, .. } => {
                return Ok(if *tab_id == self.tab_id {
                    EventDisposition::Deliver
                } else {
                    EventDisposition::Ignore
                })
            }
            TerminalEvent::Settlement {
                tab_id,
                generation,
                settled_at,
            } => {
                if *tab_id != self.tab_id || *generation != self.generation {
                    return Ok(EventDisposition::Ignore);
                }
                self.settled_at = Some(settled_at.clone());
                return Ok(EventDisposition::Deliver);
            }
        };
        if tab_id != self.tab_id || generation != self.generation || epoch < self.control_epoch {
            return Ok(EventDisposition::Ignore);
        }
        if let TerminalEvent::Control { control, .. } = event {
            if epoch == self.control_epoch && (*control == Control::Owned) == self.owned {
                return Ok(EventDisposition::Ignore);
            }
            self.control_epoch = epoch;
            self.owned = false;
            self.needs_recovery = *control == Control::Owned;
            return Ok(if self.needs_recovery {
                EventDisposition::Reattach
            } else {
                EventDisposition::Blocked
            });
        }
        if epoch != self.control_epoch || !self.owned || self.needs_recovery {
            return Ok(EventDisposition::Ignore);
        }
        if let TerminalEvent::Data { sequence, .. } = event {
            if *sequence <= self.received {
                return Ok(EventDisposition::Ignore);
            }
            if *sequence != self.received + 1 {
                self.needs_recovery = true;
                return Ok(EventDisposition::Reattach);
            }
            self.received = *sequence;
        }
        if let TerminalEvent::Exit { last_sequence, .. } = event {
            if *last_sequence > self.received {
                self.needs_recovery = true;
                return Ok(EventDisposition::Reattach);
            }
        }
        Ok(EventDisposition::Deliver)
    }
    /// Call only after the terminal view/parser has consumed all bytes through sequence.
    /// Receiving or queuing an event is not consumption. This method never sends anything.
    pub fn consumed(
        &mut self,
        sequence: u64,
        connection_id: u64,
    ) -> Result<AcknowledgeTerminal, CursorError> {
        self.control(connection_id)?;
        if sequence < self.consumed || sequence > self.received {
            return Err(CursorError::NotConsumedInOrder);
        }
        self.consumed = sequence;
        Ok(AcknowledgeTerminal {
            tab_id: self.tab_id,
            generation: self.generation,
            control_epoch: self.control_epoch,
            sequence,
        })
    }
    pub fn recovery_request(&self, connection_id: u64) -> Result<AttachTerminal, CursorError> {
        self.connection(connection_id)?;
        Ok(AttachTerminal {
            tab_id: self.tab_id,
            generation: Some(self.generation),
            after_sequence: Some(self.consumed),
        })
    }
    pub fn write(
        &self,
        data: String,
        user_initiated: Option<bool>,
        connection_id: u64,
    ) -> Result<WriteTerminal, CursorError> {
        self.control(connection_id)?;
        let input = WriteTerminal {
            tab_id: self.tab_id,
            generation: self.generation,
            control_epoch: self.control_epoch,
            data,
            user_initiated,
            settled_at: self.settled_at.clone(),
        };
        input.validate().map_err(|_| CursorError::InvalidMessage)?;
        Ok(input)
    }
    pub fn resize(
        &self,
        cols: u16,
        rows: u16,
        connection_id: u64,
    ) -> Result<ResizeTerminal, CursorError> {
        self.control(connection_id)?;
        let input = ResizeTerminal {
            tab_id: self.tab_id,
            generation: self.generation,
            control_epoch: self.control_epoch,
            cols,
            rows,
        };
        input.validate().map_err(|_| CursorError::InvalidMessage)?;
        Ok(input)
    }
    pub fn begin_upload(
        &self,
        file_name: String,
        size: u64,
        connection_id: u64,
    ) -> Result<BeginTerminalUpload, CursorError> {
        self.control(connection_id)?;
        let request = BeginTerminalUpload {
            tab_id: self.tab_id,
            generation: self.generation,
            control_epoch: self.control_epoch,
            file_name,
            size,
        };
        request
            .validate_wire()
            .map_err(|_| CursorError::InvalidMessage)?;
        Ok(request)
    }
    /// Caller-supplied fields must match the native attached view before beginning an upload.
    pub fn validate_upload_begin(
        &self,
        request: &BeginTerminalUpload,
        connection_id: u64,
    ) -> Result<(), CursorError> {
        let expected = self.begin_upload(request.file_name.clone(), request.size, connection_id)?;
        if request.tab_id == expected.tab_id
            && request.generation == expected.generation
            && request.control_epoch == expected.control_epoch
        {
            Ok(())
        } else {
            Err(CursorError::NotOwned)
        }
    }
    fn connection(&self, connection_id: u64) -> Result<(), CursorError> {
        if self.connection_id == connection_id {
            Ok(())
        } else {
            Err(CursorError::StaleConnection)
        }
    }
    fn control(&self, connection_id: u64) -> Result<(), CursorError> {
        self.connection(connection_id)?;
        if self.owned && !self.needs_recovery {
            Ok(())
        } else {
            Err(CursorError::NotOwned)
        }
    }
}
