use super::*;
use shellfox_core::{Generation, Session, TabId};
use shellfox_runtime::{AnsiRecoveryCapability, Command, Identity, Response};

pub(super) struct Replay {
    identity: Identity,
    epoch: u64,
    last_sequence: u64,
}

pub(super) fn read_only() -> AppError {
    failure(ErrorCode::AccessDenied, "Automation replay is read-only.")
}

impl Replay {
    pub(super) fn generation(&self) -> Generation {
        self.identity.generation
    }

    pub(super) fn acknowledge(
        &self,
        service: &Service,
        ack: &wire::AcknowledgeTerminal,
    ) -> Result<(), AppError> {
        if ack.generation != self.identity.generation
            || ack.control_epoch != self.epoch
            || service.terminals.get(&ack.tab_id).is_none_or(|terminal| {
                terminal.runtime.identity() != self.identity
                    || service.remote_automation_exit_code(terminal).is_none()
            })
        {
            return Err(super::v1_terminal::lost());
        }
        if ack.sequence != self.last_sequence {
            return Err(failure(
                ErrorCode::Validation,
                "ACK must match the replay snapshot.",
            ));
        }
        Ok(())
    }
}

impl View {
    pub(super) fn check_capacity(&self, tab: TabId) -> Result<(), AppError> {
        if self.terminals.len() + self.replays.len() >= 16
            && !self.terminals.contains_key(&tab)
            && !self.replays.contains_key(&tab)
        {
            return Err(failure(
                ErrorCode::Validation,
                "Detach an unused terminal before attaching another.",
            ));
        }
        Ok(())
    }
}

impl RemoteService<'_> {
    pub(super) fn attach_automation_replay(
        &mut self,
        actor: &Actor,
        session: &Session,
        tab: TabId,
    ) -> Result<serde_json::Value, AppError> {
        let lost = super::v1_terminal::lost;
        let terminal = self.service.terminals.get(&tab).ok_or_else(lost)?;
        let identity = terminal.runtime.identity();
        let exit_code = self
            .service
            .remote_automation_exit_code(terminal)
            .filter(|_| identity.session_id == session.id)
            .ok_or_else(|| {
                failure(
                    ErrorCode::AccessDenied,
                    "No completed non-elevated replay is available.",
                )
            })?;
        let view = self.state.views.get_mut(&key(actor)).ok_or_else(lost)?;
        view.check_capacity(tab)?;
        let epoch = self.service.allocate_viewer_id().map_err(|_| lost())?;
        let offer = self
            .service
            .observer_grant(tab, epoch)
            .map_err(|_| lost())?;
        if offer.identity() != identity {
            return Err(lost());
        }
        let recovery = self.service.call(
            tab,
            Command::ObserverRecoverStamped {
                observer: offer.lease(),
                capability: AnsiRecoveryCapability::RemoteV1,
            },
        );
        // This is a single immutable snapshot. Retire the temporary observer;
        // no controller or ongoing delivery is kept for the v1 client.
        drop(offer);
        let Ok(Response::Recovery(recovery)) = recovery else {
            return Err(failure(
                ErrorCode::NativeUnavailable,
                "Could not build a bounded automation snapshot.",
            ));
        };
        let text = String::from_utf8_lossy(&recovery.bytes);
        let chunks = super::v1_delivery::snapshot_chunks(tab, identity.generation, &text);
        let last_sequence = chunks.len() as u64;
        // V1 has no observer attachment. Closed + owned is its existing replay
        // envelope; this epoch only acknowledges the snapshot, never input.
        let reply = checked(wire::TerminalAttachment::Owned(wire::OwnedAttachment {
            tab_id: tab,
            session_id: session.id,
            generation: identity.generation,
            control_epoch: epoch,
            first_sequence: 1,
            last_sequence,
            chunks,
            truncated: true,
            snapshot: Some(true),
            state: wire::ScreenState::Closed,
            exit_code: Some(exit_code),
            cols: recovery.cols,
            rows: recovery.rows,
            lifetime: wire::TerminalLifetime::AppOwned,
            settled_at: Some(session.settled_at.clone()),
        }))?;
        view.replays.insert(
            tab,
            Replay {
                identity,
                epoch,
                last_sequence,
            },
        );
        Ok(reply)
    }
}
