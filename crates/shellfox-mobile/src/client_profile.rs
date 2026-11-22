use super::{runtime::*, transport::NativeTransport};
use shellfox_client::{observer_v2::ObserverClient, Client};
use shellfox_protocol::observer_v2 as v2;

/// Native parser input/layout APIs must also respect a bound delivery-only role.
/// Unbound fixture engines remain usable; a registry failure fails closed.
pub(crate) fn observer_bound(engine: u64) -> bool {
    // Rust-only Manager mirrors never inherit local C ABI input/layout control.
    let owner = match super::terminal::owners().lock() {
        Ok(owners) => owners.get(&engine).copied(),
        Err(_) => return true,
    };
    let Some(owner) = owner else {
        return false;
    };
    registry_lookup(owner).map_or(true, |mobile| mobile.is_observer())
}
pub(super) enum Driver {
    Legacy(Client<NativeTransport>),
    Observer(ObserverClient<NativeTransport>),
}
impl Mobile {
    pub fn legacy(&self) -> Result<&Client<NativeTransport>> {
        match &self.driver {
            Driver::Legacy(client) => Ok(client),
            Driver::Observer(_) => Err(INVALID),
        }
    }
    pub fn observer(&self) -> Result<&ObserverClient<NativeTransport>> {
        match &self.driver {
            Driver::Observer(client) => Ok(client),
            Driver::Legacy(_) => Err(INVALID),
        }
    }
    pub fn is_manager(&self) -> bool {
        {
            false
        }
    }
    pub fn is_observer(&self) -> bool {
        matches!(self.driver, Driver::Observer(_))
    }
    pub fn close_client(&self) {
        match &self.driver {
            Driver::Legacy(client) => client.suspend(),
            // Canonical reconnect is a local cancellation fence, never a send.
            Driver::Observer(client) => client.reconnect(v2::Id::new()),
        }
    }
}
