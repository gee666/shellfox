//! Canonical geometry journal and full-recovery consumption fence.
use crate::{layout_api::*, *};
use std::{
    collections::VecDeque,
    sync::atomic::{AtomicU64, Ordering},
};
#[path = "layout_controller.rs"]
mod controller;
const CHANGES: usize = 64;
static NEXT_DOMAIN: AtomicU64 = AtomicU64::new(1);
struct View {
    lease: PresentationLease,
    revision: u64,
    recovery: Option<RecoveryStamp>,
    accepted: Option<RecoveryStamp>,
    needs_recovery: bool,
    loss_reported: bool,
}
#[derive(Clone, Copy)]
struct RecoveryStamp {
    id: RecoveryId,
    sequence: u64,
    revision: u64,
}
pub(crate) enum LayoutNotice {
    Changed(LayoutStamp),
    Lost(u64),
}
pub(crate) struct Layouts {
    current: LayoutStamp,
    changes: VecDeque<LayoutStamp>,
    views: Vec<View>,
    domain: Option<u64>,
    next_recovery: u64,
}
impl Layouts {
    pub(crate) fn new(cols: u16, rows: u16) -> Self {
        Self {
            current: LayoutStamp {
                cols,
                rows,
                layout_revision: 1,
                last_delivery_sequence: 0,
            },
            changes: VecDeque::new(),
            views: Vec::new(),
            domain: NEXT_DOMAIN
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
                .ok(),
            next_recovery: 0,
        }
    }
    pub(crate) fn validate_change(&self, cols: u16, rows: u16) -> Result<(), RuntimeError> {
        if (cols, rows) != (self.current.cols, self.current.rows)
            && self.current.layout_revision == u64::MAX
        {
            return Err(RuntimeError::validation("Layout revision exhausted."));
        }
        Ok(())
    }
    pub(crate) fn observers(&self) -> Vec<ObserverLease> {
        self.views
            .iter()
            .filter_map(|view| match view.lease {
                PresentationLease::Observer(observer) => Some(observer),
                _ => None,
            })
            .collect()
    }
    pub(crate) fn changed(
        &mut self,
        cols: u16,
        rows: u16,
        sequence: u64,
    ) -> Result<(), RuntimeError> {
        if (cols, rows) == (self.current.cols, self.current.rows) {
            return Ok(());
        }
        let revision = self
            .current
            .layout_revision
            .checked_add(1)
            .ok_or_else(|| RuntimeError::validation("Layout revision exhausted."))?;
        self.current = LayoutStamp {
            cols,
            rows,
            layout_revision: revision,
            last_delivery_sequence: sequence,
        };
        self.changes.push_back(self.current);
        if self.changes.len() > CHANGES {
            self.changes.pop_front();
        }
        Ok(())
    }
    pub(crate) fn observer_changed(&mut self, observer: ObserverLease) {
        self.views.retain(|view| !matches!(view.lease, PresentationLease::Observer(old) if old.viewer == observer.viewer && old != observer));
    }
    pub(crate) fn remove(&mut self, observer: ObserverLease) {
        self.views
            .retain(|view| view.lease != PresentationLease::Observer(observer));
    }
    pub(crate) fn recovery(
        &mut self,
        observer: ObserverLease,
        bytes: Vec<u8>,
        capability: AnsiRecoveryCapability,
        sequence: u64,
    ) -> Result<StampedRecovery, RuntimeError> {
        self.recovery_for(
            PresentationLease::Observer(observer),
            bytes,
            capability,
            sequence,
        )
    }
    fn recovery_for(
        &mut self,
        lease: PresentationLease,
        bytes: Vec<u8>,
        capability: AnsiRecoveryCapability,
        sequence: u64,
    ) -> Result<StampedRecovery, RuntimeError> {
        let cut = self.stamp_for(lease, sequence)?;
        Ok(StampedRecovery {
            bytes,
            capability,
            last_delivery_sequence: sequence,
            cols: cut.cols,
            rows: cut.rows,
            layout_revision: cut.layout_revision,
            recovery_id: cut.recovery_id,
        })
    }
    fn stamp_for(
        &mut self,
        lease: PresentationLease,
        sequence: u64,
    ) -> Result<RecoveryCut, RuntimeError> {
        if !self.views.iter().any(|view| view.lease == lease)
            && self
                .views
                .iter()
                .filter(|view| matches!(view.lease, PresentationLease::Observer(_)))
                .count()
                >= 16
            && matches!(lease, PresentationLease::Observer(_))
        {
            return Err(RuntimeError::validation(
                "At most 16 stamped observers may attach.",
            ));
        }
        let domain = self
            .domain
            .ok_or_else(|| RuntimeError::validation("Recovery domains exhausted."))?;
        let id = self
            .next_recovery
            .checked_add(1)
            .ok_or_else(|| RuntimeError::validation("Recovery ID exhausted."))?;
        self.next_recovery = id;
        let stamp = RecoveryStamp {
            id: RecoveryId { domain, value: id },
            sequence,
            revision: self.current.layout_revision,
        };
        if let Some(view) = self.views.iter_mut().find(|view| view.lease == lease) {
            view.recovery = Some(stamp);
            view.needs_recovery = true;
            view.loss_reported = false;
        } else {
            self.views.push(View {
                lease,
                revision: stamp.revision,
                recovery: Some(stamp),
                accepted: None,
                needs_recovery: true,
                loss_reported: false,
            });
        }
        Ok(RecoveryCut {
            recovery_id: stamp.id,
            last_delivery_sequence: sequence,
            cols: self.current.cols,
            rows: self.current.rows,
            layout_revision: stamp.revision,
        })
    }
    #[cfg(feature = "experimental-shellfox-native")]
    pub(crate) fn native0_recovery(
        &mut self,
        observer: ObserverLease,
        sequence: u64,
    ) -> Result<RecoveryCut, RuntimeError> {
        self.stamp_for(PresentationLease::Observer(observer), sequence)
    }
    pub(crate) fn validate_ack(
        &self,
        observer: ObserverLease,
        id: RecoveryId,
        sequence: u64,
        revision: u64,
    ) -> Result<(), RuntimeError> {
        self.validate_ack_for(
            PresentationLease::Observer(observer),
            id,
            sequence,
            revision,
        )
    }
    fn validate_ack_for(
        &self,
        lease: PresentationLease,
        id: RecoveryId,
        sequence: u64,
        revision: u64,
    ) -> Result<(), RuntimeError> {
        let stamp = self
            .views
            .iter()
            .find(|view| view.lease == lease)
            .and_then(|view| view.recovery)
            .ok_or_else(|| {
                RuntimeError::validation("Only a pending full recovery can be acknowledged.")
            })?;
        if stamp.id != id || stamp.sequence != sequence || stamp.revision != revision {
            return Err(RuntimeError::validation(
                "Recovery ACK does not match the latest snapshot/sequence/layout.",
            ));
        }
        Ok(())
    }
    pub(crate) fn accepted(&mut self, observer: ObserverLease) {
        self.accepted_for(PresentationLease::Observer(observer));
    }
    fn accepted_for(&mut self, lease: PresentationLease) {
        if let Some(view) = self.views.iter_mut().find(|view| view.lease == lease) {
            if let Some(stamp) = view.recovery.take() {
                view.revision = stamp.revision;
                view.accepted = Some(stamp);
                view.needs_recovery = false;
                view.loss_reported = false;
            }
        }
    }
    pub(crate) fn data_ack_allowed(&self, observer: ObserverLease) -> bool {
        !self.views.iter().any(|view| {
            view.lease == PresentationLease::Observer(observer) && view.recovery.is_some()
        })
    }
    pub(crate) fn allows(
        &mut self,
        observer: ObserverLease,
        next: u64,
        current_sequence: u64,
        notify: impl FnMut(LayoutNotice) -> bool,
    ) -> bool {
        self.allows_for(
            PresentationLease::Observer(observer),
            next,
            current_sequence,
            notify,
        )
    }
    /// A notification is not a grant. Only an exact full-recovery ACK releases it.
    fn allows_for(
        &mut self,
        lease: PresentationLease,
        next: u64,
        current_sequence: u64,
        mut notify: impl FnMut(LayoutNotice) -> bool,
    ) -> bool {
        let Some(view) = self.views.iter_mut().find(|view| view.lease == lease) else {
            return true;
        };
        if view.recovery.is_some() || view.needs_recovery {
            return false;
        }
        if self
            .changes
            .front()
            .is_some_and(|first| view.revision < first.layout_revision - 1)
        {
            if !view.loss_reported && notify(LayoutNotice::Lost(current_sequence)) {
                view.loss_reported = true;
                view.needs_recovery = true;
            }
            return false;
        }
        if let Some(layout) = self
            .changes
            .iter()
            .find(|layout| layout.layout_revision > view.revision)
        {
            if next > layout.last_delivery_sequence {
                if notify(LayoutNotice::Changed(*layout)) {
                    view.needs_recovery = true;
                }
                return false;
            }
        }
        true
    }
}
#[cfg(test)]
#[path = "layout_tests.rs"]
mod tests;
