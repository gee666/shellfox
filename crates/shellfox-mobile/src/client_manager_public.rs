//! Safe Rust product surface over the SAME native view/queue, not another client.
use super::*;
#[path = "client_manager_presentation.rs"]
mod presentation;
/// Cloneable UI renderer handle. All native methods still require its original
/// serial owner thread. Cloning does not create a parser or grant host control.
#[derive(Clone)]
pub struct ManagerOwnerView {
    terminal: u64,
    state: Arc<Mutex<ViewState>>,
}
impl ManagerOwnerView {
    pub fn terminal_handle(&self) -> u64 {
        self.terminal
    }
    pub fn pump(&self) -> Result<(), ManagerRecoveryError> {
        crate::engine::with_engine(self.terminal, crate::engine::pump)?;
        if let Some(error) = self.state.lock().map_err(|_| crate::Status::Panic)?.failure {
            return Err(error.into());
        }
        Ok(())
    }
    /// UI owner admission only. Geometry is frozen from this view's acknowledged
    /// recovery, never from a caller-supplied stamp or a later queued request.
    pub fn admit_mouse(
        &self,
        event: m::MouseEvent,
        cell_width_px: u32,
        cell_height_px: u32,
        hints: m::InputHints,
    ) -> Result<PendingManagerInput, ManagerRecoveryError> {
        use shellfox_protocol::Validate;
        crate::engine::with_engine(self.terminal, |_| Ok(()))?;
        event.validate().map_err(|_| crate::Status::Invalid)?;
        hints.validate().map_err(|_| crate::Status::Invalid)?;
        let origin = self
            .state
            .lock()
            .map_err(|_| crate::Status::Panic)?
            .origin
            .clone()
            .ok_or(crate::Status::Stale)?;
        let shared = crate::engine::lookup(self.terminal)?;
        let request = origin.witness.commit_if_current(|| {
            let mut state = self.state.lock().map_err(|_| crate::Status::Panic)?;
            origin.check(&state)?;
            let queue = shared.queue.lock().map_err(|_| crate::Status::Panic)?;
            if queue.closed {
                return Err(crate::Status::Stale.into());
            }
            if queue.failed {
                return Err(crate::Status::Engine.into());
            }
            state
                .cursor
                .as_mut()
                .ok_or(crate::Status::Stale)?
                .admit_mouse(
                    m::Id::new(),
                    event,
                    cell_width_px,
                    cell_height_px,
                    hints,
                    origin.witness.binding(),
                )
                .map_err(ManagerRecoveryError::Cursor)
        })??;
        Ok(PendingManagerInput {
            request,
            origin,
            state: self.state.clone(),
        })
    }
    pub fn frame(&self) -> Result<shellfox_ghostty_vt::Screen, ManagerRecoveryError> {
        Ok(crate::engine::with_engine(self.terminal, |owned| {
            owned.terminal.render().map_err(|_| crate::Status::Engine)
        })?)
    }
    pub fn close(&self) -> Result<(), ManagerRecoveryError> {
        {
            let mut state = self.state.lock().map_err(|_| crate::Status::Panic)?;
            state.retired = true;
            state.generation = state.generation.saturating_add(1);
            state.origin = None;
            state.pending_ack = None;
            state.installed = None;
            state.data.clear();
        }
        Ok(crate::engine::close(self.terminal)?)
    }
}
/// Frozen original-context input, issued only by admit_input. It is not serde or
/// caller-constructible. Queue this exact object; never re-admit uncertain input.
pub struct PendingManagerInput {
    request: m::WriteTerminal,
    origin: RecoveryOrigin,
    state: Arc<Mutex<ViewState>>,
}
pub struct PendingManagerResize {
    request: m::ResizeTerminal,
    origin: RecoveryOrigin,
    state: Arc<Mutex<ViewState>>,
}
impl PendingManagerInput {
    pub fn input_id(&self) -> m::Id {
        self.request.input_id
    }
    pub fn cancel_before_send(self) -> Result<(), ManagerRecoveryError> {
        self.origin.witness.commit_if_current(|| {
            let mut state = self.state.lock().map_err(|_| crate::Status::Panic)?;
            self.origin.check(&state)?;
            state
                .cursor
                .as_mut()
                .ok_or(crate::Status::Stale)?
                .cancel_before_send(self.request.input_id)
                .map_err(ManagerRecoveryError::Cursor)
        })??;
        Ok(())
    }
}
impl<T: OutboundTransport> ManagerRecovery<T> {
    pub fn native_client(
        peer: AuthenticatedPeerId,
        id: m::Id,
        transport: T,
    ) -> Result<Arc<ManagerClient<T>>, ManagerRecoveryError> {
        let verified = VerifiedNativeExtension::verify().map_err(|_| crate::Status::Engine)?;
        let text = String::from_utf8(verified.identity().as_bytes().to_vec())
            .map_err(|_| crate::Status::Invalid)?;
        let codec = m::Native0Codec::new(
            m::EngineIdentity::try_from(text).map_err(|_| crate::Status::Invalid)?,
        );
        Ok(Arc::new(ManagerClient::native0(
            peer, id, transport, codec,
        )?))
    }
    pub fn from_shared_native0(
        client: Arc<ManagerClient<T>>,
        cols: u16,
        rows: u16,
        limits: SnapshotLimits,
    ) -> Result<Self, ManagerRecoveryError> {
        let receiver = VerifiedNativeExtension::verify().map_err(|_| crate::Status::Engine)?;
        let terminal = crate::engine::create(cols, rows)?;
        Self::bind_native0(client, terminal, receiver, limits)
    }
    pub fn owner_view(&self) -> ManagerOwnerView {
        ManagerOwnerView {
            terminal: self.terminal,
            state: self.state.clone(),
        }
    }
    pub fn admit_input(
        &self,
        input: m::Input,
        hints: m::InputHints,
    ) -> Result<PendingManagerInput, ManagerRecoveryError> {
        use shellfox_protocol::Validate;
        self.ensure_live_engine()?;
        if matches!(input, m::Input::Mouse { .. }) {
            return Err(crate::Status::Invalid.into());
        }
        input.validate().map_err(|_| crate::Status::Invalid)?;
        hints.validate().map_err(|_| crate::Status::Invalid)?;
        let origin = self.installed_origin()?;
        let request = origin.witness.commit_if_current(|| {
            let mut state = self.state.lock().map_err(|_| crate::Status::Panic)?;
            origin.check(&state)?;
            let cursor = state.cursor.as_mut().ok_or(crate::Status::Stale)?;
            let id = m::Id::new();
            cursor
                .admit_input(id, input, hints, origin.witness.binding())
                .map_err(ManagerRecoveryError::Cursor)
        })??;
        Ok(PendingManagerInput {
            request,
            origin,
            state: self.state.clone(),
        })
    }
    pub fn admit_mouse(
        &self,
        event: m::MouseEvent,
        cell_width_px: u32,
        cell_height_px: u32,
        hints: m::InputHints,
    ) -> Result<PendingManagerInput, ManagerRecoveryError> {
        self.owner_view()
            .admit_mouse(event, cell_width_px, cell_height_px, hints)
    }
    pub async fn execute_input(
        &self,
        pending: PendingManagerInput,
    ) -> Result<m::Reply<m::AcceptedInput>, ManagerRecoveryError> {
        self.ensure_live_engine()?;
        if !Arc::ptr_eq(&self.state, &pending.state) {
            return Err(crate::Status::Stale.into());
        }
        pending.origin.witness.commit_if_current(|| {
            let state = self.state.lock().map_err(|_| crate::Status::Panic)?;
            pending.origin.check(&state)
        })??;
        let result = self
            .client
            .execute_with_witness(&pending.origin.witness, &pending.request)
            .await;
        let _ = pending.origin.witness.commit_if_current(|| {
            if let Ok(mut state) = self.state.lock() {
                if pending.origin.check(&state).is_ok() {
                    if let Some(cursor) = state.cursor.as_mut() {
                        match &result {
                            Ok(m::Reply::Success(receipt)) => {
                                let _ = cursor
                                    .accepted(receipt.clone(), pending.origin.witness.binding());
                            }
                            _ => {
                                let _ = cursor.input_unknown(pending.request.input_id);
                            }
                        }
                    }
                }
            }
        });
        Ok(result?)
    }
    pub fn admit_resize(
        &self,
        cols: u16,
        rows: u16,
    ) -> Result<PendingManagerResize, ManagerRecoveryError> {
        self.ensure_live_engine()?;
        let origin = self.installed_origin()?;
        let request = origin.witness.commit_if_current(|| {
            let mut state = self.state.lock().map_err(|_| crate::Status::Panic)?;
            origin.check(&state)?;
            state
                .cursor
                .as_mut()
                .ok_or(crate::Status::Stale)?
                .resize(cols, rows, origin.witness.binding())
                .map_err(ManagerRecoveryError::Cursor)
        })??;
        Ok(PendingManagerResize {
            request,
            origin,
            state: self.state.clone(),
        })
    }
    pub async fn execute_resize(
        &self,
        pending: PendingManagerResize,
    ) -> Result<m::Reply<m::Resized>, ManagerRecoveryError> {
        self.ensure_live_engine()?;
        if !Arc::ptr_eq(&self.state, &pending.state) {
            return Err(crate::Status::Stale.into());
        }
        pending.origin.witness.commit_if_current(|| {
            let state = self.state.lock().map_err(|_| crate::Status::Panic)?;
            pending.origin.check(&state)
        })??;
        Ok(self
            .client
            .execute_with_witness(&pending.origin.witness, &pending.request)
            .await?)
    }
    pub async fn detach(
        &mut self,
    ) -> Result<m::Reply<shellfox_protocol::Detached>, ManagerRecoveryError> {
        let origin = self.installed_origin()?;
        let request = origin.witness.commit_if_current(|| {
            let mut state = self.state.lock().map_err(|_| crate::Status::Panic)?;
            origin.check(&state)?;
            state
                .cursor
                .as_mut()
                .ok_or(crate::Status::Stale)?
                .detach_request(true, origin.witness.binding())
                .map_err(ManagerRecoveryError::Cursor)
        })??;
        self.cancel_pending();
        Ok(self
            .client
            .execute_with_witness(&origin.witness, &request)
            .await?)
    }
}
