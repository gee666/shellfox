//! 0.9's one-shot pattern over `--host`: `shellfox --host <peer> api writeTerminal`
//! after an attachTerminal from another process, each call its own view.
use super::v1_terminal::{accessible, lost};
use super::*;
use crate::{grant::requests::GrantFailure, terminals::Attachment};
use shellfox_core::{Generation, TabId};

impl RemoteService<'_> {
    /// The running, remotely reachable terminal of `tab` at `generation`.
    pub(super) fn running(&self, tab: TabId, generation: Generation) -> Result<(), AppError> {
        let snapshot = self.service.host().snapshot().map_err(host_error)?;
        if self.private_tabs(&snapshot).contains(&tab) {
            return Err(failure(
                ErrorCode::AccessDenied,
                "Elevated sources remain local-only.",
            ));
        }
        match self.service.terminals.get(&tab) {
            Some(terminal)
                if accessible(terminal) && terminal.runtime.identity().generation == generation =>
            {
                Ok(())
            }
            _ => Err(lost()),
        }
    }

    /// A view that never attached `tab` borrows control for one call, only
    /// when no other view holds it, then hands it back.
    pub(super) fn with_borrowed_control<T>(
        &mut self,
        actor: &Actor,
        tab: TabId,
        generation: Generation,
        operation: impl FnOnce(&mut Service, &Attachment) -> Result<T, AppError>,
    ) -> Result<T, AppError> {
        self.running(tab, generation)?;
        let controller = self
            .state
            .views
            .get(&key(actor))
            .ok_or_else(lost)?
            .controller;
        self.service.settle_control_release(tab);
        let offer = match self.service.attach_manager(tab, controller, false) {
            Ok(offer) => offer,
            Err(error) if matches!(error.downcast_ref::<GrantFailure>(), Some(GrantFailure::Rejected(error)) if error.code == shellfox_runtime::ErrorCode::ControlRequired) =>
            {
                return Err(failure(
                    ErrorCode::TerminalControlRequired,
                    "This terminal is in use by another view. Take it to view or control it.",
                ));
            }
            Err(_) => return Err(lost()),
        };
        let lease = self
            .service
            .adopt_controller_grant(offer)
            .map_err(|_| lost())?;
        let result = operation(self.service, &lease);
        let _ = self.service.detach_attachment(tab, &lease);
        self.service.settle_control_release(tab);
        result
    }
}
