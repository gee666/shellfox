//! Authoritative ownership-request admission/wait/adoption, never output ACK.
use crate::grant_custody::{
    ActualGrant, CustodyError, GrantGuard, GrantKind, GrantPoll, GrantTicket,
};
use crate::*;
use shellfox_protocol::CancellationToken;
use shellfox_runtime::{Command, Identity, Lease, ObserverLease, Request, RuntimeError};
pub(crate) struct RecipientCancellation(CancellationToken);
impl RecipientCancellation {
    pub fn new() -> Self {
        Self(CancellationToken::new())
    }
    pub fn token(&self) -> CancellationToken {
        self.0.clone()
    }
}
impl Drop for RecipientCancellation {
    fn drop(&mut self) {
        self.0.cancel();
    }
}
#[derive(Debug)]
pub(crate) enum GrantNotSent {
    AdmissionUnavailable,
    SourceUnavailable,
    RequestIdExhausted,
    RecipientCancelled,
    Custody(CustodyError),
    Invalid(RuntimeError),
    Full,
    Disconnected,
}
#[derive(Debug)]
pub(crate) enum GrantFailure {
    NotSent(GrantNotSent),
    Rejected(RuntimeError),
    Uncertain(GrantTicket),
    Retired(GrantTicket),
    Custody(CustodyError),
}
impl std::fmt::Display for GrantNotSent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AdmissionUnavailable => {
                f.write_str("native ownership/resource admission unavailable")
            }
            Self::SourceUnavailable => f.write_str("source unavailable"),
            Self::RequestIdExhausted => f.write_str("request IDs exhausted"),
            Self::RecipientCancelled => f.write_str("recipient cancelled before queue admission"),
            Self::Custody(error) => write!(f, "custody {error:?}"),
            Self::Invalid(error) => write!(f, "invalid command ({:?})", error.code),
            Self::Full => f.write_str("Runtime queue full"),
            Self::Disconnected => f.write_str("Runtime queue disconnected"),
        }
    }
}
impl std::fmt::Display for GrantFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotSent(reason)=>write!(f,"Ownership request not sent: {reason}."),
            Self::Rejected(error)=>write!(f,"Runtime rejected ownership without a new grant ({:?}).",error.code),
            Self::Uncertain(ticket)=>write!(f,"Ownership request {} is uncertain; original request remains in Service custody and must not be retried.",ticket.request().0),
            Self::Retired(ticket)=>write!(f,"Exact ownership request {} was retired/revoked.",ticket.request().0),
            Self::Custody(error)=>write!(f,"Ownership custody transition rejected: {error:?}."),
        }
    }
}
impl std::error::Error for GrantFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Rejected(error) | Self::NotSent(GrantNotSent::Invalid(error)) => Some(error),
            _ => None,
        }
    }
}
impl From<CustodyError> for GrantFailure {
    fn from(error: CustodyError) -> Self {
        Self::Custody(error)
    }
}
pub(crate) struct ControllerGrantOffer {
    ticket: GrantTicket,
    lease: Lease,
    guard: GrantGuard,
}
pub(crate) struct ObserverGrantOffer {
    ticket: GrantTicket,
    lease: ObserverLease,
    guard: GrantGuard,
}
impl ControllerGrantOffer {
    pub fn identity(&self) -> Identity {
        self.ticket().identity()
    }
    pub fn lease(&self) -> Lease {
        self.lease
    }
    pub fn ticket(&self) -> GrantTicket {
        self.ticket
    }
}
impl ObserverGrantOffer {
    pub fn identity(&self) -> Identity {
        self.ticket().identity()
    }
    pub fn lease(&self) -> ObserverLease {
        self.lease
    }
    pub fn ticket(&self) -> GrantTicket {
        self.ticket
    }
}
#[derive(Clone, Copy)]
pub(crate) enum GrantHolder {
    LegacyController,
    NativeObserver,
    ObserverMailbox,
    ManagerMailbox,
}
impl Service {
    pub(crate) fn execute_for_recipient(
        &mut self,
        request: crate::experiment::ExperimentRequest,
        controller: u64,
        cancel: CancellationToken,
    ) -> Result<crate::experiment::ExperimentResponse> {
        self.with_grant_recipient(cancel, |service| service.execute(request, controller))
    }
    pub(crate) fn with_grant_recipient<T>(
        &mut self,
        cancel: CancellationToken,
        operation: impl FnOnce(&mut Self) -> T,
    ) -> T {
        let previous = std::mem::replace(&mut self.grant_recipient, cancel);
        let result = operation(self);
        self.grant_recipient = previous;
        result
    }
    fn begin_ownership_request(
        &mut self,
        tab: TabId,
        kind: GrantKind,
        command: Command,
    ) -> std::result::Result<(GrantTicket, GrantGuard), GrantFailure> {
        self.assert_accepting()
            .map_err(|_| GrantFailure::NotSent(GrantNotSent::AdmissionUnavailable))?;
        if self.grant_recipient.is_cancelled() {
            return Err(GrantFailure::NotSent(GrantNotSent::RecipientCancelled));
        }
        let identity = self
            .terminals
            .get(&tab)
            .ok_or(GrantFailure::NotSent(GrantNotSent::SourceUnavailable))?
            .runtime
            .identity();
        let id = self
            .request_id()
            .map_err(|_| GrantFailure::NotSent(GrantNotSent::RequestIdExhausted))?;
        let (ticket, mut guard) = self
            .grants
            .reserve(identity, id, kind, self.grant_recipient.clone())
            .map_err(|error| GrantFailure::NotSent(GrantNotSent::Custody(error)))?;
        let submitted = self
            .terminals
            .get(&tab)
            .ok_or(GrantFailure::NotSent(GrantNotSent::SourceUnavailable))?
            .runtime
            .try_send(Request {
                id: ticket.request(),
                command,
            });
        if let Err(error) = submitted {
            self.grants.not_sent(ticket)?;
            guard.disarm();
            let reason = match error {
                shellfox_runtime::SubmitError::Invalid(error) => GrantNotSent::Invalid(error),
                shellfox_runtime::SubmitError::Full(_) => GrantNotSent::Full,
                shellfox_runtime::SubmitError::Disconnected(_) => GrantNotSent::Disconnected,
            };
            return Err(GrantFailure::NotSent(reason));
        }
        self.grants.sent(ticket)?;
        Ok((ticket, guard))
    }
    fn wait_ownership_request(
        &mut self,
        ticket: GrantTicket,
    ) -> std::result::Result<ActualGrant, GrantFailure> {
        let deadline = Instant::now() + Duration::from_secs(12);
        loop {
            if self.assert_owned().is_err() || self.drain(ticket.identity().tab_id).is_err() {
                return Err(GrantFailure::Uncertain(ticket));
            }
            if self.grants.recipient_cancelled(ticket)? {
                return Err(GrantFailure::Uncertain(ticket));
            }
            match self.grants.poll(ticket)? {
                GrantPoll::Granted(actual) => return Ok(actual),
                GrantPoll::Rejected(error) => {
                    self.grants.acknowledge_rejection(ticket)?;
                    return Err(GrantFailure::Rejected(error));
                }
                GrantPoll::Retired => return Err(GrantFailure::Retired(ticket)),
                GrantPoll::Retiring => return Err(GrantFailure::Uncertain(ticket)),
                GrantPoll::Pending => {}
            }
            if Instant::now() >= deadline {
                return Err(GrantFailure::Uncertain(ticket));
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    pub(crate) fn controller_grant(
        &mut self,
        tab: TabId,
        controller: u64,
        takeover: bool,
    ) -> std::result::Result<ControllerGrantOffer, GrantFailure> {
        let (ticket, guard) = self.begin_ownership_request(
            tab,
            GrantKind::Controller(controller),
            Command::Claim {
                controller,
                takeover,
            },
        )?;
        match self.wait_ownership_request(ticket)? {
            ActualGrant::Controller(lease) => Ok(ControllerGrantOffer {
                ticket,
                lease,
                guard,
            }),
            _ => Err(GrantFailure::Custody(CustodyError::WrongReply)),
        }
    }
    pub(crate) fn observer_grant(
        &mut self,
        tab: TabId,
        viewer: u64,
    ) -> std::result::Result<ObserverGrantOffer, GrantFailure> {
        let (ticket, guard) = self.begin_ownership_request(
            tab,
            GrantKind::Observer(viewer),
            Command::Observe {
                viewer,
                after: None,
            },
        )?;
        match self.wait_ownership_request(ticket)? {
            ActualGrant::Observer(lease) => Ok(ObserverGrantOffer {
                ticket,
                lease,
                guard,
            }),
            _ => Err(GrantFailure::Custody(CustodyError::WrongReply)),
        }
    }
    pub(crate) fn adopt_controller_grant(
        &mut self,
        mut offer: ControllerGrantOffer,
        holder: GrantHolder,
    ) -> Result<Attachment> {
        self.assert_accepting()?;
        let identity = offer.identity();
        let lease = offer.lease;
        let terminal = self
            .terminals
            .get_mut(&identity.tab_id)
            .filter(|terminal| terminal.runtime.identity() == identity)
            .context("Controller source owner is not current.")?;
        let book = match holder {
            GrantHolder::LegacyController => None,
            GrantHolder::ManagerMailbox => Some(
                self.manager_v2_mailboxes
                    .prepare_adoption(identity, crate::manager_delivery::Role::Controller(lease))
                    .context(
                        "Current provisional manager book does not own the exact controller grant.",
                    )?,
            ),
            _ => anyhow::bail!("Controller grant requires a real controller custody book."),
        };
        self.grants
            .adopt(offer.ticket, ActualGrant::Controller(lease))
            .map_err(GrantFailure::from)?;
        // No lookup/allocation/fallible work after custody transfer.
        if let Some(book) = book {
            book.commit();
        }
        terminal.install_lease(lease);
        offer.guard.disarm();
        Ok(Attachment {
            controller: lease.controller,
            epoch: lease.epoch,
            generation: identity.generation,
        })
    }
    pub(crate) fn adopt_observer_grant(
        &mut self,
        mut offer: ObserverGrantOffer,
        holder: GrantHolder,
    ) -> Result<ObserverLease> {
        self.assert_accepting()?;
        let identity = offer.identity();
        let lease = offer.lease;
        let book = match holder {
            GrantHolder::NativeObserver => {
                if !self.has_native_observer_grant(identity, lease) {
                    anyhow::bail!("Current native view does not own the exact observer grant.");
                }
                None
            }
            GrantHolder::ObserverMailbox => Some(
                self.remote_v2_mailboxes
                    .prepare_adoption(identity, lease)
                    .context(
                        "Current provisional observer book does not own the exact observer grant.",
                    )?,
            ),
            GrantHolder::ManagerMailbox => Some(
                self.manager_v2_mailboxes
                    .prepare_adoption(identity, crate::manager_delivery::Role::Observer(lease))
                    .context(
                        "Current provisional manager book does not own the exact observer grant.",
                    )?,
            ),
            _ => anyhow::bail!("Observer grant requires a real observer custody book."),
        };
        self.grants
            .adopt(offer.ticket, ActualGrant::Observer(lease))
            .map_err(GrantFailure::from)?;
        if let Some(book) = book {
            book.commit();
        }
        offer.guard.disarm();
        Ok(lease)
    }
}
