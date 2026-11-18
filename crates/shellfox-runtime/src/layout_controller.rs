use super::*;
impl Layouts {
    pub(crate) fn remove_controller(&mut self) {
        self.views
            .retain(|view| !matches!(view.lease, PresentationLease::Controller(_)));
    }
    pub(crate) fn controller_opted(&self, lease: Lease) -> bool {
        self.views
            .iter()
            .any(|view| view.lease == PresentationLease::Controller(lease))
    }
    pub(crate) fn controller_recovery(
        &mut self,
        lease: Lease,
        bytes: Vec<u8>,
        capability: AnsiRecoveryCapability,
        sequence: u64,
    ) -> Result<StampedRecovery, RuntimeError> {
        self.recovery_for(
            PresentationLease::Controller(lease),
            bytes,
            capability,
            sequence,
        )
    }
    #[cfg(feature = "experimental-shellfox-native")]
    pub(crate) fn controller_native0_recovery(
        &mut self,
        lease: Lease,
        sequence: u64,
    ) -> Result<RecoveryCut, RuntimeError> {
        self.stamp_for(PresentationLease::Controller(lease), sequence)
    }
    pub(crate) fn controller_validate_ack(
        &self,
        lease: Lease,
        id: RecoveryId,
        sequence: u64,
        revision: u64,
    ) -> Result<(), RuntimeError> {
        self.validate_ack_for(PresentationLease::Controller(lease), id, sequence, revision)
    }
    pub(crate) fn controller_accepted(&mut self, lease: Lease) {
        self.accepted_for(PresentationLease::Controller(lease));
    }
    pub(crate) fn controller_data_ack_allowed(&self, lease: Lease) -> bool {
        !self.views.iter().any(|view| {
            view.lease == PresentationLease::Controller(lease) && view.recovery.is_some()
        })
    }
    pub(crate) fn controller_allows(
        &mut self,
        lease: Lease,
        next: u64,
        current_sequence: u64,
        notify: impl FnMut(LayoutNotice) -> bool,
    ) -> bool {
        self.allows_for(
            PresentationLease::Controller(lease),
            next,
            current_sequence,
            notify,
        )
    }
    pub(crate) fn controller_lost(&mut self, lease: Lease) {
        if let Some(view) = self
            .views
            .iter_mut()
            .find(|view| view.lease == PresentationLease::Controller(lease))
        {
            view.needs_recovery = true;
        }
    }
    pub(crate) fn controller_ready(&self, lease: Lease) -> Result<(), RuntimeError> {
        let ready = self
            .views
            .iter()
            .find(|view| view.lease == PresentationLease::Controller(lease))
            .is_some_and(|view| {
                view.recovery.is_none()
                    && !view.needs_recovery
                    && view
                        .accepted
                        .is_some_and(|stamp| stamp.revision == self.current.layout_revision)
            });
        if !ready {
            return Err(RuntimeError {
                code: ErrorCode::RecoveryRequired,
                message:
                    "Consume full stamped controller recovery before interactive side effects."
                        .into(),
            });
        }
        Ok(())
    }
    pub(crate) fn controller_geometry(
        &self,
        lease: Lease,
        stamp: ControllerGeometryStamp,
    ) -> Result<(), RuntimeError> {
        self.controller_ready(lease)?;
        let accepted = self
            .views
            .iter()
            .find(|view| view.lease == PresentationLease::Controller(lease))
            .and_then(|view| view.accepted);
        if !accepted.is_some_and(|accepted| accepted.id == stamp.recovery_id)
            || stamp.layout_revision != self.current.layout_revision
            || stamp.cols != self.current.cols
            || stamp.rows != self.current.rows
        {
            return Err(RuntimeError {
                code: ErrorCode::StaleLayout,
                message: "Mouse geometry is stale; consume fresh recovery before another gesture."
                    .into(),
            });
        }
        Ok(())
    }
}
