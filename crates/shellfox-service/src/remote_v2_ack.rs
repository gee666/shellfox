use super::*;
impl ObserverDelivery {
    /// A recovery ACK maps its wire UUID to the exact opaque Runtime stamp.
    /// This never accepts a standalone layout grant or window resize command.
    pub fn ack_command(
        &self,
        delivery: &wire::DeliveryRef,
        ack: &wire::Ack,
    ) -> Result<shellfox_runtime::Command, DeliveryFailure> {
        self.check_ack(delivery, ack)?;
        match self.recovery {
            Some(stamp) => Ok(shellfox_runtime::Command::ObserverLayoutAck {
                observer: self.observer,
                recovery_id: stamp.runtime_id,
                last_delivery_sequence: stamp.sequence,
                layout_revision: stamp.revision,
            }),
            None => Ok(shellfox_runtime::Command::ObserverAck {
                observer: self.observer,
                sequence: ack.sequence(),
            }),
        }
    }
    pub fn commit_ack(
        &mut self,
        delivery: &wire::DeliveryRef,
        ack: &wire::Ack,
        response: &Response,
    ) -> Result<(), DeliveryFailure> {
        self.check_ack(delivery, ack)?;
        if !matches!(response, Response::Applied) {
            return Err(DeliveryFailure::Invalid);
        }
        self.acknowledged = ack.sequence();
        self.recovery = None;
        while self
            .pending
            .front()
            .is_some_and(|chunk| chunk.sequence <= self.acknowledged)
        {
            if let Some(chunk) = self.pending.pop_front() {
                self.pending_bytes -= chunk.bytes.len();
            }
        }
        Ok(())
    }
    fn check_ack(
        &self,
        delivery: &wire::DeliveryRef,
        ack: &wire::Ack,
    ) -> Result<(), DeliveryFailure> {
        if delivery != &self.delivery {
            return Err(DeliveryFailure::Stale);
        }
        ack.validate().map_err(|_| DeliveryFailure::Invalid)?;
        if self.lost {
            return Err(DeliveryFailure::RecoveryRequired);
        }
        match (self.recovery, ack) {
            (
                Some(stamp),
                wire::Ack::Recovery {
                    recovery_id,
                    sequence,
                },
            ) if stamp.wire_id == *recovery_id && stamp.sequence == *sequence => Ok(()),
            (None, wire::Ack::Data { sequence })
                if *sequence >= self.acknowledged && *sequence <= self.offered =>
            {
                Ok(())
            }
            _ => Err(DeliveryFailure::Invalid),
        }
    }
}
