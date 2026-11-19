use super::*;
use crate::{
    grant_requests::{GrantFailure, GrantHolder},
    terminals::Attachment,
};
use shellfox_core::{Generation, TabId, TabState};
use shellfox_runtime::SourceKind;

pub(super) fn lost() -> AppError {
    failure(
        ErrorCode::TargetLost,
        "Terminal generation or control changed. Reattach to continue.",
    )
}

fn accessible(terminal: &crate::terminals::OwnedTerminal) -> bool {
    terminal.runtime.source_kind() == SourceKind::OwnedPty
        && !super::v1_privilege::explicitly_elevated(&terminal.runtime.launch_privilege_facts())
}

impl RemoteService<'_> {
    pub(super) fn attach_terminal(
        &mut self,
        actor: &Actor,
        input: wire::AttachTerminal,
        takeover: bool,
    ) -> Result<serde_json::Value, AppError> {
        let tab = input.tab_id;
        let snapshot = self.service.host().snapshot().map_err(host_error)?;
        let (session, saved) = snapshot
            .sessions
            .iter()
            .find_map(|s| s.tabs.iter().find(|t| t.id == tab).map(|t| (s, t)))
            .ok_or_else(|| failure(ErrorCode::NotFound, "Terminal tab not found."))?;
        if self.private_tabs(&snapshot).contains(&tab) {
            return Err(failure(
                ErrorCode::AccessDenied,
                "Elevated sources remain local-only.",
            ));
        }
        let runtime = self.service.terminals.get(&tab);
        if input.generation.is_some_and(|expected| {
            runtime.is_none_or(|r| r.runtime.identity().generation != expected)
        }) {
            return Err(lost());
        }
        if runtime.is_none() {
            if saved.state == TabState::Archived {
                return Err(lost());
            }
            super::v1_privilege::ensure_normal_launch()?;
            observer::launch::ensure_launch_profile(self.state, saved.profile_id.as_str())?;
            self.service.start_terminal(tab).map_err(|_| {
                failure(
                    ErrorCode::NativeUnavailable,
                    "Terminal launch did not complete.",
                )
            })?;
        }
        let runtime = self.service.terminals.get(&tab).ok_or_else(lost)?;
        if !accessible(runtime) {
            return Err(failure(
                ErrorCode::AccessDenied,
                "This terminal does not run as an ordinary host-account PTY.",
            ));
        }
        let generation = runtime.runtime.identity().generation;
        let view = self.state.views.get_mut(&key(actor)).ok_or_else(lost)?;
        if !view.terminals.contains_key(&tab) && view.terminals.len() >= 16 {
            return Err(failure(
                ErrorCode::Validation,
                "Detach an unused terminal before attaching another.",
            ));
        }
        let offer = match self.service.attach_manager(tab, view.controller, takeover) {
            Ok(offer) => offer,
            Err(error) if matches!(error.downcast_ref::<GrantFailure>(), Some(GrantFailure::Rejected(error)) if error.code == shellfox_runtime::ErrorCode::ControlRequired) =>
            {
                return checked(wire::TerminalAttachment::Blocked {
                    tab_id: tab,
                    session_id: session.id,
                    generation,
                    control_epoch: self
                        .service
                        .terminals
                        .get(&tab)
                        .map_or(1, |t| t.control_epoch.max(1)),
                });
            }
            Err(_) => return Err(lost()),
        };
        let lease = self
            .service
            .adopt_controller_grant(offer, GrantHolder::LegacyController)
            .map_err(|_| lost())?;
        let projection = (|| {
            let (bytes, raw_cut, cols, rows) =
                self.service.remote_v1_recover(tab, &lease).map_err(|_| {
                    failure(
                        ErrorCode::NativeUnavailable,
                        "Could not build a bounded terminal snapshot.",
                    )
                })?;
            let (delivery, chunks) =
                super::v1_delivery::Delivery::snapshot(lease.clone(), tab, &bytes, raw_cut);
            let screen = if self
                .service
                .terminals
                .get(&tab)
                .is_some_and(|t| t.runtime.phase() == shellfox_runtime::Phase::Closed)
            {
                wire::ScreenState::Closed
            } else {
                wire::ScreenState::Open
            };
            let result = checked(wire::TerminalAttachment::Owned(wire::OwnedAttachment {
                tab_id: tab,
                session_id: session.id,
                generation,
                control_epoch: lease.epoch,
                first_sequence: 1,
                last_sequence: chunks.len() as u64,
                chunks,
                truncated: true,
                snapshot: Some(true),
                state: screen,
                exit_code: None,
                cols,
                rows,
                lifetime: wire::TerminalLifetime::AppOwned,
                settled_at: Some(session.settled_at.clone()),
            }))?;
            Ok((delivery, result))
        })();
        match projection {
            Ok((delivery, result)) => {
                view.terminals.insert(tab, delivery);
                Ok(result)
            }
            Err(error) => {
                let _ = self.service.detach_attachment(tab, &lease);
                Err(error)
            }
        }
    }

    fn terminal_lease(
        &self,
        actor: &Actor,
        tab: TabId,
        generation: Generation,
        epoch: u64,
    ) -> Result<Attachment, AppError> {
        let lease = self
            .state
            .views
            .get(&key(actor))
            .and_then(|view| view.terminals.get(&tab))
            .map(|d| &d.lease)
            .filter(|lease| lease.generation == generation && lease.epoch == epoch)
            .ok_or_else(lost)?;
        if self
            .service
            .terminals
            .get(&tab)
            .is_none_or(|terminal| !accessible(terminal))
        {
            return Err(lost());
        }
        self.service.lease(tab, lease).map_err(|_| lost())?;
        Ok(lease.clone())
    }

    pub(super) fn terminal_operation(
        &mut self,
        actor: &Actor,
        command: RemoteCommand,
    ) -> Result<serde_json::Value, AppError> {
        match command {
            RemoteCommand::AttachTerminal(input) => self.attach_terminal(actor, input, false),
            RemoteCommand::TakeTerminalControl(input) => {
                self.attach_terminal(
                    actor,
                    wire::AttachTerminal {
                        tab_id: input.tab_id,
                        generation: Some(input.generation),
                        after_sequence: None,
                    },
                    true,
                )?;
                checked(wire::Taken { taken: true })
            }
            RemoteCommand::WriteTerminal(input) => {
                let lease = self.terminal_lease(
                    actor,
                    input.tab_id,
                    input.generation,
                    input.control_epoch,
                )?;
                self.service.remote_v1_input(input, &lease)?;
                checked(wire::Written { written: true })
            }
            RemoteCommand::ResizeTerminal(input) => {
                let lease = self.terminal_lease(
                    actor,
                    input.tab_id,
                    input.generation,
                    input.control_epoch,
                )?;
                self.service
                    .resize(input.tab_id, &lease, input.cols, input.rows)
                    .map_err(|_| lost())?;
                checked(wire::Resized { resized: true })
            }
            RemoteCommand::AcknowledgeTerminal(input) => {
                self.terminal_lease(actor, input.tab_id, input.generation, input.control_epoch)?;
                self.state
                    .views
                    .get_mut(&key(actor))
                    .and_then(|view| view.terminals.get_mut(&input.tab_id))
                    .ok_or_else(lost)?
                    .acknowledge(self.service, input.tab_id, input.sequence)?;
                checked(wire::Acknowledged { acknowledged: true })
            }
            RemoteCommand::DetachTerminal(input) => {
                let view = self.state.views.get_mut(&key(actor)).ok_or_else(lost)?;
                if let Some(delivery) = view.terminals.get(&input.tab_id) {
                    if delivery.lease.generation != input.generation {
                        return Err(lost());
                    }
                    let _ = self
                        .service
                        .detach_attachment(input.tab_id, &delivery.lease);
                    view.terminals.remove(&input.tab_id);
                }
                checked(wire::Detached { detached: true })
            }
            _ => Err(failure(
                ErrorCode::Internal,
                "Not a terminal delivery operation.",
            )),
        }
    }
}
