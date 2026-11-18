//! Pure bounded delivery bookkeeping shared by profiles, never input authority.
use shellfox_protocol::observer_v2::{DeliveryRef, Id};
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CursorError {
    InvalidMessage,
    Unavailable,
    StaleConnection,
    StaleDelivery,
    Revoked,
    RecoveryNotConsumed,
    RecoveryRequired,
    InvalidSequence,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EventDisposition {
    Deliver,
    Ignore,
    Reattach,
    Revoked,
}
#[derive(Clone, Copy)]
pub(crate) struct Stamp {
    pub id: Id,
    pub sequence: u64,
    pub revision: u64,
    pub cols: u16,
    pub rows: u16,
}
pub(crate) struct Tracker<B> {
    pub binding: B,
    pub delivery: DeliveryRef,
    pub stamp: Stamp,
    pub consumed: Option<u64>,
    received: u64,
    recovery_consumed: bool,
    accepted: bool,
    needs_recovery: bool,
    active: bool,
    ended: Option<u64>,
}
impl<B: PartialEq> Tracker<B> {
    pub fn new(binding: B, delivery: DeliveryRef, stamp: Stamp) -> Self {
        Self {
            binding,
            delivery,
            stamp,
            consumed: None,
            received: stamp.sequence,
            recovery_consumed: false,
            accepted: false,
            needs_recovery: false,
            active: true,
            ended: None,
        }
    }
    pub fn binding(&self, current: &B) -> Result<(), CursorError> {
        if self.binding == *current {
            Ok(())
        } else {
            Err(CursorError::StaleConnection)
        }
    }
    pub fn check(&self, current: &B) -> Result<(), CursorError> {
        self.binding(current)?;
        if !self.active {
            Err(CursorError::Revoked)
        } else {
            Ok(())
        }
    }
    pub fn reference(&self, reference: &DeliveryRef) -> Result<(), CursorError> {
        if self.delivery == *reference {
            Ok(())
        } else {
            Err(CursorError::StaleDelivery)
        }
    }
    fn ready_data(&self) -> Result<(), CursorError> {
        if self.needs_recovery {
            return Err(CursorError::RecoveryRequired);
        }
        if !self.recovery_consumed {
            return Err(CursorError::RecoveryNotConsumed);
        }
        Ok(())
    }
    pub fn ready_input(&self, current: &B) -> Result<(), CursorError> {
        self.check(current)?;
        self.ready_data()?;
        if self.accepted {
            Ok(())
        } else {
            Err(CursorError::RecoveryNotConsumed)
        }
    }
    pub fn consume_recovery(&mut self, id: Id) -> Result<(), CursorError> {
        if self.needs_recovery {
            return Err(CursorError::RecoveryRequired);
        }
        if id != self.stamp.id {
            return Err(CursorError::InvalidMessage);
        }
        if !self.recovery_consumed {
            self.recovery_consumed = true;
            self.consumed = Some(self.stamp.sequence);
        }
        Ok(())
    }
    pub fn validate_recovery_ack(
        &self,
        id: Id,
        seq: u64,
        revision: u64,
    ) -> Result<(), CursorError> {
        self.ready_data()?;
        if id == self.stamp.id && seq == self.stamp.sequence && revision == self.stamp.revision {
            Ok(())
        } else {
            Err(CursorError::InvalidSequence)
        }
    }
    pub fn accept_recovery(&mut self, id: Id, seq: u64, revision: u64) -> Result<(), CursorError> {
        self.validate_recovery_ack(id, seq, revision)?;
        self.accepted = true;
        Ok(())
    }
    pub fn consume_data(&mut self, seq: u64) -> Result<(), CursorError> {
        self.ready_data()?;
        if seq <= self.stamp.sequence
            || seq > self.received
            || self.consumed.is_some_and(|last| seq < last)
        {
            return Err(CursorError::InvalidSequence);
        }
        self.consumed = Some(seq);
        Ok(())
    }
    pub fn validate_data_ack(&self, seq: u64) -> Result<(), CursorError> {
        self.ready_data()?;
        if seq > self.stamp.sequence && self.consumed.is_some_and(|last| seq <= last) {
            Ok(())
        } else {
            Err(CursorError::InvalidSequence)
        }
    }
    pub fn data(&mut self, seq: u64) -> Result<EventDisposition, CursorError> {
        if self.needs_recovery {
            return Ok(EventDisposition::Reattach);
        }
        self.ready_data()?;
        if seq <= self.received {
            return Ok(EventDisposition::Ignore);
        }
        if self.ended.is_some_and(|last| seq > last) {
            return Err(CursorError::InvalidSequence);
        }
        if seq != self.received + 1 {
            self.needs_recovery = true;
            self.accepted = false;
            return Ok(EventDisposition::Reattach);
        }
        self.received = seq;
        Ok(EventDisposition::Deliver)
    }
    pub fn layout(
        &mut self,
        revision: u64,
        cols: u16,
        rows: u16,
        boundary: Option<u64>,
    ) -> Result<EventDisposition, CursorError> {
        if boundary.is_some_and(|seq| seq < self.received) {
            return Err(CursorError::InvalidSequence);
        }
        if revision < self.stamp.revision {
            return Ok(EventDisposition::Ignore);
        }
        if revision == self.stamp.revision && boundary.is_none_or(|seq| seq == self.received) {
            return if cols == self.stamp.cols && rows == self.stamp.rows {
                Ok(EventDisposition::Ignore)
            } else {
                Err(CursorError::InvalidMessage)
            };
        }
        self.needs_recovery = true;
        self.accepted = false;
        Ok(EventDisposition::Reattach)
    }
    pub fn source_end(&mut self, seq: u64) -> Result<EventDisposition, CursorError> {
        if self.needs_recovery || seq > self.received {
            self.needs_recovery = true;
            self.accepted = false;
            return Ok(EventDisposition::Reattach);
        }
        if seq < self.received {
            return Err(CursorError::InvalidSequence);
        }
        if self.ended == Some(seq) {
            return Ok(EventDisposition::Ignore);
        }
        self.ended = Some(seq);
        Ok(EventDisposition::Deliver)
    }
    pub fn process_exit(&mut self, seq: u64) -> EventDisposition {
        if self.needs_recovery || seq > self.received {
            self.needs_recovery = true;
            self.accepted = false;
            EventDisposition::Reattach
        } else {
            EventDisposition::Deliver
        }
    }
    pub fn require_recovery(&mut self) {
        self.needs_recovery = true;
        self.accepted = false;
    }
    pub fn revoke(&mut self) -> EventDisposition {
        self.active = false;
        self.accepted = false;
        EventDisposition::Revoked
    }
}
