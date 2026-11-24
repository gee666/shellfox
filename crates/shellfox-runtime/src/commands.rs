use crate::{owner::Owner, *};

pub(crate) fn engine(error: shellfox_ghostty_vt::Error) -> RuntimeError {
    if error.0 == -2 {
        RuntimeError::validation(error.to_string())
    } else {
        RuntimeError::unavailable(error)
    }
}
impl Owner {
    pub(crate) fn command(
        &mut self,
        request_id: RequestId,
        command: Command,
    ) -> Result<Response, RuntimeError> {
        let lease = match &command {
            Command::Claim {
                controller,
                takeover,
            } => {
                if self.source_kind == SourceKind::ExternalBytes {
                    return Err(RuntimeError {
                        code: ErrorCode::Unsupported,
                        message: "External sources do not grant terminal control.".into(),
                    });
                }
                let previous = self.delivery.lease();
                let lease = self
                    .delivery
                    .claim(*controller, *takeover)
                    .map_err(RuntimeError::control)?;
                self.layouts.remove_controller();
                if let Some(previous) = previous {
                    self.outgoing.push_back(Event::ControllerRevoked {
                        lease: previous,
                        reason: ControllerRevocation::Replaced,
                    });
                }
                self.presentations.controller_changed(lease);
                self.shared
                    .screen
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .take();
                self.exit_sent = false;
                self.dirty = true;
                return Ok(Response::Claimed(lease));
            }
            Command::Presentation { .. } => {
                if let Command::Presentation { view, action } = command {
                    return self.presentation_command(view, action);
                }
                unreachable!("matched presentation command");
            }
            Command::RetryClose => {
                self.begin_close()?;
                return Ok(Response::Applied);
            }
            Command::Observe { .. }
            | Command::ObserverReplay { .. }
            | Command::ObserverAck { .. }
            | Command::ObserverRecover { .. }
            | Command::ObserverRecoverStamped { .. }
            | Command::ObserverLayoutAck { .. }
            | Command::Unobserve { .. } => return self.observer_command(command),
            Command::Key { lease, .. }
            | Command::Keys { lease, .. }
            | Command::Paste { lease, .. }
            | Command::RawInput { lease, .. }
            | Command::Mouse { lease, .. }
            | Command::Resize { lease, .. }
            | Command::Colors { lease, .. }
            | Command::Scroll { lease, .. }
            | Command::Selection { lease, .. }
            | Command::Copy { lease }
            | Command::Hyperlink { lease, .. }
            | Command::MouseTracking { lease }
            | Command::Screen { lease }
            | Command::Ack { lease, .. }
            | Command::Replay { lease, .. }
            | Command::RecoverAnsi { lease }
            | Command::Recover { lease, .. }
            | Command::RecoverStamped { lease, .. }
            | Command::ControllerLayoutAck { lease, .. }
            | Command::InteractiveBytes { lease, .. }
            | Command::MouseStamped { lease, .. }
            | Command::CheckControllerReady { lease }
            | Command::ReleaseControl { lease }
            | Command::Close { lease } => *lease,
        };
        self.delivery.check(lease).map_err(RuntimeError::control)?;
        match &command {
            Command::Key { .. }
            | Command::Keys { .. }
            | Command::Paste { .. }
            | Command::RawInput { .. }
            | Command::Mouse { .. }
            | Command::Resize { .. }
            | Command::Colors { .. } => self.controller_side_effects(lease, false)?,
            _ => {}
        }
        match command {
            Command::RecoverStamped { capability, .. } => {
                return self.controller_stamped_recovery(lease, capability)
            }
            Command::ControllerLayoutAck {
                recovery_id,
                last_delivery_sequence,
                layout_revision,
                ..
            } => {
                return self.controller_layout_ack(
                    lease,
                    recovery_id,
                    last_delivery_sequence,
                    layout_revision,
                )
            }
            Command::ReleaseControl { .. } => return self.release_control(lease),
            Command::CheckControllerReady { .. } => {
                self.active()?;
                self.controller_side_effects(lease, true)?;
            }
            Command::InteractiveBytes { bytes, .. } => {
                self.controller_side_effects(lease, true)?;
                let result = self.accept_input(request_id, &bytes, InputKind::Raw)?;
                if !bytes.is_empty() {
                    self.follow_accepted_input(lease);
                }
                return Ok(result);
            }
            Command::MouseStamped {
                event,
                geometry,
                stamp,
                ..
            } => return self.stamped_mouse(request_id, lease, event, geometry, stamp),
            Command::Key { event, .. } => return self.keys(request_id, lease, &[event]),
            Command::Keys { events, .. } => return self.keys(request_id, lease, &events),
            Command::Paste { text, .. } => {
                self.active()?;
                let bytes = self.terminal.encode_paste(&text).map_err(engine)?;
                let result = self.accept_input(
                    request_id,
                    &bytes,
                    InputKind::Paste {
                        nonempty: !text.is_empty(),
                    },
                )?;
                if !text.is_empty() && !bytes.is_empty() {
                    self.follow_accepted_input(lease);
                }
                return Ok(result);
            }
            Command::RawInput { bytes, .. } => {
                let result = self.accept_input(request_id, &bytes, InputKind::Raw)?;
                if !bytes.is_empty() {
                    self.follow_accepted_input(lease);
                }
                return Ok(result);
            }
            Command::Mouse {
                event, geometry, ..
            } => {
                self.active()?;
                let bytes = self
                    .terminal
                    .encode_mouse(&event, geometry)
                    .map_err(engine)?;
                return self.accept_input(
                    request_id,
                    &bytes,
                    InputKind::Mouse {
                        action: event.action,
                        button: event.button,
                        any_button_pressed: event.any_button_pressed,
                    },
                );
            }
            Command::Resize { cols, rows, .. } => {
                #[cfg(windows)]
                if self.phase == Phase::Starting {
                    if let Some(pty) = &mut self.elevated {
                        self.limits.validate_size(cols, rows)?;
                        pty.resize(cols, rows)?;
                        self.canonical_resize(cols, rows)?;
                        return Ok(Response::Applied);
                    }
                }
                self.active()?;
                self.limits.validate_size(cols, rows)?;
                // Drain the bounded reader queue at the old size before the resize.
                // New OS bytes may race the ioctl, but owner commands and every
                // already-queued output chunk have a single defined order.
                self.poll_output(shellfox_terminal::OUTPUT_QUEUE_CHUNKS);
                self.active()?;
                #[cfg(windows)]
                if let Some(pty) = &mut self.elevated {
                    pty.resize(cols, rows)?;
                    self.canonical_resize(cols, rows)?;
                    self.replies()?;
                    return Ok(Response::Applied);
                }
                self.canonical_resize(cols, rows)?;
                self.replies()?;
                self.pty
                    .as_ref()
                    .ok_or_else(|| RuntimeError::unavailable("missing PTY"))?
                    .resize(cols, rows)
                    .map_err(RuntimeError::unavailable)?;
            }
            Command::Colors { colors, .. } => {
                self.terminal.set_colors(&colors).map_err(engine)?;
                self.presentations.source_changed()?;
                self.dirty = true;
            }
            Command::Scroll { delta, .. } => {
                return self.presentation_command(
                    PresentationLease::Controller(lease),
                    PresentationAction::Scroll { delta },
                )
            }
            Command::Selection { selection, .. } => {
                return self.presentation_command(
                    PresentationLease::Controller(lease),
                    PresentationAction::Selection { selection },
                )
            }
            Command::Copy { .. } => {
                return self.presentation_command(
                    PresentationLease::Controller(lease),
                    PresentationAction::Copy,
                )
            }
            Command::Hyperlink { point, .. } => {
                return self.presentation_command(
                    PresentationLease::Controller(lease),
                    PresentationAction::Hyperlink { point },
                )
            }
            Command::MouseTracking { .. } => {
                return self
                    .terminal
                    .mouse_tracking()
                    .map(Response::MouseTracking)
                    .map_err(engine)
            }
            Command::Screen { .. } => {
                self.shared.legacy_requested.store(true, Ordering::Release);
                self.dirty = true;
            }
            Command::Ack { sequence, .. } => {
                if !self.layouts.controller_data_ack_allowed(lease) {
                    return Err(RuntimeError::validation(
                        "Stamped recovery requires its exact controller layout ACK.",
                    ));
                }
                self.delivery
                    .acknowledge(lease, sequence)
                    .map_err(RuntimeError::validation)?;
            }
            Command::Replay { after, .. } => {
                self.legacy_controller_recovery(lease)?;
                return self
                    .delivery
                    .replay(lease, after)
                    .map(Response::Replay)
                    .map_err(|message| RuntimeError {
                        code: ErrorCode::ReplayLost,
                        message,
                    });
            }
            Command::RecoverAnsi { .. } => {
                self.legacy_controller_recovery(lease)?;
                return self.recover(lease, AnsiRecoveryCapability::VisibleScreen);
            }
            Command::Recover { capability, .. } => {
                self.legacy_controller_recovery(lease)?;
                return self.recover(lease, capability);
            }
            Command::Close { .. } => self.begin_close()?,
            Command::Claim { .. }
            | Command::Presentation { .. }
            | Command::RetryClose
            | Command::Observe { .. }
            | Command::ObserverReplay { .. }
            | Command::ObserverAck { .. }
            | Command::ObserverRecover { .. }
            | Command::ObserverRecoverStamped { .. }
            | Command::ObserverLayoutAck { .. }
            | Command::Unobserve { .. } => {
                unreachable!("handled before lease validation")
            }
        }
        Ok(Response::Applied)
    }
    fn begin_close(&mut self) -> Result<(), RuntimeError> {
        if self.source_kind == SourceKind::ExternalBytes {
            let ended = self.external.as_ref().is_some_and(|source| {
                let status = source
                    .status
                    .lock()
                    .unwrap_or_else(|error| error.into_inner());
                status.finished || status.failed
            });
            if !ended {
                return Err(RuntimeError { code: ErrorCode::Unsupported,
                message: "Detach the view or finish its ordered source; external runtime close is not raw-process stop.".into() });
            }
            self.shared.abandoned.store(true, Ordering::Release);
            return Ok(());
        }
        if self.phase == Phase::Closed {
            return Ok(());
        }
        self.set_phase(Phase::Draining, None);
        #[cfg(windows)]
        if let Some(pty) = &mut self.elevated {
            return pty.begin_close();
        }
        if let Some(pty) = self.pty.as_mut() {
            // Stop owned processes without cancelling queued/final output.
            pty.begin_shutdown().map_err(RuntimeError::unavailable)?;
        } else {
            self.cleanup()?;
        }
        Ok(())
    }
}
