pub use crate::v2_common::delivery::{CursorError, EventDisposition};
use crate::v2_common::delivery::{Stamp, Tracker};
use shellfox_protocol::{observer_v2::*, Validate};
/// Observer bookkeeping never exposes control/layout authority or sends ACKs.
pub struct DeliveryCursor {
    tracker: Tracker<Binding>,
    recovery: Recovery,
}
impl DeliveryCursor {
    pub fn new(binding: Binding, attachment: &Attachment) -> Result<Self, CursorError> {
        attachment
            .validate()
            .map_err(|_| CursorError::InvalidMessage)?;
        let Attachment::Attached {
            delivery, recovery, ..
        } = attachment
        else {
            return Err(CursorError::Unavailable);
        };
        Ok(Self {
            tracker: Tracker::new(
                binding,
                delivery.clone(),
                Stamp {
                    id: recovery.recovery_id,
                    sequence: recovery.last_sequence,
                    revision: recovery.layout_revision,
                    cols: recovery.cols,
                    rows: recovery.rows,
                },
            ),
            recovery: recovery.clone(),
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
                sequence: self.recovery.last_sequence,
            },
        })
    }
    pub fn observe(
        &mut self,
        event: &Event,
        current: &Binding,
    ) -> Result<EventDisposition, CursorError> {
        self.tracker.check(current)?;
        event.validate().map_err(|_| CursorError::InvalidMessage)?;
        self.tracker.reference(event.delivery())?;
        match event {
            Event::Data { sequence, .. } => self.tracker.data(*sequence),
            Event::SourceEnded { last_sequence, .. } => self.tracker.source_end(*last_sequence),
            Event::ProcessExited { last_sequence, .. } => {
                Ok(self.tracker.process_exit(*last_sequence))
            }
            Event::Revoked { .. } => Ok(self.tracker.revoke()),
            Event::Layout {
                layout_revision,
                cols,
                rows,
                ..
            } => self.tracker.layout(*layout_revision, *cols, *rows, None),
            Event::Error { .. } => Ok(EventDisposition::Deliver),
        }
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
            } => self.tracker.validate_recovery_ack(
                recovery_id,
                sequence,
                self.recovery.layout_revision,
            ),
            Ack::Data { sequence } => self.tracker.validate_data_ack(sequence),
        }
    }
    pub fn recovery_request(&self, current: &Binding) -> Result<AttachTerminal, CursorError> {
        self.tracker.check(current)?;
        Ok(AttachTerminal {
            tab_id: self.delivery().tab_id,
            resume: self.tracker.consumed.map(|after_sequence| Resume {
                generation: self.delivery().generation,
                after_sequence,
            }),
        })
    }
    pub fn detach_request(&self, current: &Binding) -> Result<DetachTerminal, CursorError> {
        self.tracker.binding(current)?;
        Ok(DetachTerminal {
            delivery: self.delivery().clone(),
        })
    }
}
