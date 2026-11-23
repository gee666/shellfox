//! Existing client registry creation; one shared connection per client handle.
use super::*;
pub(in crate::client) fn create(
    peer: shellfox_protocol::AuthenticatedPeerId,
    identity: ClientId,
) -> Result<u64> {
    runtime()?;
    let mut registry = registry().lock().map_err(|_| PANIC)?;
    if registry.clients.len() >= 16 {
        return Err(BUSY);
    }
    let id = registry.next;
    registry.next = id.checked_add(1).ok_or(BUSY)?;
    let mobile = Arc::new_cyclic(|weak| Mobile {
        id,
        client: Client::new(
            peer,
            identity,
            super::super::transport::NativeTransport(weak.clone()),
        ),
        state: Mutex::new(State::default()),
        dispatch_gate: Mutex::new(()),
        waker: Default::default(),
        request_slots: tokio::sync::Semaphore::new(8),
        mutation_slots: tokio::sync::Semaphore::new(4),
    });
    registry.clients.insert(id, mobile);
    Ok(id)
}
