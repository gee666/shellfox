use super::{error, SystemResult};
use shellfox_protocol::ErrorCode;
use std::{
    collections::HashSet,
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
};

pub(super) struct SystemNativeAdmissions {
    controllers: Mutex<HashSet<u64>>,
    closing: AtomicBool,
}
impl SystemNativeAdmissions {
    pub fn new() -> Self {
        Self {
            controllers: Mutex::new(HashSet::new()),
            closing: AtomicBool::new(false),
        }
    }
    /// Called only by Service's crate-private Task::Native System admission hook.
    /// No wire/native request can mint this capability or select a controller ID.
    pub fn admit(&self, controller: u64) -> SystemResult<()> {
        if controller == 0 || self.closing.load(Ordering::SeqCst) {
            return Err(denied());
        }
        let mut controllers = self.controllers.lock().map_err(|_| denied())?;
        if self.closing.load(Ordering::SeqCst)
            || !controllers.contains(&controller) && controllers.len() >= 128
        {
            return Err(denied());
        }
        controllers.insert(controller);
        Ok(())
    }
    pub fn validate(&self, controller: u64) -> SystemResult<()> {
        if controller == 0
            || self.closing.load(Ordering::SeqCst)
            || !self
                .controllers
                .lock()
                .map_err(|_| denied())?
                .contains(&controller)
        {
            return Err(denied());
        }
        Ok(())
    }
    pub fn revoke(&self, controller: u64) {
        // Poisoning cannot grant authority. A poisoned mutex remains unusable
        // for validate/admit even if removal cannot be recorded here.
        if let Ok(mut controllers) = self.controllers.lock() {
            controllers.remove(&controller);
        }
    }
    pub fn is_closing(&self) -> bool {
        self.closing.load(Ordering::SeqCst)
    }
    pub fn begin_stop(&self) {
        self.closing.store(true, Ordering::SeqCst);
        if let Ok(mut controllers) = self.controllers.lock() {
            controllers.clear();
        }
    }
}
fn denied() -> shellfox_protocol::AppError {
    error(
        ErrorCode::AuthFailed,
        "System settings require an active owner-admitted local/native controller.",
    )
}
