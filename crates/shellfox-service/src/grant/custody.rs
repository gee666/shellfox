//! One bounded owner ledger for unresolved Runtime ownership requests.
use shellfox_protocol::CancellationToken;
use shellfox_runtime::{Event, Identity, RequestId, Response, RuntimeError};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use uuid::Uuid;
const MAX_GRANTS: usize = 128;
type RetirementCandidate = (GrantTicket, ActualGrant, Option<(RequestId, bool)>);
mod types;
pub(crate) use types::*;
struct Entry {
    ticket: GrantTicket,
    kind: GrantKind,
    sent: bool,
    actual: Option<ActualGrant>,
    rejected: Option<RuntimeError>,
    retired: bool,
    abandoned: Arc<AtomicBool>,
    cancel: CancellationToken,
    cleanup: Option<(RequestId, bool)>,
    /// Cancelled when the entry leaves the ledger; ends its cancel watcher.
    removed: CancellationToken,
}
impl Drop for Entry {
    fn drop(&mut self) {
        self.removed.cancel();
    }
}
#[derive(Default)]
pub(crate) struct GrantCustody {
    entries: Vec<Entry>,
}
impl GrantCustody {
    pub fn reserve(
        &mut self,
        identity: Identity,
        request: RequestId,
        kind: GrantKind,
        cancel: CancellationToken,
    ) -> Result<(GrantTicket, GrantGuard), CustodyError> {
        if request.0 == 0 || matches!(kind, GrantKind::Controller(0) | GrantKind::Observer(0)) {
            return Err(CustodyError::NotSent);
        }
        if self.entries.iter().any(|entry| {
            let pending_claim = matches!(
                (kind, entry.kind),
                (GrantKind::Controller(_), GrantKind::Controller(_))
            ) && entry.sent
                && entry.actual.is_none();
            entry.ticket.identity == identity
                && (entry.kind == kind || pending_claim)
                && !entry.retired
                && entry.rejected.is_none()
        }) {
            return Err(CustodyError::Conflict);
        }
        if self.entries.len() >= MAX_GRANTS {
            return Err(CustodyError::Full);
        }
        let ticket = GrantTicket {
            identity,
            request,
            nonce: Uuid::new_v4(),
        };
        let abandoned = Arc::new(AtomicBool::new(false));
        self.entries.push(Entry {
            ticket,
            kind,
            sent: false,
            actual: None,
            rejected: None,
            retired: false,
            abandoned: abandoned.clone(),
            cancel,
            cleanup: None,
            removed: CancellationToken::new(),
        });
        Ok((
            ticket,
            GrantGuard {
                abandoned,
                armed: true,
            },
        ))
    }
    fn entry(&self, ticket: GrantTicket) -> Result<&Entry, CustodyError> {
        self.entries
            .iter()
            .find(|entry| entry.ticket == ticket)
            .ok_or(CustodyError::Stale)
    }
    fn entry_mut(&mut self, ticket: GrantTicket) -> Result<&mut Entry, CustodyError> {
        self.entries
            .iter_mut()
            .find(|entry| entry.ticket == ticket)
            .ok_or(CustodyError::Stale)
    }
    pub fn recipient_cancelled(&self, ticket: GrantTicket) -> Result<bool, CustodyError> {
        let entry = self.entry(ticket)?;
        Ok(entry.cancel.is_cancelled() || entry.abandoned.load(Ordering::Acquire))
    }
    pub fn retire_owned(
        &mut self,
        identity: Identity,
        id: RequestId,
        actual: ActualGrant,
    ) -> Result<(), CustodyError> {
        if self.entries.iter().any(|entry| {
            entry.ticket.identity == identity && entry.actual == Some(actual) && !entry.retired
        }) {
            return Ok(());
        }
        let kind = match actual {
            ActualGrant::Controller(lease) => GrantKind::Controller(lease.controller),
            ActualGrant::Observer(lease) => GrantKind::Observer(lease.viewer),
        };
        let (ticket, guard) = self.reserve(identity, id, kind, CancellationToken::new())?;
        self.sent(ticket)?;
        self.entry_mut(ticket)?.actual = Some(actual);
        drop(guard);
        Ok(())
    }
    pub fn sent(&mut self, ticket: GrantTicket) -> Result<(), CustodyError> {
        self.entry_mut(ticket)?.sent = true;
        Ok(())
    }
    pub fn not_sent(&mut self, ticket: GrantTicket) -> Result<(), CustodyError> {
        if self.entry(ticket)?.sent {
            return Err(CustodyError::Conflict);
        }
        self.entries.retain(|entry| entry.ticket != ticket);
        Ok(())
    }
    pub fn poll(&self, ticket: GrantTicket) -> Result<GrantPoll, CustodyError> {
        let entry = self.entry(ticket)?;
        Ok(if entry.retired {
            GrantPoll::Retired
        } else if let Some(error) = &entry.rejected {
            GrantPoll::Rejected(error.clone())
        } else if entry.cleanup.is_some() {
            GrantPoll::Retiring
        } else if let Some(actual) = entry.actual {
            GrantPoll::Granted(actual)
        } else {
            GrantPoll::Pending
        })
    }
    pub fn acknowledge_rejection(&mut self, ticket: GrantTicket) -> Result<(), CustodyError> {
        if self.entry(ticket)?.rejected.is_none() {
            return Err(CustodyError::Conflict);
        }
        self.entries.retain(|entry| entry.ticket != ticket);
        Ok(())
    }
    pub fn adopt(
        &mut self,
        ticket: GrantTicket,
        expected: ActualGrant,
    ) -> Result<(), CustodyError> {
        let entry = self.entry(ticket)?;
        // This acquire is the adoption linearization point. Later recipient
        // cancellation cannot release an entry already transferred to its book.
        if entry.cancel.is_cancelled() {
            return Err(CustodyError::Cancelled);
        }
        if entry.cleanup.is_some_and(|(_, sent)| sent) {
            return Err(CustodyError::ReleaseSent);
        }
        if entry.retired {
            return Err(CustodyError::Stale);
        }
        if entry.actual != Some(expected) {
            return Err(CustodyError::WrongReply);
        }
        self.entries.retain(|entry| entry.ticket != ticket);
        Ok(())
    }
    /// Matching actual Claim/Observe errors prove no NEW grant, not rollback
    /// of source cleanup. Host errors/timeouts/panics never enter this function.
    pub fn record_reply(
        &mut self,
        identity: Identity,
        id: RequestId,
        result: &Result<Response, RuntimeError>,
    ) -> Result<bool, CustodyError> {
        for entry in &mut self.entries {
            if entry.ticket.identity != identity {
                continue;
            }
            if entry
                .cleanup
                .is_some_and(|(request, sent)| request == id && sent)
            {
                if matches!(result, Ok(Response::Applied))
                    || matches!(result,Err(error) if error.code==shellfox_runtime::ErrorCode::ControlRequired)
                {
                    entry.retired = true;
                    entry.cleanup = None;
                }
                return Ok(true);
            }
            if entry.ticket.request != id || !entry.sent {
                continue;
            }
            if let Some(actual) = entry.actual {
                return if matches!((actual,result),(ActualGrant::Controller(old),Ok(Response::Claimed(new))) if old==*new)
                    || matches!((actual,result),(ActualGrant::Observer(old),Ok(Response::Observed(new))) if old==*new)
                {
                    Ok(true)
                } else {
                    Err(CustodyError::WrongReply)
                };
            }
            if entry.rejected.is_some() {
                return Err(CustodyError::WrongReply);
            }
            match (entry.kind, result) {
                (GrantKind::Controller(expected), Ok(Response::Claimed(lease)))
                    if expected == lease.controller =>
                {
                    entry.actual = Some(ActualGrant::Controller(*lease))
                }
                (GrantKind::Observer(expected), Ok(Response::Observed(lease)))
                    if expected == lease.viewer =>
                {
                    entry.actual = Some(ActualGrant::Observer(*lease))
                }
                (_, Err(error)) => entry.rejected = Some(error.clone()),
                _ => return Err(CustodyError::WrongReply),
            }
            return Ok(true);
        }
        Ok(false)
    }
    pub fn record_revocation(&mut self, identity: Identity, event: &Event) {
        if let Event::ControllerRevoked { lease, .. } = event {
            for entry in &mut self.entries {
                if entry.ticket.identity == identity
                    && entry.actual == Some(ActualGrant::Controller(*lease))
                {
                    entry.retired = true;
                    entry.cleanup = None;
                }
            }
        }
    }
    /// The recipient's cancel token and a token cancelled once the entry is
    /// removed, so a watcher can wake the owner to retire an abandoned grant.
    pub fn cancel_watch(
        &self,
        ticket: GrantTicket,
    ) -> Option<(CancellationToken, CancellationToken)> {
        self.entries
            .iter()
            .find(|entry| entry.ticket == ticket)
            .map(|entry| (entry.cancel.clone(), entry.removed.clone()))
    }
    pub fn retirements(&self) -> Vec<RetirementCandidate> {
        self.entries
            .iter()
            .filter(|entry| {
                !entry.retired
                    && entry.rejected.is_none()
                    && (entry.abandoned.load(Ordering::Acquire) || entry.cancel.is_cancelled())
            })
            .filter_map(|entry| Some((entry.ticket, entry.actual?, entry.cleanup)))
            .collect()
    }
    pub fn prepare_retirement(
        &mut self,
        ticket: GrantTicket,
        id: RequestId,
    ) -> Result<(), CustodyError> {
        let entry = self.entry_mut(ticket)?;
        if entry.actual.is_none() || entry.retired {
            return Err(CustodyError::Conflict);
        }
        if entry.cleanup.is_none() {
            entry.cleanup = Some((id, false));
        }
        Ok(())
    }
    pub fn retirement_sent(
        &mut self,
        ticket: GrantTicket,
        id: RequestId,
    ) -> Result<(), CustodyError> {
        let entry = self.entry_mut(ticket)?;
        if entry.cleanup != Some((id, false)) {
            return Err(CustodyError::Stale);
        }
        entry.cleanup = Some((id, true));
        Ok(())
    }
    pub fn destroy_source(&mut self, identity: Identity) {
        self.entries
            .retain(|entry| entry.ticket.identity != identity);
    }
    pub fn reap_closed(&mut self) {
        self.entries.retain(|entry| {
            !((entry.retired || entry.rejected.is_some())
                && (entry.abandoned.load(Ordering::Acquire) || entry.cancel.is_cancelled()))
        });
    }
}
#[cfg(test)]
mod tests;
