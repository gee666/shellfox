use crate::{owner::Owner, *};
impl Owner {
    pub(crate) fn stamped_recovery(
        &mut self,
        observer: ObserverLease,
        capability: AnsiRecoveryCapability,
    ) -> Result<Response, RuntimeError> {
        self.observers.check(observer)?;
        let bytes = self.recovery_bytes(capability)?;
        let sequence = self.delivery.current_sequence();
        let snapshot = self
            .layouts
            .recovery(observer, bytes, capability, sequence)?;
        self.observers.recover(observer, &self.delivery)?;
        Ok(Response::Recovery(snapshot))
    }
    pub(crate) fn recovery_ack(
        &mut self,
        observer: ObserverLease,
        recovery_id: RecoveryId,
        sequence: u64,
        revision: u64,
    ) -> Result<Response, RuntimeError> {
        self.observers.check(observer)?;
        self.layouts
            .validate_ack(observer, recovery_id, sequence, revision)?;
        self.observers.acknowledge(observer, sequence)?;
        self.layouts.accepted(observer);
        Ok(Response::Applied)
    }
    pub(crate) fn canonical_resize(&mut self, cols: u16, rows: u16) -> Result<(), RuntimeError> {
        self.layouts.validate_change(cols, rows)?;
        self.terminal
            .resize(cols, rows)
            .map_err(RuntimeError::unavailable)?;
        // Record actual canonical geometry immediately. A later PTY ioctl error
        // can retire input, but cannot make a late snapshot claim old dimensions.
        self.layouts
            .changed(cols, rows, self.delivery.current_sequence())?;
        self.presentations.source_changed()?;
        self.dirty = true;
        Ok(())
    }
}
