use super::*;
use shellfox_runtime::{ControllerRevocation, Event};
impl ManagerDelivery {
    pub fn record(&mut self, identity: Identity, event: Event) -> Option<Event> {
        if identity != self.identity {
            return Some(event);
        }
        let matched = match &event {
            Event::Output { lease, .. }
            | Event::ReplayLost { lease, .. }
            | Event::Exited { lease, .. }
            | Event::ControllerLayout { lease, .. }
            | Event::ControllerRevoked { lease, .. } => self.role == Role::Controller(*lease),
            Event::ObserverOutput { observer, .. }
            | Event::ObserverLayout { observer, .. }
            | Event::ObserverReplayLost { observer, .. }
            | Event::ObserverExited { observer, .. } => self.role == Role::Observer(*observer),
            _ => false,
        };
        if !matched {
            return Some(event);
        }
        if self.revoked() {
            return None;
        }
        match event {
            Event::Output { chunk, .. } | Event::ObserverOutput { chunk, .. } => {
                if self.lost {
                    return None;
                }
                if self.received.checked_add(1) != Some(chunk.sequence)
                    || chunk.sequence > shellfox_protocol::MAX_SAFE_INTEGER
                    || chunk.bytes.is_empty()
                    || chunk.bytes.len() > wire::MAX_DATA_BYTES
                    || self.pending.len() >= 256
                    || self.bytes + chunk.bytes.len() > 128 * 1024
                {
                    self.invalidate();
                    return None;
                }
                self.received = chunk.sequence;
                self.bytes += chunk.bytes.len();
                self.pending.push_back(chunk);
            }
            Event::ControllerLayout { layout, .. } | Event::ObserverLayout { layout, .. } => {
                let prior = self
                    .recovery
                    .or(self.accepted)
                    .map_or(0, |stamp| stamp.revision);
                if layout.layout_revision > prior
                    && self
                        .layout_pending
                        .is_none_or(|old| layout.layout_revision > old.layout_revision)
                {
                    self.invalidate();
                    self.layout_pending = Some(layout);
                }
            }
            Event::ControllerRevoked { reason, .. } => self.revoke(match reason {
                ControllerRevocation::Replaced => wire::RevokedReason::ControlLost,
                ControllerRevocation::Released => wire::RevokedReason::ControlReleased,
            }),
            Event::ReplayLost { .. } | Event::ObserverReplayLost { .. } => self.invalidate(),
            Event::Exited { last_sequence, .. } | Event::ObserverExited { last_sequence, .. } => {
                if last_sequence == self.received {
                    self.ended = Some(last_sequence);
                } else {
                    self.invalidate();
                }
            }
            _ => return Some(event),
        }
        None
    }
}
