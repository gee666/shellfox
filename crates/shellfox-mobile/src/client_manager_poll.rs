//! One global poll with original per-view contexts captured before network IO.
use super::*;
use shellfox_client::manager_v2::EventDisposition;
use shellfox_protocol::OutboundTransport;
pub struct ManagerPollContext {
    witness: ManagerBindingWitness,
    targets: Vec<Target>,
}
pub(super) struct Target {
    pub origin: RecoveryOrigin,
    pub state: Arc<Mutex<ViewState>>,
    pub shared: Arc<crate::engine::Shared>,
    pub tab: m::Id,
}
impl Target {
    pub fn capture<T: OutboundTransport>(
        view: &ManagerRecovery<T>,
    ) -> Result<Self, ManagerRecoveryError> {
        let origin = view.installed_origin()?;
        let tab = origin.witness.commit_if_current(|| {
            let state = view.state.lock().map_err(|_| crate::Status::Panic)?;
            origin.check(&state)?;
            Ok::<_, crate::Status>(
                state
                    .cursor
                    .as_ref()
                    .ok_or(crate::Status::Stale)?
                    .delivery()
                    .tab_id,
            )
        })??;
        Ok(Self {
            origin,
            state: view.state.clone(),
            shared: crate::engine::lookup(view.terminal)?,
            tab,
        })
    }
    pub fn apply(&self, event: &m::Event) -> Result<EventDisposition, ManagerRecoveryError> {
        let origin = &self.origin;
        let binding = origin.witness.binding();
        if let m::Event::Data { data, sequence, .. } = event {
            let bytes = shellfox_protocol::observer_v2::decode_bytes(data, m::MAX_DATA_BYTES)
                .map_err(|_| crate::Status::Invalid)?;
            self.shared
                .queue
                .lock()
                .map_err(|_| crate::Status::Panic)?
                .reserve_native0()?;
            self.state
                .lock()
                .map_err(|_| crate::Status::Panic)?
                .data
                .try_reserve(1)
                .map_err(|_| crate::Status::Busy)?;
            return origin.witness.commit_if_current(|| {
                let mut state = self.state.lock().map_err(|_| crate::Status::Panic)?;
                origin.check(&state)?;
                let mut queue = self.shared.queue.lock().map_err(|_| crate::Status::Panic)?;
                if queue.closed
                    || queue.failed
                    || queue.chunks.len() >= queue.chunks.capacity()
                    || queue.chunks.len() >= crate::MAX_CHUNKS
                    || queue
                        .bytes
                        .checked_add(bytes.len())
                        .is_none_or(|n| n > crate::MAX_QUEUE)
                    || queue.accepted == u64::MAX
                    || state.data.len() >= crate::MAX_CHUNKS
                    || state.data.len() >= state.data.capacity()
                {
                    return Err(crate::Status::Busy.into());
                }
                let disposition = state
                    .cursor
                    .as_mut()
                    .ok_or(crate::Status::Stale)?
                    .observe(event, binding)
                    .map_err(ManagerRecoveryError::Cursor)?;
                if disposition == EventDisposition::Deliver {
                    let ticket = queue.submit_prepared_bytes(bytes)?;
                    state.data.push_back((ticket, *sequence));
                }
                Ok(disposition)
            })?;
        }
        let disposition = origin.witness.commit_if_current(|| {
            let mut state = self.state.lock().map_err(|_| crate::Status::Panic)?;
            origin.check(&state)?;
            state
                .cursor
                .as_mut()
                .ok_or(crate::Status::Stale)?
                .observe(event, binding)
                .map_err(ManagerRecoveryError::Cursor)
        })??;
        if matches!(
            disposition,
            EventDisposition::Reattach | EventDisposition::Revoked
        ) {
            origin.cancel.cancel();
            let mut state = self.state.lock().map_err(|_| crate::Status::Panic)?;
            if state.generation == origin.generation {
                state.generation = state.generation.saturating_add(1);
                state.origin = None;
                state.installed = None;
                state.pending_ack = None;
                state.data.clear();
                self.shared
                    .queue
                    .lock()
                    .map_err(|_| crate::Status::Panic)?
                    .discard_pending();
            }
        }
        Ok(disposition)
    }
}
impl ManagerPollContext {
    pub fn capture<T: OutboundTransport>(
        client: &ManagerClient<T>,
        views: &[&ManagerRecovery<T>],
    ) -> Result<Self, ManagerRecoveryError> {
        let witness = client.binding_witness()?;
        let mut targets = Vec::new();
        if views.len() > 32 {
            return Err(crate::Status::Busy.into());
        }
        for view in views {
            if !std::ptr::eq(view.client.as_ref(), client) {
                return Err(crate::Status::Invalid.into());
            }
            match Target::capture(view) {
                Ok(target) if target.origin.witness.same_origin(&witness) => targets.push(target),
                Ok(_) => return Err(crate::Status::Stale.into()),
                Err(ManagerRecoveryError::Engine(crate::Status::Stale)) => {}
                Err(error) => return Err(error),
            }
        }
        Ok(Self { witness, targets })
    }
    pub(super) fn captured(
        witness: ManagerBindingWitness,
        targets: Vec<Target>,
    ) -> Result<Self, ManagerRecoveryError> {
        if targets.len() > 32
            || targets
                .iter()
                .any(|t| !t.origin.witness.same_origin(&witness))
        {
            return Err(crate::Status::Stale.into());
        }
        Ok(Self { witness, targets })
    }
    pub async fn poll<T: OutboundTransport>(
        self,
        client: &ManagerClient<T>,
    ) -> Result<m::Reply<m::PollResponse>, ManagerRecoveryError> {
        self.receive(client, false).await
    }
    pub(super) async fn receive<T: OutboundTransport>(
        self,
        client: &ManagerClient<T>,
        retain_data: bool,
    ) -> Result<m::Reply<m::PollResponse>, ManagerRecoveryError> {
        let result = client.poll_with_witness(&self.witness).await?;
        let m::Reply::Success(response) = result else {
            return Ok(result);
        };
        let mut coarse = Vec::new();
        for host in response.events {
            match host {
                m::HostEvent::Terminal { event } => {
                    let Some(target) = self
                        .targets
                        .iter()
                        .find(|t| t.tab == event.delivery().tab_id)
                    else {
                        continue;
                    };
                    match target.apply(&event) {
                        Ok(_) => {}
                        Err(ManagerRecoveryError::Engine(crate::Status::Stale)) => continue,
                        Err(ManagerRecoveryError::Cursor(CursorError::Delivery(
                            shellfox_client::observer_v2::CursorError::StaleDelivery,
                        ))) => continue,
                        Err(error) => return Err(error),
                    }
                    if retain_data || !matches!(event, m::Event::Data { .. }) {
                        coarse.push(m::HostEvent::Terminal { event });
                    }
                }
                other => coarse.push(other),
            }
        }
        Ok(m::Reply::Success(m::PollResponse { events: coarse }))
    }
}
