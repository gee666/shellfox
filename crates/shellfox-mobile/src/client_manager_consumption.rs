use super::*;
use shellfox_client::manager_v2::EventDisposition;
impl<T: OutboundTransport> ManagerRecovery<T> {
    pub(super) fn installed_origin(&self) -> Result<RecoveryOrigin, ManagerRecoveryError> {
        let origin = self
            .state
            .lock()
            .map_err(|_| crate::Status::Panic)?
            .origin
            .clone()
            .ok_or(crate::Status::Stale)?;
        origin.witness.commit_if_current(|| {
            let state = self.state.lock().map_err(|_| crate::Status::Panic)?;
            origin.check(&state)?;
            if state
                .cursor
                .as_ref()
                .is_none_or(|cursor| cursor.binding() != origin.witness.binding())
            {
                return Err(crate::Status::Stale);
            }
            Ok(())
        })??;
        Ok(origin)
    }
    /// Capture the installed view's ORIGINAL origin before polling. Bare events
    /// are never admitted using whatever PAIR happens to be current afterward.
    pub async fn poll(&mut self) -> Result<m::Reply<m::PollResponse>, ManagerRecoveryError> {
        let origin = self.installed_origin()?;
        let reply = self.client.poll_with_witness(&origin.witness).await?;
        if let m::Reply::Success(value) = &reply {
            for host in &value.events {
                if let m::HostEvent::Terminal { event } = host {
                    if matches!(
                        self.observe_from(&origin, event)?,
                        EventDisposition::Reattach | EventDisposition::Revoked
                    ) {
                        break;
                    }
                }
            }
        }
        Ok(reply)
    }
    pub(super) fn observe_from(
        &mut self,
        origin: &RecoveryOrigin,
        event: &m::Event,
    ) -> Result<EventDisposition, ManagerRecoveryError> {
        let target = super::global_poll::Target {
            origin: origin.clone(),
            state: self.state.clone(),
            shared: crate::engine::lookup(self.terminal)?,
            tab: event.delivery().tab_id,
        };
        target.apply(event)
    }
    pub async fn acknowledge_consumed(
        &self,
    ) -> Result<m::Reply<shellfox_protocol::Acknowledged>, ManagerRecoveryError> {
        let _permit = self
            .ack_slot
            .try_acquire()
            .map_err(|_| crate::Status::Busy)?;
        let origin = self.installed_origin()?;
        let binding = origin.witness.binding();
        let shared = crate::engine::lookup(self.terminal)?;
        let ack = origin.witness.commit_if_current(
            || -> Result<m::AcknowledgeTerminal, ManagerRecoveryError> {
                let mut state = self.state.lock().map_err(|_| crate::Status::Panic)?;
                origin.check(&state)?;
                let queue = shared.queue.lock().map_err(|_| crate::Status::Panic)?;
                if queue.closed
                    || queue.failed
                    || state
                        .cursor
                        .as_ref()
                        .is_none_or(|cursor| cursor.binding() != binding)
                {
                    return Err(crate::Status::Stale.into());
                }
                if let Some(ack) = &state.pending_ack {
                    return Ok(ack.clone());
                }
                let ack = if let Some((ticket, id)) = state.installed {
                    if queue.consumed < ticket {
                        return Err(crate::Status::Busy.into());
                    }
                    state
                        .cursor
                        .as_mut()
                        .ok_or(crate::Status::Stale)?
                        .consumed_recovery(id, binding)
                        .map_err(ManagerRecoveryError::Cursor)?
                } else {
                    let sequence = state
                        .data
                        .iter()
                        .take_while(|(ticket, _)| *ticket <= queue.consumed)
                        .last()
                        .map(|(_, seq)| *seq)
                        .ok_or(crate::Status::Busy)?;
                    state
                        .cursor
                        .as_mut()
                        .ok_or(crate::Status::Stale)?
                        .consumed_data(sequence, binding)
                        .map_err(ManagerRecoveryError::Cursor)?
                };
                state.pending_ack = Some(ack.clone());
                Ok(ack)
            },
        )??;
        // Same original binding/cycle at request admission AND reply application.
        let reply = self
            .client
            .execute_with_witness(&origin.witness, &ack)
            .await?;
        if let m::Reply::Success(applied) = &reply {
            origin
                .witness
                .commit_if_current(|| -> Result<(), ManagerRecoveryError> {
                    let mut state = self.state.lock().map_err(|_| crate::Status::Panic)?;
                    origin.check(&state)?;
                    state
                        .cursor
                        .as_mut()
                        .ok_or(crate::Status::Stale)?
                        .acknowledged(&ack, applied, binding)
                        .map_err(ManagerRecoveryError::Cursor)?;
                    match ack.ack {
                        m::Ack::Recovery { .. } => state.installed = None,
                        m::Ack::Data { sequence } => {
                            while state.data.front().is_some_and(|(_, seq)| *seq <= sequence) {
                                state.data.pop_front();
                            }
                        }
                    }
                    state.pending_ack = None;
                    Ok(())
                })??;
        }
        Ok(reply)
    }
}
