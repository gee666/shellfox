//! Host-local revocable ticket around the SINGLE canonical wire Binding.
use crate::{error, Actor};
use shellfox_protocol::{manager_v2 as wire, AppError, AuthenticatedPeerId, ErrorCode};
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct ManagerConnection {
    actor: Actor,
    selected: wire::PairSelection,
    authenticated: Arc<wire::ConnectionBinding>,
    current: Arc<Mutex<bool>>,
}
impl ManagerConnection {
    pub(crate) fn new(
        actor: Actor,
        peer: AuthenticatedPeerId,
        selected: wire::PairSelection,
    ) -> Self {
        Self {
            authenticated: Arc::new(wire::ConnectionBinding::new(peer, selected.binding())),
            actor,
            selected,
            current: Arc::new(Mutex::new(true)),
        }
    }
    pub fn actor(&self) -> &Actor {
        &self.actor
    }
    pub fn selection(&self) -> &wire::PairSelection {
        &self.selected
    }
    pub fn binding(&self) -> wire::Binding {
        self.selected.binding()
    }
    pub fn authenticated(&self) -> &wire::ConnectionBinding {
        &self.authenticated
    }
    pub fn invalidate(&self) {
        if let Ok(mut current) = self.current.lock() {
            *current = false;
        }
    }
    pub fn check(
        &self,
        peer: &AuthenticatedPeerId,
        binding: &wire::Binding,
    ) -> Result<(), AppError> {
        self.authenticated
            .check(peer, binding)
            .map_err(|_| stale())?;
        if !*self.current.lock().map_err(|_| stale())? {
            return Err(stale());
        }
        Ok(())
    }
    /// Approval is held first, then the connection fence, through owner commit.
    /// Replacement/revocation cannot race a queued request into an old view.
    pub(crate) fn dispatch<T>(
        &self,
        command: Option<&wire::Command>,
        action: impl FnOnce() -> Result<T, AppError>,
    ) -> Result<T, AppError> {
        let legacy = command
            .and_then(crate::manager_policy::compatible)
            .as_ref()
            .and_then(crate::observer_policy::legacy_command);
        if let Some(command) = command {
            crate::manager_policy::authorize(&self.actor, command)?;
        }
        self.actor.with_authorized(legacy.as_ref(), || {
            let current = self.current.lock().map_err(|_| stale())?;
            if !*current {
                return Err(stale());
            }
            action()
        })
    }
}
fn stale() -> AppError {
    error(
        ErrorCode::AuthFailed,
        "Manager connection was replaced, withdrawn or revoked.",
        false,
    )
}

#[derive(Clone)]
pub enum ManagerOperation {
    Pair,
    Execute(wire::Command),
    Poll,
    Disconnect,
}
#[derive(Clone)]
pub struct ManagerOwnerRequest {
    pub connection: ManagerConnection,
    pub operation: ManagerOperation,
    pub failure: ManagerFailure,
}
#[derive(Clone, Default)]
pub struct ManagerFailure(Arc<Mutex<Option<wire::Error>>>);
impl ManagerFailure {
    pub fn report(&self, error: wire::Error) -> AppError {
        let app = AppError {
            code: error.code,
            message: error.message.clone(),
            retryable: error.retryable,
        };
        if let Ok(mut slot) = self.0.lock() {
            if slot.is_none() {
                *slot = Some(error);
            }
        }
        app
    }
    pub fn take(&self) -> Option<wire::Error> {
        self.0.lock().ok()?.take()
    }
}
impl ManagerOwnerRequest {
    pub fn new(connection: ManagerConnection, operation: ManagerOperation) -> Self {
        Self {
            connection,
            operation,
            failure: ManagerFailure::default(),
        }
    }
    pub fn dispatch<T>(&self, action: impl FnOnce() -> Result<T, AppError>) -> Result<T, AppError> {
        // Revoked/disconnected views still need their own manager cleanup.
        if matches!(self.operation, ManagerOperation::Disconnect) {
            return action();
        }
        let command = match &self.operation {
            ManagerOperation::Execute(command) => Some(command),
            _ => None,
        };
        self.connection.dispatch(command, action)
    }
}
