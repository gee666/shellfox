//! Peer closeTab/deleteSession run as the owner's non-blocking close. The
//! request is answered once the close finishes; its metadata commit runs
//! inside a fresh authorization check, so a peer revoked meanwhile commits
//! nothing. While any close is in flight, peer requests other than Poll wait
//! in `held`, in arrival order.
use super::*;
use crate::close::CloseTarget;
use shellfox_remote::OwnerRequest;
use std::{collections::VecDeque, sync::mpsc};

/// Peer requests held behind a close at most; more are refused.
const MAX_HELD: usize = 64;

pub(super) enum Kind {
    Tab { session: SessionId },
    Session { session: SessionId },
}
pub(crate) struct Closing {
    kind: Kind,
    outcome: mpsc::Receiver<anyhow::Result<Snapshot>>,
    /// The peer request, kept by `ServiceHost::keep` after its dispatch.
    request: Option<OwnerRequest>,
}
#[derive(Default)]
pub(crate) struct Held(pub(super) VecDeque<OwnerRequest>);
/// The peer lost authorization before the commit; nothing was committed.
#[derive(Debug)]
struct Refused(AppError);
impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0.message)
    }
}
impl std::error::Error for Refused {}

impl RemoteService<'_> {
    /// Starts a gated close from an authorized dispatch. Answered by `finish_close`.
    pub(super) fn start_close(
        &mut self,
        actor: &Actor,
        command: RemoteCommand,
        target: CloseTarget,
        kind: Kind,
    ) -> Result<serde_json::Value, AppError> {
        if self.state.closing.is_some() {
            return Err(failure(
                ErrorCode::NativeUnavailable,
                "The host is busy closing a tab.",
            ));
        }
        let (send, outcome) = mpsc::channel();
        let actor = actor.clone();
        let gate: crate::close::CommitGate = Box::new(move |commit| {
            let mut failed = None;
            let authorized = actor.with_authorized(Some(&command), || {
                commit().map_err(|error| {
                    failed = Some(error);
                    failure(ErrorCode::NativeUnavailable, "")
                })
            });
            authorized.map_err(|refused| failed.unwrap_or_else(|| Refused(refused).into()))
        });
        self.service.close_request_gated(
            target,
            gate,
            Box::new(move |result| {
                let _ = send.send(result);
            }),
        );
        self.state.closing = Some(Closing {
            kind,
            outcome,
            request: None,
        });
        Ok(serde_json::Value::Null)
    }
    /// Keeps the request whose dispatch just started a close.
    pub(super) fn keep_closing(&mut self, request: OwnerRequest) -> Option<OwnerRequest> {
        match &mut self.state.closing {
            Some(closing) if closing.request.is_none() => {
                closing.request = Some(request);
                None
            }
            _ => Some(request),
        }
    }
}
impl RemoteState {
    /// Holds `request` behind a close in flight. Returns it when it may run now.
    pub(super) fn hold(
        &mut self,
        service: &Service,
        request: OwnerRequest,
    ) -> Option<OwnerRequest> {
        // Disconnect only releases this view; it writes no metadata.
        if !service.metadata_busy()
            || matches!(
                request.command,
                OwnerCommand::Poll { .. } | OwnerCommand::Disconnect { .. }
            )
        {
            return Some(request);
        }
        if self.held.0.len() >= MAX_HELD {
            request.complete(Err(failure(
                ErrorCode::NativeUnavailable,
                "The host is busy closing a tab. Operation was not queued.",
            )));
        } else {
            self.held.0.push_back(request);
        }
        None
    }
    /// Puts a request taken by `next_held` back at the front.
    pub(super) fn unhold(&mut self, request: OwnerRequest) {
        self.held.0.push_front(request);
    }
    /// The oldest held request, once no close is in flight.
    pub(super) fn next_held(&mut self, service: &Service) -> Option<OwnerRequest> {
        if service.metadata_busy() {
            return None;
        }
        self.held.0.pop_front()
    }
    /// Answers the peer request of a finished close.
    pub(super) fn finish_close(&mut self, service: &mut Service) {
        let outcome = match self
            .closing
            .as_ref()
            .map(|closing| closing.outcome.try_recv())
        {
            None | Some(Err(mpsc::TryRecvError::Empty)) => return,
            Some(Ok(outcome)) => outcome,
            Some(Err(mpsc::TryRecvError::Disconnected)) => {
                Err(anyhow::anyhow!("The close ended without an outcome."))
            }
        };
        let Some(Closing {
            kind,
            request: Some(request),
            ..
        }) = self.closing.take()
        else {
            return;
        };
        // A revoked peer gets its authorization error, not a cleanup failure.
        let outcome = outcome.map_err(|error| error.downcast::<Refused>());
        let response = match (outcome, kind) {
            (Err(Ok(Refused(error))), _) => Err(error),
            (Ok(snapshot), Kind::Tab { session }) => self
                .bind(service)
                .session(&snapshot, session)
                .and_then(checked),
            (Ok(_), Kind::Session { session }) => {
                self.facts.sessions.remove(&session);
                checked(wire::Deleted { deleted: true })
            }
            (Err(_), Kind::Tab { .. }) => Err(failure(
                ErrorCode::NativeUnavailable,
                "Owned cleanup not confirmed; metadata retained.",
            )),
            (Err(_), Kind::Session { .. }) => Err(failure(
                ErrorCode::NativeUnavailable,
                "Owned terminal cleanup not confirmed; metadata retained.",
            )),
        };
        // The close committed under its own cause, in an earlier pass.
        service.finish_remote(request, response, service.durability.barrier());
    }
}
