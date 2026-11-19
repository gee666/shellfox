use crate::{owner::Owner, *};
impl Owner {
    pub(crate) fn recover(
        &mut self,
        lease: Lease,
        capability: AnsiRecoveryCapability,
    ) -> Result<Response, RuntimeError> {
        let bytes = self.recovery_bytes(capability)?;
        let last_sequence = self
            .delivery
            .recover(lease)
            .map_err(RuntimeError::control)?;
        if capability == AnsiRecoveryCapability::RemoteV1 {
            let geometry = self.terminal.recovery_info().map_err(crate::commands::engine)?;
            return Ok(Response::RemoteV1Ansi { bytes, last_sequence, cols: geometry.cols, rows: geometry.rows });
        }
        Ok(Response::Ansi { capability, bytes, last_sequence })
    }
    pub(crate) fn recovery_bytes(
        &mut self,
        capability: AnsiRecoveryCapability,
    ) -> Result<Vec<u8>, RuntimeError> {
        match capability {
            AnsiRecoveryCapability::VisibleScreen => {
                ansi::visible_screen(&self.terminal.render().map_err(crate::commands::engine)?)
            }
            AnsiRecoveryCapability::RemoteV1 => crate::recovery::remote_v1_ansi(&mut self.terminal),
            AnsiRecoveryCapability::FormattedActiveBufferV1 => {
                crate::recovery::active_buffer_ansi(&mut self.terminal).map_err(|mut error| {
                    // Failure of an optional serializer must not stop a live PTY.
                    // Feed/resize/reply errors still take the fatal cleanup path.
                    if error.code == ErrorCode::Unavailable {
                        error.code = ErrorCode::Unsupported;
                    }
                    error
                })
            }
        }
    }

    pub(crate) fn legacy_controller_recovery(&self, lease: Lease) -> Result<(), RuntimeError> {
        if self.layouts.controller_opted(lease) {
            return Err(RuntimeError::validation(
                "A stamped controller epoch cannot downgrade to legacy recovery/replay.",
            ));
        }
        Ok(())
    }
    pub(crate) fn controller_side_effects(
        &self,
        lease: Lease,
        require_stamped: bool,
    ) -> Result<(), RuntimeError> {
        if require_stamped || self.layouts.controller_opted(lease) {
            self.layouts.controller_ready(lease)?;
            if self
                .delivery
                .first_sequence()
                .is_some_and(|first| self.delivery.last_sent().saturating_add(1) < first)
            {
                return Err(RuntimeError {
                    code: ErrorCode::RecoveryRequired,
                    message: "Controller delivery history expired; consume fresh recovery.".into(),
                });
            }
        }
        Ok(())
    }
    pub(crate) fn controller_stamped_recovery(
        &mut self,
        lease: Lease,
        capability: AnsiRecoveryCapability,
    ) -> Result<Response, RuntimeError> {
        let bytes = self.recovery_bytes(capability)?;
        let snapshot = self.layouts.controller_recovery(
            lease,
            bytes,
            capability,
            self.delivery.current_sequence(),
        )?;
        self.delivery
            .recover(lease)
            .map_err(RuntimeError::control)?;
        Ok(Response::Recovery(snapshot))
    }
    #[cfg(feature = "experimental-shellfox-native")]
    pub(crate) fn native0_snapshot(
        &mut self,
    ) -> Result<
        (
            shellfox_ghostty_vt::extension::NativeSnapshot,
            shellfox_ghostty_vt::extension::SnapshotLimits,
        ),
        RuntimeError,
    > {
        self.native_creation
            .as_ref()
            .ok_or_else(|| RuntimeError {
                code: ErrorCode::Unsupported,
                message: "This source was not explicitly created with Native0 recovery.".into(),
            })?
            .capture(&mut self.terminal)
    }
    #[cfg(feature = "experimental-shellfox-native")]
    pub(crate) fn controller_native0_stamped_recovery(
        &mut self,
        lease: Lease,
    ) -> Result<Response, RuntimeError> {
        let (snapshot, limits) = self.native0_snapshot()?;
        let cut = self
            .layouts
            .controller_native0_recovery(lease, self.delivery.current_sequence())?;
        self.delivery
            .recover(lease)
            .map_err(RuntimeError::control)?;
        Ok(Response::Native0Recovery(Box::new(
            StampedNative0Recovery {
                snapshot,
                limits,
                cut,
            },
        )))
    }
    pub(crate) fn controller_layout_ack(
        &mut self,
        lease: Lease,
        id: RecoveryId,
        sequence: u64,
        revision: u64,
    ) -> Result<Response, RuntimeError> {
        self.layouts
            .controller_validate_ack(lease, id, sequence, revision)?;
        self.delivery
            .acknowledge(lease, sequence)
            .map_err(RuntimeError::validation)?;
        self.layouts.controller_accepted(lease);
        Ok(Response::Applied)
    }
    pub(crate) fn release_control(&mut self, lease: Lease) -> Result<Response, RuntimeError> {
        self.delivery
            .release(lease)
            .map_err(RuntimeError::control)?;
        self.layouts.remove_controller();
        self.presentations.release_controller(lease);
        self.shared
            .screen
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take();
        self.outgoing.push_back(Event::ControllerRevoked {
            lease,
            reason: ControllerRevocation::Released,
        });
        Ok(Response::Applied)
    }
    pub(crate) fn stamped_mouse(
        &mut self,
        id: RequestId,
        lease: Lease,
        event: shellfox_ghostty_vt::MouseEvent,
        geometry: shellfox_ghostty_vt::MouseGeometry,
        stamp: ControllerGeometryStamp,
    ) -> Result<Response, RuntimeError> {
        self.active()?;
        self.controller_side_effects(lease, true)?;
        self.layouts.controller_geometry(lease, stamp)?;
        let bytes = self
            .terminal
            .encode_mouse(&event, geometry)
            .map_err(|error| RuntimeError::validation(error.to_string()))?;
        self.accept_input(
            id,
            &bytes,
            InputKind::Mouse {
                action: event.action,
                button: event.button,
                any_button_pressed: event.any_button_pressed,
            },
        )
    }
}
