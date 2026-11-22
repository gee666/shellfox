//! Exactly one real Runtime delivery role per manager-v2 attachment.
#[path = "manager_delivery_ack.rs"]
mod ack;
#[path = "manager_delivery_events.rs"]
mod events;
use shellfox_protocol::{manager_v2 as wire, Validate};
use shellfox_runtime::{
    AnsiRecoveryCapability, Chunk, Identity, LayoutStamp, Lease, ObserverLease, RecoveryId,
    Response,
};
use std::collections::VecDeque;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Controller(Lease),
    Observer(ObserverLease),
}
#[derive(Clone, Copy)]
struct Stamp {
    wire_id: wire::Id,
    native_id: RecoveryId,
    sequence: u64,
    revision: u64,
    cols: u16,
    rows: u16,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Failure {
    Stale,
    RecoveryRequired,
    Invalid,
    Bounds,
}
pub struct ManagerDelivery {
    identity: Identity,
    role: Role,
    reference: wire::DeliveryRef,
    control: Option<wire::ControlRef>,
    layout: Option<wire::LayoutRef>,
    pending: VecDeque<Chunk>,
    bytes: usize,
    received: u64,
    offered: u64,
    acked: u64,
    recovery: Option<Stamp>,
    accepted: Option<Stamp>,
    layout_pending: Option<LayoutStamp>,
    revoked: Option<wire::RevokedReason>,
    lost: bool,
    ended: Option<u64>,
}
impl ManagerDelivery {
    pub fn new(identity: Identity, role: Role) -> Result<Self, Failure> {
        let epoch = match role {
            Role::Controller(lease) => lease.epoch,
            Role::Observer(lease) => lease.epoch,
        };
        let tab_id = identity
            .tab_id
            .to_string()
            .parse()
            .map_err(|_| Failure::Invalid)?;
        let generation = identity
            .generation
            .to_string()
            .parse()
            .map_err(|_| Failure::Invalid)?;
        let reference = wire::DeliveryRef {
            tab_id,
            generation,
            delivery_id: wire::Id::new(),
            delivery_epoch: epoch,
        };
        reference.validate().map_err(|_| Failure::Invalid)?;
        let (control, layout) = if matches!(role, Role::Controller(_)) {
            (
                Some(wire::ControlRef {
                    tab_id,
                    generation,
                    control_id: wire::Id::new(),
                    control_epoch: epoch,
                }),
                Some(wire::LayoutRef {
                    tab_id,
                    generation,
                    layout_id: wire::Id::new(),
                    layout_epoch: epoch,
                }),
            )
        } else {
            (None, None)
        };
        Ok(Self {
            identity,
            role,
            reference,
            control,
            layout,
            pending: VecDeque::new(),
            bytes: 0,
            received: 0,
            offered: 0,
            acked: 0,
            recovery: None,
            accepted: None,
            layout_pending: None,
            revoked: None,
            lost: true,
            ended: None,
        })
    }
    pub fn identity(&self) -> Identity {
        self.identity
    }
    pub fn role(&self) -> Role {
        self.role
    }
    pub fn reference(&self) -> &wire::DeliveryRef {
        &self.reference
    }
    pub fn control(&self) -> Option<&wire::ControlRef> {
        self.control.as_ref()
    }
    pub fn layout(&self) -> Option<&wire::LayoutRef> {
        self.layout.as_ref()
    }
    pub fn revoked(&self) -> bool {
        self.revoked.is_some()
    }
    pub fn revoked_reason(&self) -> Option<wire::RevokedReason> {
        self.revoked
    }
    pub fn revoke(&mut self, reason: wire::RevokedReason) {
        self.revoked = Some(reason);
        self.invalidate();
        self.control = None;
        self.layout = None;
        self.layout_pending = None;
    }
    fn invalidate(&mut self) {
        self.pending.clear();
        self.bytes = 0;
        self.recovery = None;
        self.accepted = None;
        self.lost = true;
        self.ended = None;
    }
    pub fn recover(&mut self, response: Response) -> Result<wire::Recovery, Failure> {
        let Response::Recovery(stamp) = response else {
            return Err(Failure::Invalid);
        };
        if stamp.capability != AnsiRecoveryCapability::FormattedActiveBufferV1
            || stamp.bytes.len() > wire::MAX_RECOVERY_BYTES
        {
            return Err(Failure::Bounds);
        }
        let cut = shellfox_runtime::RecoveryCut {
            recovery_id: stamp.recovery_id,
            last_delivery_sequence: stamp.last_delivery_sequence,
            cols: stamp.cols,
            rows: stamp.rows,
            layout_revision: stamp.layout_revision,
        };
        let recovery = wire::Recovery::FormattedActiveBuffer {
            recovery_id: wire::Id::new(),
            encoding: wire::Encoding::Base64,
            data: base64(&stamp.bytes)?,
            last_sequence: cut.last_delivery_sequence,
            cols: cut.cols,
            rows: cut.rows,
            layout_revision: cut.layout_revision,
        };
        self.commit_recovery(recovery, cut)
    }
    fn commit_recovery(
        &mut self,
        recovery: wire::Recovery,
        cut: shellfox_runtime::RecoveryCut,
    ) -> Result<wire::Recovery, Failure> {
        if self.revoked() {
            return Err(Failure::Stale);
        }
        if self.layout_pending.is_some_and(|pending| {
            pending.layout_revision > cut.layout_revision
                || pending.last_delivery_sequence > cut.last_delivery_sequence
        }) {
            return Err(Failure::RecoveryRequired);
        }
        recovery.validate().map_err(|_| Failure::Invalid)?;
        self.pending.clear();
        self.bytes = 0;
        self.received = cut.last_delivery_sequence;
        self.offered = cut.last_delivery_sequence;
        self.acked = cut.last_delivery_sequence;
        self.recovery = Some(Stamp {
            wire_id: recovery.recovery_id(),
            native_id: cut.recovery_id,
            sequence: cut.last_delivery_sequence,
            revision: cut.layout_revision,
            cols: cut.cols,
            rows: cut.rows,
        });
        self.accepted = None;
        self.lost = false;
        self.layout_pending = None;
        Ok(recovery)
    }
    pub fn ready_control(&self, control: &wire::ControlRef) -> Result<Lease, Failure> {
        if self.control.as_ref() != Some(control) || self.revoked() {
            return Err(Failure::Stale);
        }
        if self.lost
            || self.recovery.is_some()
            || self.layout_pending.is_some()
            || self.accepted.is_none()
        {
            return Err(Failure::RecoveryRequired);
        }
        match self.role {
            Role::Controller(lease) => Ok(lease),
            Role::Observer(_) => Err(Failure::Stale),
        }
    }
    pub fn geometry(
        &self,
        control: &wire::ControlRef,
        geometry: &wire::GeometryStamp,
    ) -> Result<shellfox_runtime::ControllerGeometryStamp, Failure> {
        self.ready_control(control)?;
        let stamp = self.accepted.ok_or(Failure::RecoveryRequired)?;
        if stamp.wire_id != geometry.recovery_id
            || stamp.revision != geometry.layout_revision
            || stamp.cols != geometry.cols
            || stamp.rows != geometry.rows
        {
            return Err(Failure::Stale);
        }
        Ok(shellfox_runtime::ControllerGeometryStamp {
            recovery_id: stamp.native_id,
            layout_revision: stamp.revision,
            cols: stamp.cols,
            rows: stamp.rows,
        })
    }
    pub fn layout_revision(&self) -> Option<u64> {
        self.accepted.map(|stamp| stamp.revision)
    }
    pub fn current_layout(&self) -> Option<LayoutStamp> {
        self.layout_pending.or_else(|| {
            self.accepted.map(|stamp| LayoutStamp {
                cols: stamp.cols,
                rows: stamp.rows,
                layout_revision: stamp.revision,
                last_delivery_sequence: stamp.sequence,
            })
        })
    }
    pub fn poll(&mut self, budget: usize, limit: usize) -> Result<Vec<wire::Event>, Failure> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        if let Some(reason) = self.revoked {
            return Ok(vec![wire::Event::Revoked {
                delivery: self.reference.clone(),
                reason,
            }]);
        }
        if let Some(layout) = self.layout_pending {
            return Ok(vec![wire::Event::Layout {
                delivery: self.reference.clone(),
                layout_revision: layout.layout_revision,
                cols: layout.cols,
                rows: layout.rows,
                recovery_required: true,
                last_sequence: layout.last_delivery_sequence,
            }]);
        }
        if self.lost {
            return Err(Failure::RecoveryRequired);
        }
        if self.recovery.is_some() {
            return Ok(Vec::new());
        }
        let (mut events, mut bytes, mut through) = (Vec::new(), 0, self.acked);
        for chunk in &self.pending {
            if events.len() >= limit.min(1024)
                || bytes + chunk.bytes.len() > budget.min(wire::MAX_POLL_DATA_BYTES)
            {
                break;
            }
            events.push(wire::Event::Data {
                delivery: self.reference.clone(),
                sequence: chunk.sequence,
                encoding: wire::Encoding::Base64,
                data: base64(&chunk.bytes)?,
            });
            bytes += chunk.bytes.len();
            through = chunk.sequence;
            self.offered = self.offered.max(chunk.sequence);
        }
        if let Some(last_sequence) = self.ended {
            if through >= last_sequence && events.len() < limit.min(1024) {
                events.push(wire::Event::SourceEnded {
                    delivery: self.reference.clone(),
                    last_sequence,
                });
            }
        }
        Ok(events)
    }
}
fn base64(bytes: &[u8]) -> Result<String, Failure> {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for part in bytes.chunks(3 * 4096) {
        out.push_str(&shellfox_protocol::encode_chunk(part).map_err(|_| Failure::Invalid)?);
    }
    Ok(out)
}
