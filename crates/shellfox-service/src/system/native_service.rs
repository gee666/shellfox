//! The real Service adapter. Only owner Task::Native admission can register a
//! controller. No wire method, PTY lease or guessed nonzero identity grants it.
use super::{
    error, NativeSystemBackend, SystemNativeAuthority, SystemNativeHandler, SystemNativeRequest,
    SystemNativeResponse, SystemResult,
};
use crate::Service;
use shellfox_protocol::ErrorCode;
use std::sync::Arc;

impl Service {
    /// Trusted non-wire owner bootstrap only. The final loss token is selected
    /// INSIDE this Service, not supplied by a frontend/other service/temporary boot.
    /// No installation action or default production write occurs here.
    pub fn install_system_native(&mut self, backend: Arc<NativeSystemBackend>) -> SystemResult<()> {
        accepting(self)?;
        if self.system_native_handler.is_some() {
            return Err(error(
                ErrorCode::Unsupported,
                "System settings provider is already registered on this owner.",
            ));
        }
        let handler = Arc::new(SystemNativeHandler::new(
            backend,
            self.ownership_lost.clone(),
        )?);
        accepting(self)?;
        self.system_native_handler = Some(handler);
        Ok(())
    }
    /// Only the original Task::Native System branch calls this after mapping its
    /// private client identity to the original owner-issued controller ID.
    pub(crate) fn admit_system_native(&mut self, controller: u64) -> SystemResult<()> {
        accepting(self)?;
        self.system_native_handler
            .as_ref()
            .ok_or_else(unavailable)?
            .admit_owner(controller)
    }
    pub(crate) fn handle_system_native(
        &self,
        request: SystemNativeRequest,
        controller: u64,
    ) -> SystemResult<SystemNativeResponse> {
        accepting(self)?;
        let handler = self
            .system_native_handler
            .as_ref()
            .ok_or_else(unavailable)?
            .clone();
        let authority = ServiceSystemAuthority {
            service: self,
            handler: &handler,
        };
        handler.handle(controller, request, &authority)
    }
    /// Detach/security revocation never reuses an old numeric controller grant.
    pub(crate) fn revoke_system_native(&mut self, controller: u64) {
        if let Some(handler) = &self.system_native_handler {
            handler.revoke_owner(controller);
        }
    }
    /// Fence first, before resource cleanup. Failed shutdown never reopens it.
    pub(crate) fn begin_stop_system_native(&mut self) {
        if let Some(handler) = &self.system_native_handler {
            handler.begin_stop();
        }
    }
}
struct ServiceSystemAuthority<'a> {
    service: &'a Service,
    handler: &'a SystemNativeHandler,
}
impl SystemNativeAuthority for ServiceSystemAuthority<'_> {
    fn validate_system_controller(&self, controller: u64) -> SystemResult<()> {
        accepting(self.service)?;
        self.handler.validate_owner(controller)
    }
}
fn accepting(service: &Service) -> SystemResult<()> {
    service.assert_accepting().map_err(|_| {
        error(
            ErrorCode::NativeUnavailable,
            "Original native system owner is lost or stopping; no action was retried.",
        )
    })
}
fn unavailable() -> shellfox_protocol::AppError {
    error(ErrorCode::Unsupported,"System settings are unavailable: no externally validated installation resources were registered on this owner.")
}
