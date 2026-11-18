//! Only the existing Service drain feeds these bounded remote observer outboxes.
use super::delivery::{DeliveryFailure, ObserverDelivery};
use shellfox_core::TabId;
use shellfox_runtime::{Event, Identity, ObserverLease, RequestId, Response};
use std::collections::HashMap;
const MAX_MAILBOXES: usize = 128;
use crate::mailbox_custody::{MailboxAdoption, MailboxCustody};
struct Mailbox {
    delivery: ObserverDelivery,
    custody: MailboxCustody,
}
/// Registry/custody only, never authorization. Requests first validate canonical
/// Binding, current approval/profile and the native source-policy epoch.
#[derive(Default)]
pub struct ObserverMailboxes {
    entries: HashMap<(TabId, u64), Mailbox>,
    retiring: HashMap<u64, (Identity, ObserverLease)>,
}
impl ObserverMailboxes {
    pub fn can_insert(&self) -> bool {
        self.entries.len() < MAX_MAILBOXES
    }
    pub fn revoke_delivery(&mut self, identity: Identity, observer: ObserverLease) {
        if let Some(entry) = self.get_mut(identity, observer) {
            entry.revoke();
        }
    }
    /// Purge all queued bytes and recovery tokens BEFORE scheduling Unobserve.
    pub fn revoke_source(&mut self, identity: Identity) {
        for entry in self
            .entries
            .values_mut()
            .filter(|entry| entry.delivery.identity() == identity)
        {
            entry.delivery.revoke();
        }
    }
    pub fn retirement_candidates(&self) -> Vec<(Identity, ObserverLease)> {
        self.entries
            .values()
            .filter(|entry| entry.custody == MailboxCustody::Adopted && entry.delivery.is_revoked())
            .map(|entry| (entry.delivery.identity(), entry.delivery.observer()))
            .filter(|target| !self.retiring.values().any(|value| value == target))
            .collect()
    }
    pub fn retirement_sent(
        &mut self,
        identity: Identity,
        observer: ObserverLease,
        request: RequestId,
    ) {
        if self
            .entries
            .get(&(identity.tab_id, observer.viewer))
            .is_some_and(|entry| {
                entry.custody == MailboxCustody::Adopted
                    && entry.delivery.identity() == identity
                    && entry.delivery.observer() == observer
                    && entry.delivery.is_revoked()
            })
        {
            self.retiring.insert(request.0, (identity, observer));
        }
    }
    pub fn insert_offer(&mut self, delivery: ObserverDelivery) -> Result<(), DeliveryFailure> {
        let key = (delivery.identity().tab_id, delivery.observer().viewer);
        if self.entries.contains_key(&key) {
            return Err(DeliveryFailure::Stale);
        }
        if !self.can_insert() {
            return Err(DeliveryFailure::Invalid);
        }
        self.entries.insert(
            key,
            Mailbox {
                delivery,
                custody: MailboxCustody::Provisional,
            },
        );
        Ok(())
    }
    pub fn get_mut(
        &mut self,
        identity: Identity,
        observer: ObserverLease,
    ) -> Option<&mut ObserverDelivery> {
        self.entries
            .get_mut(&(identity.tab_id, observer.viewer))
            .map(|entry| &mut entry.delivery)
            .filter(|entry| entry.identity() == identity && entry.observer() == observer)
    }
    pub(crate) fn prepare_adoption(
        &mut self,
        identity: Identity,
        observer: ObserverLease,
    ) -> Option<MailboxAdoption<'_>> {
        let entry = self.entries.get_mut(&(identity.tab_id, observer.viewer))?;
        if entry.delivery.identity() != identity
            || entry.delivery.observer() != observer
            || entry.delivery.is_revoked()
        {
            return None;
        }
        MailboxAdoption::provisional(&mut entry.custody)
    }
    pub fn discard_unadopted(
        &mut self,
        identity: Identity,
        observer: ObserverLease,
    ) -> Result<(), DeliveryFailure> {
        let entry = self
            .entries
            .get(&(identity.tab_id, observer.viewer))
            .ok_or(DeliveryFailure::Stale)?;
        if entry.custody != MailboxCustody::Provisional
            || entry.delivery.identity() != identity
            || entry.delivery.observer() != observer
        {
            return Err(DeliveryFailure::Stale);
        }
        self.remove_confirmed(identity, observer);
        Ok(())
    }
    pub fn source_destroyed(&mut self, identity: Identity) {
        let observers = self
            .entries
            .values()
            .filter(|entry| entry.delivery.identity() == identity)
            .map(|entry| entry.delivery.observer())
            .collect::<Vec<_>>();
        for observer in observers {
            self.remove_confirmed(identity, observer);
        }
    }
    fn remove_confirmed(
        &mut self,
        identity: Identity,
        observer: ObserverLease,
    ) -> Option<ObserverDelivery> {
        let key = (identity.tab_id, observer.viewer);
        self.retiring
            .retain(|_, value| *value != (identity, observer));
        self.get_mut(identity, observer)?;
        self.entries.remove(&key).map(|entry| entry.delivery)
    }
    /// Matched chunks MOVE. Unmatched Native/controller/reply/activity events
    /// return unchanged to the SAME consumer. Revoked bytes never reenter a book.
    pub fn record(&mut self, identity: Identity, event: Event) -> Option<Event> {
        if let Event::Reply { id, result } = &event {
            if let Some((expected, observer)) = self.retiring.remove(&id.0) {
                if expected == identity
                    && (matches!(result, Ok(Response::Applied))
                        || matches!(result, Err(error)
                    if matches!(error.code, shellfox_runtime::ErrorCode::ControlRequired | shellfox_runtime::ErrorCode::Closed)))
                {
                    self.remove_confirmed(identity, observer);
                }
            }
            return Some(event);
        }
        let observer = match &event {
            Event::ObserverOutput { observer, .. }
            | Event::ObserverLayout { observer, .. }
            | Event::ObserverReplayLost { observer, .. }
            | Event::ObserverExited { observer, .. } => *observer,
            _ => return Some(event),
        };
        match self.get_mut(identity, observer) {
            Some(entry) => entry.record(identity, event),
            None => Some(event),
        }
    }
}
