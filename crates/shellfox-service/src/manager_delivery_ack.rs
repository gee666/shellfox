use super::*;
impl ManagerDelivery {
    fn validate_ack(&self, reference: &wire::DeliveryRef, ack: &wire::Ack) -> Result<(), Failure> {
        reference.validate().map_err(|_| Failure::Invalid)?;
        ack.validate().map_err(|_| Failure::Invalid)?;
        if reference != &self.reference || self.revoked() {
            return Err(Failure::Stale);
        }
        if self.lost {
            return Err(Failure::RecoveryRequired);
        }
        match (self.recovery, ack) {
            (
                Some(stamp),
                wire::Ack::Recovery {
                    recovery_id,
                    sequence,
                    layout_revision,
                },
            ) if stamp.wire_id == *recovery_id
                && stamp.sequence == *sequence
                && stamp.revision == *layout_revision =>
            {
                Ok(())
            }
            (None, wire::Ack::Data { sequence })
                if *sequence >= self.acked && *sequence <= self.offered =>
            {
                Ok(())
            }
            _ => Err(Failure::Invalid),
        }
    }
    pub fn ack_command(
        &self,
        reference: &wire::DeliveryRef,
        ack: &wire::Ack,
    ) -> Result<shellfox_runtime::Command, Failure> {
        self.validate_ack(reference, ack)?;
        use shellfox_runtime::Command;
        Ok(match (self.role, self.recovery) {
            (Role::Controller(lease), Some(stamp)) => Command::ControllerLayoutAck {
                lease,
                recovery_id: stamp.native_id,
                last_delivery_sequence: stamp.sequence,
                layout_revision: stamp.revision,
            },
            (Role::Observer(observer), Some(stamp)) => Command::ObserverLayoutAck {
                observer,
                recovery_id: stamp.native_id,
                last_delivery_sequence: stamp.sequence,
                layout_revision: stamp.revision,
            },
            (Role::Controller(lease), None) => Command::Ack {
                lease,
                sequence: ack.sequence(),
            },
            (Role::Observer(observer), None) => Command::ObserverAck {
                observer,
                sequence: ack.sequence(),
            },
        })
    }
    pub fn commit_ack(
        &mut self,
        reference: &wire::DeliveryRef,
        ack: &wire::Ack,
        response: &Response,
    ) -> Result<(), Failure> {
        self.validate_ack(reference, ack)?;
        if !matches!(response, Response::Applied) {
            return Err(Failure::Invalid);
        }
        self.acked = ack.sequence();
        if let Some(stamp) = self.recovery.take() {
            self.accepted = Some(stamp);
        }
        while self
            .pending
            .front()
            .is_some_and(|chunk| chunk.sequence <= self.acked)
        {
            if let Some(chunk) = self.pending.pop_front() {
                self.bytes -= chunk.bytes.len();
            }
        }
        Ok(())
    }
}
