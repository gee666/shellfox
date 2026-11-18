//! Existing client registry creation; one shared connection per client handle.
use super::*;
pub(in crate::client) fn create(
    peer: shellfox_protocol::AuthenticatedPeerId,
    identity: ClientId,
) -> Result<u64> {
    create_mode(peer, Creator::Legacy(identity))
}
pub(in crate::client) fn create_observer(
    peer: shellfox_protocol::AuthenticatedPeerId,
    identity: shellfox_protocol::observer_v2::Id,
) -> Result<u64> {
    create_mode(peer, Creator::Observer(identity))
}
#[cfg(feature = "experimental-shellfox-native")]
pub(in crate::client) fn create_manager(
    peer: shellfox_protocol::AuthenticatedPeerId,
    identity: shellfox_protocol::manager_v2::Id,
) -> Result<u64> {
    use shellfox_ghostty_vt::extension::VerifiedNativeExtension;
    use shellfox_protocol::{manager_v2 as m, Validate};
    let verified = VerifiedNativeExtension::verify().map_err(|_| PROTOCOL)?;
    let identity_text =
        String::from_utf8(verified.identity().as_bytes().to_vec()).map_err(|_| PROTOCOL)?;
    let receiver =
        m::Native0Codec::new(m::EngineIdentity::try_from(identity_text).map_err(|_| PROTOCOL)?);
    receiver.validate().map_err(|_| PROTOCOL)?;
    create_mode(peer, Creator::Manager(identity, receiver))
}
enum Creator {
    Legacy(ClientId),
    Observer(shellfox_protocol::observer_v2::Id),
    #[cfg(feature = "experimental-shellfox-native")]
    Manager(
        shellfox_protocol::manager_v2::Id,
        shellfox_protocol::manager_v2::Native0Codec,
    ),
}
fn create_mode(peer: shellfox_protocol::AuthenticatedPeerId, creator: Creator) -> Result<u64> {
    runtime()?;
    let mut registry = registry().lock().map_err(|_| PANIC)?;
    if registry.clients.len() >= 16 {
        return Err(BUSY);
    }
    let id = registry.next;
    registry.next = id.checked_add(1).ok_or(BUSY)?;
    let mobile = Arc::new_cyclic(|weak| Mobile {
        id,
        driver: match creator {
            #[cfg(feature = "experimental-shellfox-native")]
            Creator::Manager(identity, receiver) => Driver::Manager(Arc::new(
                shellfox_client::manager_v2::ManagerClient::native0(
                    peer,
                    identity,
                    super::super::transport::NativeTransport(weak.clone()),
                    receiver,
                )
                .expect("receiver was verified and validated before registry creation"),
            )),
            Creator::Legacy(identity) => Driver::Legacy(Client::new(
                peer,
                identity,
                super::super::transport::NativeTransport(weak.clone()),
            )),
            Creator::Observer(identity) => {
                Driver::Observer(shellfox_client::observer_v2::ObserverClient::new(
                    peer,
                    identity,
                    super::super::transport::NativeTransport(weak.clone()),
                ))
            }
        },
        state: Mutex::new(State::default()),
        dispatch_gate: Mutex::new(()),
        request_slots: tokio::sync::Semaphore::new(8),
        mutation_slots: tokio::sync::Semaphore::new(4),
    });
    registry.clients.insert(id, mobile);
    Ok(id)
}
