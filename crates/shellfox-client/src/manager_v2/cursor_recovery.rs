use super::*;
impl ManagerCursor {
    pub fn new(binding: Binding, attachment: &Attachment) -> Result<Self, CursorError> {
        if matches!(attachment,Attachment::Attached(a)if a.recovery.format()!=RecoveryFormat::FormattedActiveBuffer)
        {
            return Err(CursorError::InvalidMessage);
        }
        Self::build(binding, attachment)
    }
    pub fn from_selection(
        selection: PairSelection,
        attachment: &Attachment,
    ) -> Result<Self, CursorError> {
        selection
            .validate()
            .map_err(|_| CursorError::InvalidMessage)?;
        if let Attachment::Attached(a) = attachment {
            a.recovery
                .check_selection(&selection)
                .map_err(|_| CursorError::InvalidMessage)?;
        }
        Self::build(selection.binding(), attachment)
    }
    fn build(binding: Binding, attachment: &Attachment) -> Result<Self, CursorError> {
        attachment
            .validate()
            .map_err(|_| CursorError::InvalidMessage)?;
        let Attachment::Attached(a) = attachment else {
            return Err(DeliveryError::Unavailable.into());
        };
        Ok(Self {
            tracker: Tracker::new(
                binding,
                a.delivery.clone(),
                Stamp {
                    id: a.recovery.recovery_id(),
                    sequence: a.recovery.last_sequence(),
                    revision: a.recovery.layout_revision(),
                    cols: a.recovery.cols(),
                    rows: a.recovery.rows(),
                },
            ),
            recovery: a.recovery.clone(),
            input: a.input.clone(),
            layout: a.layout.clone(),
            input_state: a.input_state,
            inputs: InputBook::default(),
        })
    }
    pub fn binding(&self) -> &Binding {
        &self.tracker.binding
    }
    pub fn delivery(&self) -> &DeliveryRef {
        &self.tracker.delivery
    }
    pub fn recovery(&self) -> &Recovery {
        &self.recovery
    }
    pub fn recovery_bytes(&self) -> Result<Vec<u8>, CursorError> {
        self.recovery
            .bytes()
            .map_err(|_| CursorError::InvalidMessage)
    }
    pub fn consumed_recovery(
        &mut self,
        id: Id,
        current: &Binding,
    ) -> Result<AcknowledgeTerminal, CursorError> {
        self.tracker.check(current)?;
        self.tracker.consume_recovery(id)?;
        Ok(AcknowledgeTerminal {
            delivery: self.delivery().clone(),
            ack: Ack::Recovery {
                recovery_id: id,
                sequence: self.recovery.last_sequence(),
                layout_revision: self.recovery.layout_revision(),
            },
        })
    }
    pub fn consumed_data(
        &mut self,
        seq: u64,
        current: &Binding,
    ) -> Result<AcknowledgeTerminal, CursorError> {
        self.tracker.check(current)?;
        self.tracker.consume_data(seq)?;
        Ok(AcknowledgeTerminal {
            delivery: self.delivery().clone(),
            ack: Ack::Data { sequence: seq },
        })
    }
    pub fn validate_ack(
        &self,
        request: &AcknowledgeTerminal,
        current: &Binding,
    ) -> Result<(), CursorError> {
        self.tracker.check(current)?;
        request
            .validate()
            .map_err(|_| CursorError::InvalidMessage)?;
        self.tracker.reference(&request.delivery)?;
        match request.ack {
            Ack::Recovery {
                recovery_id,
                sequence,
                layout_revision,
            } => self
                .tracker
                .validate_recovery_ack(recovery_id, sequence, layout_revision)?,
            Ack::Data { sequence } => self.tracker.validate_data_ack(sequence)?,
        }
        Ok(())
    }
    pub fn acknowledged(
        &mut self,
        request: &AcknowledgeTerminal,
        response: &crate::protocol::Acknowledged,
        current: &Binding,
    ) -> Result<(), CursorError> {
        self.validate_ack(request, current)?;
        response
            .validate()
            .map_err(|_| CursorError::InvalidMessage)?;
        if let Ack::Recovery {
            recovery_id,
            sequence,
            layout_revision,
        } = request.ack
        {
            self.tracker
                .accept_recovery(recovery_id, sequence, layout_revision)?;
        }
        Ok(())
    }
}
