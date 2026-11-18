use super::manager_delivery::{ManagerDelivery, Role};
use crate::mailbox_custody::{MailboxAdoption, MailboxCustody};
use shellfox_protocol::manager_v2 as wire;
use shellfox_runtime::{Event, Identity, RequestId, Response};
use std::collections::HashMap;
struct Mailbox {
    delivery: ManagerDelivery,
    custody: MailboxCustody,
}
type Key = (shellfox_core::TabId, bool, u64);
fn key(identity: Identity, role: Role) -> Key {
    match role {
        Role::Controller(lease) => (identity.tab_id, true, lease.controller),
        Role::Observer(lease) => (identity.tab_id, false, lease.viewer),
    }
}
#[derive(Default)]
pub(crate) struct ManagerMailboxes {
    entries: HashMap<Key, Mailbox>,
    pending: HashMap<u64, (Identity, Role)>,
    revoked: HashMap<wire::Id, (wire::DeliveryRef, wire::RevokedReason)>,
    notices: std::collections::HashSet<wire::Id>,
}
impl ManagerMailboxes {
    pub fn can_insert(&self) -> bool {
        self.entries.len() < 128 && self.notices.len() < 128
    }
    pub fn insert_offer(
        &mut self,
        entry: ManagerDelivery,
    ) -> Result<(), super::manager_delivery::Failure> {
        let key = key(entry.identity(), entry.role());
        if !self.can_insert() || self.entries.contains_key(&key) {
            return Err(super::manager_delivery::Failure::Bounds);
        }
        self.notices.insert(entry.reference().delivery_id);
        self.entries.insert(
            key,
            Mailbox {
                delivery: entry,
                custody: MailboxCustody::Provisional,
            },
        );
        Ok(())
    }
    pub fn get(&mut self, identity: Identity, role: Role) -> Option<&mut ManagerDelivery> {
        self.entries
            .get_mut(&key(identity, role))
            .map(|entry| &mut entry.delivery)
            .filter(|entry| entry.identity() == identity && entry.role() == role)
    }
    pub fn revoke(&mut self, identity: Identity, role: Role, reason: wire::RevokedReason) {
        if let Some(entry) = self.get(identity, role) {
            entry.revoke(reason);
            let reference = entry.reference().clone();
            if self.notices.contains(&reference.delivery_id) {
                self.revoked
                    .insert(reference.delivery_id, (reference, reason));
            }
        }
    }
    pub fn revocation(&self, reference: &wire::DeliveryRef) -> Option<wire::RevokedReason> {
        self.revoked
            .get(&reference.delivery_id)
            .filter(|(current, _)| current == reference)
            .map(|(_, reason)| *reason)
    }
    pub fn forget_notice(&mut self, reference: &wire::DeliveryRef) {
        self.revoked.remove(&reference.delivery_id);
        self.notices.remove(&reference.delivery_id);
    }
    pub fn revoke_controllers(&mut self, identity: Identity) {
        let roles = self
            .entries
            .values()
            .filter(|entry| {
                entry.delivery.identity() == identity
                    && matches!(entry.delivery.role(), Role::Controller(_))
            })
            .map(|entry| entry.delivery.role())
            .collect::<Vec<_>>();
        for role in roles {
            self.revoke(identity, role, wire::RevokedReason::ControlLost);
        }
    }
    pub fn revoke_source(&mut self, identity: Identity) {
        let roles = self
            .entries
            .values()
            .filter(|entry| entry.delivery.identity() == identity)
            .map(|entry| entry.delivery.role())
            .collect::<Vec<_>>();
        for role in roles {
            self.revoke(identity, role, wire::RevokedReason::PolicyChanged);
        }
    }
    pub fn retirements(&self) -> Vec<(Identity, Role)> {
        self.entries
            .values()
            .filter(|entry| entry.custody == MailboxCustody::Adopted && entry.delivery.revoked())
            .map(|entry| (entry.delivery.identity(), entry.delivery.role()))
            .filter(|target| !self.pending.values().any(|value| value == target))
            .collect()
    }
    pub fn sent(&mut self, id: RequestId, identity: Identity, role: Role) {
        if self.entries.get(&key(identity, role)).is_some_and(|entry| {
            entry.custody == MailboxCustody::Adopted
                && entry.delivery.identity() == identity
                && entry.delivery.role() == role
                && entry.delivery.revoked()
        }) {
            self.pending.insert(id.0, (identity, role));
        }
    }
    pub fn prepare_adoption(
        &mut self,
        identity: Identity,
        role: Role,
    ) -> Option<MailboxAdoption<'_>> {
        let entry = self.entries.get_mut(&key(identity, role))?;
        if entry.delivery.identity() != identity
            || entry.delivery.role() != role
            || entry.delivery.revoked()
        {
            return None;
        }
        MailboxAdoption::provisional(&mut entry.custody)
    }
    pub fn discard_unadopted(
        &mut self,
        identity: Identity,
        role: Role,
    ) -> Result<(), super::manager_delivery::Failure> {
        let entry = self
            .entries
            .get(&key(identity, role))
            .ok_or(super::manager_delivery::Failure::Stale)?;
        if entry.custody != MailboxCustody::Provisional
            || entry.delivery.identity() != identity
            || entry.delivery.role() != role
        {
            return Err(super::manager_delivery::Failure::Stale);
        }
        let reference = entry.delivery.reference().clone();
        self.forget_notice(&reference);
        self.remove_confirmed(identity, role);
        Ok(())
    }
    pub fn source_destroyed(&mut self, identity: Identity) {
        let roles = self
            .entries
            .values()
            .filter(|entry| entry.delivery.identity() == identity)
            .map(|entry| entry.delivery.role())
            .collect::<Vec<_>>();
        for role in roles {
            self.remove_confirmed(identity, role);
        }
    }
    fn remove_confirmed(&mut self, identity: Identity, role: Role) {
        let key = key(identity, role);
        if self.entries.get(&key).is_some_and(|entry| {
            entry.delivery.identity() == identity && entry.delivery.role() == role
        }) {
            self.entries.remove(&key);
        }
        self.pending.retain(|_, target| *target != (identity, role));
    }
    pub fn record(&mut self, identity: Identity, event: Event) -> Option<Event> {
        if let Event::Reply { id, result } = &event {
            if let Some((_, role)) = self
                .pending
                .get(&id.0)
                .copied()
                .filter(|(expected, _)| *expected == identity)
            {
                self.pending.remove(&id.0);
                if matches!(result, Ok(Response::Applied))
                    || matches!(result,Err(error)if matches!(error.code,shellfox_runtime::ErrorCode::ControlRequired|shellfox_runtime::ErrorCode::Closed))
                {
                    self.remove_confirmed(identity, role);
                }
                return None;
            }
            return Some(event);
        }
        let role = match &event {
            Event::Output { lease, .. }
            | Event::ReplayLost { lease, .. }
            | Event::Exited { lease, .. }
            | Event::ControllerLayout { lease, .. }
            | Event::ControllerRevoked { lease, .. } => Role::Controller(*lease),
            Event::ObserverOutput { observer, .. }
            | Event::ObserverReplayLost { observer, .. }
            | Event::ObserverExited { observer, .. }
            | Event::ObserverLayout { observer, .. } => Role::Observer(*observer),
            _ => return Some(event),
        };
        match self.get(identity, role) {
            Some(entry) => {
                let result = entry.record(identity, event);
                let reference = entry.reference().clone();
                let notice = entry.revoked_reason();
                if let Some(reason) = notice {
                    if self.notices.contains(&reference.delivery_id) {
                        self.revoked
                            .insert(reference.delivery_id, (reference, reason));
                    }
                }
                result
            }
            None => Some(event),
        }
    }
}
