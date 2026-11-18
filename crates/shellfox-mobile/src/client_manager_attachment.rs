//! Original request context capture and queue admission; no native import here.
use super::*;
impl<T: OutboundTransport> ManagerRecovery<T> {
    // Capture the actual PAIR origin and this SDK operation before any request.
    // A held response cannot acquire a newer binding or a newer SDK generation.
    pub(super) fn begin_recovery(&mut self) -> Result<RecoveryOrigin, ManagerRecoveryError> {
        let witness = self.client.binding_witness()?;
        self.begin_recovery_from(witness)
    }
    fn begin_recovery_from(
        &mut self,
        witness: ManagerBindingWitness,
    ) -> Result<RecoveryOrigin, ManagerRecoveryError> {
        self.cancel_pending();
        let cancel = self.cancel.child_token();
        witness
            .commit_if_current(|| {
                let state = self.state.lock().map_err(|_| crate::Status::Panic)?;
                if state.retired || cancel.is_cancelled() {
                    return Err(crate::Status::Stale);
                }
                Ok(RecoveryOrigin {
                    witness: witness.clone(),
                    generation: state.generation,
                    cancel,
                })
            })?
            .map_err(Into::into)
    }
    pub async fn attach(
        &mut self,
        input: m::AttachTerminal,
    ) -> Result<m::Reply<m::Attachment>, ManagerRecoveryError> {
        self.ensure_live_engine()?;
        let origin = self.begin_recovery()?;
        let tab = input.tab_id;
        let generation = input.resume.as_ref().map(|resume| resume.generation);
        self.recover_request(origin, input, tab, generation).await
    }
    async fn recover_request<R: m::Request<Response = m::Attachment>>(
        &mut self,
        origin: RecoveryOrigin,
        input: R,
        expected_tab: m::Id,
        expected_generation: Option<m::Id>,
    ) -> Result<m::Reply<m::Attachment>, ManagerRecoveryError> {
        let reply = self
            .client
            .execute_with_witness(&origin.witness, &input)
            .await?;
        if let m::Reply::Success(value) = &reply {
            match value {
                m::Attachment::Attached(attached) => {
                    if attached.delivery.tab_id != expected_tab
                        || expected_generation
                            .is_some_and(|generation| attached.delivery.generation != generation)
                    {
                        return Err(crate::Status::Invalid.into());
                    }
                    self.stage_attachment(&origin, value)?;
                }
                m::Attachment::Unavailable { tab_id, .. } => {
                    if *tab_id != expected_tab {
                        return Err(crate::Status::Invalid.into());
                    }
                }
            }
        }
        Ok(reply)
    }
    pub async fn take_control(&mut self) -> Result<m::Reply<m::Attachment>, ManagerRecoveryError> {
        self.ensure_live_engine()?;
        let current = self.installed_origin()?;
        let request = current.witness.commit_if_current(|| {
            let mut state = self.state.lock().map_err(|_| crate::Status::Panic)?;
            current.check(&state)?;
            state
                .cursor
                .as_mut()
                .ok_or(crate::Status::Stale)?
                .takeover_request(current.witness.binding())
                .map_err(ManagerRecoveryError::Cursor)
        })??;
        let expected_tab = request.delivery.tab_id;
        let expected_generation = request.delivery.generation;
        let origin = self.begin_recovery_from(current.witness)?;
        self.recover_request(origin, request, expected_tab, Some(expected_generation))
            .await
    }
    pub(super) fn stage_attachment(
        &mut self,
        origin: &RecoveryOrigin,
        value: &m::Attachment,
    ) -> Result<u64, ManagerRecoveryError> {
        let cursor = ManagerCursor::from_selection(origin.witness.selection().clone(), value)
            .map_err(ManagerRecoveryError::Cursor)?;
        let charge = serde_json::to_vec(cursor.recovery())
            .map_err(|_| crate::Status::Invalid)?
            .len();
        if charge > crate::MAX_QUEUE {
            return Err(crate::Status::Busy.into());
        }
        let shared = crate::engine::lookup(self.terminal)?;
        let state = self.state.clone();
        shared
            .queue
            .lock()
            .map_err(|_| crate::Status::Panic)?
            .reserve_native0()?;
        let work = Box::new(RecoveryWork {
            cursor,
            receiver: self.receiver,
            limits: self.limits,
            origin: origin.clone(),
            state: state.clone(),
            shared: shared.clone(),
            charge,
        });
        let mut retired_cursor = None;
        // Network workers may enqueue; only the native owner decodes/installs
        // this work when it pumps the existing engine queue.
        let ticket = origin
            .witness
            .commit_if_current(|| {
                let mut state = state.lock().map_err(|_| crate::Status::Panic)?;
                origin.check(&state)?;
                if state.origin.is_some() {
                    return Err(crate::Status::Stale);
                }
                let mut queue = shared.queue.lock().map_err(|_| crate::Status::Panic)?;
                if queue.closed || queue.failed || queue.chunks.capacity() == 0 {
                    return Err(crate::Status::Busy);
                }
                queue.discard_pending();
                let ticket = queue.submit_native0(work)?;
                retired_cursor = state.cursor.take();
                state.failure = None;
                state.pending_ack = None;
                state.installed = None;
                state.origin = Some(origin.clone());
                state.data.clear();
                Ok(ticket)
            })
            .map_err(|_| crate::Status::Stale)??;
        drop(retired_cursor);
        Ok(ticket)
    }
}
