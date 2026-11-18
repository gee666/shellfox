//! A remote delivery outbox for one actual Runtime observer. No input/control API.
#[path = "remote_v2_ack.rs"]
mod ack;
use shellfox_protocol::{observer_v2 as wire, Validate};
use shellfox_runtime::{
    AnsiRecoveryCapability, Chunk, Event, Identity, LayoutStamp, ObserverLease, RecoveryId,
    Response,
};
use std::collections::VecDeque;
const MAX_PENDING_BYTES: usize = 128 * 1024;
const MAX_PENDING_CHUNKS: usize = 256;
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum DeliveryFailure {
    Invalid,
    Stale,
    RecoveryRequired,
    RecoveryUnavailable,
    GeometryUnsupported,
}
#[derive(Clone, Copy)]
struct PendingRecovery {
    wire_id: wire::Id,
    runtime_id: RecoveryId,
    sequence: u64,
    revision: u64,
}
/// Owner-local retention, not a wire grant or another parser/journal. Takes
/// ownership of Runtime chunks without cloning. Every request still requires
/// canonical connection binding, profile and current approval validation.
pub struct ObserverDelivery {
    identity: Identity,
    observer: ObserverLease,
    delivery: wire::DeliveryRef,
    pending: VecDeque<Chunk>,
    pending_bytes: usize,
    received: u64,
    offered: u64,
    acknowledged: u64,
    recovery: Option<PendingRecovery>,
    layout_revision: u64,
    layout_pending: Option<LayoutStamp>,
    lost: bool,
    revoked: bool,
    ended: Option<u64>,
}
impl ObserverDelivery {
    pub fn new(identity: Identity, observer: ObserverLease) -> Result<Self, DeliveryFailure> {
        let delivery = wire::DeliveryRef {
            tab_id: identity
                .tab_id
                .to_string()
                .parse()
                .map_err(|_| DeliveryFailure::Invalid)?,
            generation: identity
                .generation
                .to_string()
                .parse()
                .map_err(|_| DeliveryFailure::Invalid)?,
            delivery_id: wire::Id::new(),
            delivery_epoch: observer.epoch,
        };
        delivery.validate().map_err(|_| DeliveryFailure::Invalid)?;
        if observer.viewer == 0 {
            return Err(DeliveryFailure::Invalid);
        }
        Ok(Self {
            identity,
            observer,
            delivery,
            pending: VecDeque::new(),
            pending_bytes: 0,
            received: 0,
            offered: 0,
            acknowledged: 0,
            recovery: None,
            layout_revision: 0,
            layout_pending: None,
            lost: true,
            revoked: false,
            ended: None,
        })
    }
    pub fn reference(&self) -> &wire::DeliveryRef {
        &self.delivery
    }
    pub fn observer(&self) -> ObserverLease {
        self.observer
    }
    pub fn identity(&self) -> Identity {
        self.identity
    }
    /// Only stamped Runtime recovery supplies bytes, geometry and its opaque
    /// correlation atomically. Frame versions/default geometry cannot enter here.
    pub fn recover(&mut self, response: Response) -> Result<wire::Recovery, DeliveryFailure> {
        if self.revoked {
            return Err(DeliveryFailure::Stale);
        }
        let Response::Recovery(stamp) = response else {
            return Err(DeliveryFailure::RecoveryUnavailable);
        };
        if stamp.capability != AnsiRecoveryCapability::FormattedActiveBufferV1
            || stamp.bytes.is_empty()
            || stamp.bytes.len() > wire::MAX_RECOVERY_BYTES
        {
            return Err(DeliveryFailure::RecoveryUnavailable);
        }
        if !(2..=500).contains(&stamp.cols) || !(2..=500).contains(&stamp.rows) {
            return Err(DeliveryFailure::GeometryUnsupported);
        }
        if stamp.layout_revision < self.layout_revision
            || self.layout_pending.is_some_and(|newer| {
                newer.layout_revision > stamp.layout_revision
                    || newer.last_delivery_sequence > stamp.last_delivery_sequence
            })
        {
            return Err(DeliveryFailure::RecoveryRequired);
        }
        let recovery = wire::Recovery {
            recovery_id: wire::Id::new(),
            format: wire::RecoveryFormat::FormattedActiveBuffer,
            encoding: wire::Encoding::Base64,
            data: base64(&stamp.bytes)?,
            last_sequence: stamp.last_delivery_sequence,
            cols: stamp.cols,
            rows: stamp.rows,
            layout_revision: stamp.layout_revision,
        };
        recovery.validate().map_err(|_| DeliveryFailure::Invalid)?;
        self.pending.clear();
        self.pending_bytes = 0;
        self.received = stamp.last_delivery_sequence;
        self.offered = stamp.last_delivery_sequence;
        self.acknowledged = stamp.last_delivery_sequence;
        self.recovery = Some(PendingRecovery {
            wire_id: recovery.recovery_id,
            runtime_id: stamp.recovery_id,
            sequence: stamp.last_delivery_sequence,
            revision: stamp.layout_revision,
        });
        self.layout_revision = stamp.layout_revision;
        self.layout_pending = None;
        self.lost = false;
        self.ended = self
            .ended
            .filter(|ended| *ended == stamp.last_delivery_sequence);
        Ok(recovery)
    }
    /// The existing Service drain is the only Runtime consumer. Matched payloads
    /// move here; all unrelated Native/controller/reply events return unchanged.
    pub fn record(&mut self, identity: Identity, event: Event) -> Option<Event> {
        if identity != self.identity {
            return Some(event);
        }
        if self.revoked {
            return match &event {
                Event::ObserverOutput { observer, .. }
                | Event::ObserverLayout { observer, .. }
                | Event::ObserverReplayLost { observer, .. }
                | Event::ObserverExited { observer, .. }
                    if *observer == self.observer =>
                {
                    None
                }
                _ => Some(event),
            };
        }
        match event {
            Event::ObserverOutput { observer, chunk } if observer == self.observer => {
                if self.lost {
                    return None;
                }
                let valid = self.received.checked_add(1) == Some(chunk.sequence)
                    && !chunk.bytes.is_empty()
                    && chunk.bytes.len() <= wire::MAX_DATA_BYTES
                    && chunk.sequence <= shellfox_protocol::MAX_SAFE_INTEGER
                    && self.pending.len() < MAX_PENDING_CHUNKS
                    && self
                        .pending_bytes
                        .checked_add(chunk.bytes.len())
                        .is_some_and(|v| v <= MAX_PENDING_BYTES);
                if !valid {
                    self.mark_lost();
                    return None;
                }
                self.received = chunk.sequence;
                self.pending_bytes += chunk.bytes.len();
                self.pending.push_back(chunk);
            }
            Event::ObserverLayout { observer, layout } if observer == self.observer => {
                if layout.layout_revision > self.layout_revision
                    && self
                        .layout_pending
                        .is_none_or(|old| layout.layout_revision > old.layout_revision)
                {
                    self.mark_lost();
                    self.layout_pending = Some(layout);
                }
            }
            Event::ObserverReplayLost { observer, .. } if observer == self.observer => {
                self.mark_lost()
            }
            Event::ObserverExited {
                observer,
                last_sequence,
            } if observer == self.observer => {
                if last_sequence == self.received
                    && last_sequence <= shellfox_protocol::MAX_SAFE_INTEGER
                {
                    self.ended = Some(last_sequence);
                } else {
                    self.mark_lost();
                }
            }
            unmatched => return Some(unmatched),
        }
        None
    }
    /// Revoke custody BEFORE Runtime Unobserve is queued. This cannot be undone
    /// by replay, recovery, a late callback or an old same-sequence ACK.
    pub fn revoke(&mut self) {
        self.revoked = true;
        self.mark_lost();
        self.layout_pending = None;
    }
    pub fn is_revoked(&self) -> bool {
        self.revoked
    }
    fn mark_lost(&mut self) {
        self.lost = true;
        self.pending.clear();
        self.pending_bytes = 0;
        self.ended = None;
        self.recovery = None;
    }
    /// Retain offered bytes until an actual ACK succeeds. Poll cancellation/loss
    /// never acknowledges them. Repeated polls may repeat unconsumed sequences.
    pub fn poll(
        &mut self,
        byte_budget: usize,
        event_budget: usize,
    ) -> Result<Vec<wire::Event>, DeliveryFailure> {
        if self.revoked {
            return Err(DeliveryFailure::Stale);
        }
        if let Some(layout) = self.layout_pending {
            if event_budget == 0 {
                return Ok(Vec::new());
            }
            let event = wire::Event::Layout {
                delivery: self.delivery.clone(),
                layout_revision: layout.layout_revision,
                cols: layout.cols,
                rows: layout.rows,
                recovery_required: true,
            };
            event.validate().map_err(|_| DeliveryFailure::Invalid)?;
            return Ok(vec![event]);
        }
        if self.lost {
            return Err(DeliveryFailure::RecoveryRequired);
        }
        if self.recovery.is_some() {
            return Ok(Vec::new());
        }
        let mut events = Vec::new();
        let mut bytes = 0;
        let mut included = self.acknowledged;
        let budget = byte_budget.min(wire::MAX_POLL_DATA_BYTES);
        for chunk in &self.pending {
            if events.len() >= event_budget.min(1024) || bytes + chunk.bytes.len() > budget {
                break;
            }
            events.push(wire::Event::Data {
                delivery: self.delivery.clone(),
                sequence: chunk.sequence,
                encoding: wire::Encoding::Base64,
                data: base64(&chunk.bytes)?,
            });
            bytes += chunk.bytes.len();
            included = chunk.sequence;
            self.offered = self.offered.max(chunk.sequence);
        }
        if let Some(last_sequence) = self.ended {
            if included >= last_sequence && events.len() < event_budget.min(1024) {
                events.push(wire::Event::SourceEnded {
                    delivery: self.delivery.clone(),
                    last_sequence,
                });
            }
        }
        Ok(events)
    }
}
fn base64(bytes: &[u8]) -> Result<String, DeliveryFailure> {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    // Divisible-by-three nonfinal pieces avoid interior base64 padding.
    for part in bytes.chunks(3 * 4096) {
        out.push_str(&shellfox_protocol::encode_chunk(part).map_err(|_| DeliveryFailure::Invalid)?);
    }
    Ok(out)
}
