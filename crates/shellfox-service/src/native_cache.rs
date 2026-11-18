use super::*;
use anyhow::{ensure, Context, Result};
use shellfox_runtime::{Event, Lease, ObserverLease, ViewFrame};
use std::collections::HashMap;
pub(super) const MAX_VIEWS: usize = 128;
const MAX_CELLS: usize = 2 * 1024 * 1024;
const MAX_TEXT: usize = 32 * 1024 * 1024;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) struct ViewKey {
    pub controller: u64,
    pub tab: TabId,
    pub observer: bool,
}
#[derive(Clone, Debug)]
pub(super) enum Grant {
    Control(Attachment),
    Observer(NativeObserverAttachment),
}
impl Grant {
    pub fn generation(&self) -> Generation {
        match self {
            Self::Control(a) => a.generation,
            Self::Observer(a) => a.generation,
        }
    }
    fn control_matches(&self, lease: Lease) -> bool {
        matches!(self, Self::Control(a) if a.controller == lease.controller && a.epoch == lease.epoch)
    }
    fn observer_matches(&self, lease: ObserverLease) -> bool {
        matches!(self, Self::Observer(a) if a.viewer == lease.viewer && a.epoch == lease.epoch)
    }
}
#[derive(Clone, Debug)]
pub(super) struct View {
    pub grant: Grant,
    pub delivered: u64,
    pub barrier: Option<u64>,
    pub accepted: Option<u64>,
    pub offered: Option<(u64, u64)>,
    pub acked: Option<u64>,
    pub exited: Option<u64>,
    pub replay_lost: bool,
    pub phase: Phase,
    pub error: Option<String>,
}
impl View {
    pub fn new(grant: Grant, barrier: u64, phase: Phase) -> Self {
        Self {
            grant,
            delivered: barrier,
            barrier: Some(barrier),
            accepted: None,
            offered: None,
            acked: None,
            exited: None,
            replay_lost: false,
            phase,
            error: None,
        }
    }
    pub fn accepted_sequence(&mut self, frame_id: u64) -> Result<u64> {
        ensure!(
            !self.replay_lost,
            "Native view requires recovery before accepting output."
        );
        let (offered, sequence) = self.offered.context("No native frame was offered.")?;
        ensure!(
            frame_id == offered,
            "Native frame acceptance is stale or fabricated."
        );
        if let Some(barrier) = self.barrier {
            ensure!(
                sequence >= barrier,
                "Native frame predates the recovery barrier."
            );
            self.accepted = Some(sequence);
            return Ok(barrier);
        }
        self.accepted = Some(sequence);
        // A newer private presentation/version does not grant raw-output credit.
        Ok(sequence.min(self.delivered))
    }
}
struct Cached {
    id: u64,
    frame: Arc<ViewFrame>,
    cells: usize,
    text: usize,
}
#[derive(Default)]
pub(crate) struct NativeState {
    pub(super) views: HashMap<ViewKey, View>,
    frames: HashMap<ViewKey, Cached>,
    next_frame: u64,
}
impl NativeState {
    pub(super) fn admit(&self, key: ViewKey) -> Result<()> {
        ensure!(
            self.views.contains_key(&key) || self.views.len() < MAX_VIEWS,
            "Native view limit reached."
        );
        Ok(())
    }
    pub(super) fn remove(&mut self, key: ViewKey) {
        self.views.remove(&key);
        self.prune();
    }
    pub(super) fn prune(&mut self) {
        self.frames.retain(|key, cached| {
            self.views
                .get(key)
                .is_some_and(|view| view.grant.generation() == cached.frame.identity.generation)
        });
    }
    pub(super) fn capture(&mut self, key: ViewKey, frame: Arc<ViewFrame>) -> Result<()> {
        ensure!(
            frame.identity.tab_id == key.tab,
            "Native frame tab mismatch."
        );
        let screen = &frame.screen.screen;
        ensure!(
            frame.row_metadata.len() == usize::from(screen.rows)
                && screen.cells.len() == usize::from(screen.cols) * usize::from(screen.rows),
            "Native presentation grid is invalid."
        );
        if self.frames.get(&key).is_some_and(|cached| {
            cached.frame.identity == frame.identity
                && cached.frame.view == frame.view
                && cached.frame.version == frame.version
        }) {
            return Ok(());
        }
        let cells = screen.cells.len();
        let text: usize = screen.cells.iter().map(|cell| cell.text.len()).sum();
        let other_cells: usize = self
            .frames
            .iter()
            .filter(|(id, _)| **id != key)
            .map(|(_, f)| f.cells)
            .sum();
        let other_text: usize = self
            .frames
            .iter()
            .filter(|(id, _)| **id != key)
            .map(|(_, f)| f.text)
            .sum();
        ensure!(
            (self.frames.contains_key(&key) || self.frames.len() < MAX_VIEWS)
                && other_cells.saturating_add(cells) <= MAX_CELLS
                && other_text.saturating_add(text) <= MAX_TEXT,
            "Native presentation cache budget refused."
        );
        self.next_frame = self
            .next_frame
            .checked_add(1)
            .context("Native frame IDs exhausted.")?;
        self.frames.insert(
            key,
            Cached {
                id: self.next_frame,
                frame,
                cells,
                text,
            },
        );
        Ok(())
    }
    pub(super) fn offer(&mut self, key: ViewKey) -> Result<Option<NativeFrame>> {
        let view = self
            .views
            .get_mut(&key)
            .context("Native view is detached.")?;
        let Some(cached) = self.frames.get(&key) else {
            return Ok(None);
        };
        ensure!(
            cached.frame.identity.generation == view.grant.generation(),
            "Native frame generation is stale."
        );
        if view
            .barrier
            .is_some_and(|barrier| cached.frame.sequence < barrier)
        {
            return Ok(None);
        }
        view.offered = Some((cached.id, cached.frame.sequence));
        Ok(Some(NativeFrame {
            frame_id: cached.id,
            frame: cached.frame.screen.clone(),
            presentation: cached.frame.clone(),
            highest_delivered: view.delivered,
            recovery_barrier: view.barrier,
            exited: view.exited,
            replay_lost: view.replay_lost,
            phase: view.phase,
            error: view.error.clone(),
        }))
    }
    /// Observes the sole Service::drain consumer. Never parses or grants credit.
    pub(crate) fn record_event(&mut self, tab: TabId, event: &Event) {
        for (key, view) in &mut self.views {
            if key.tab != tab {
                continue;
            }
            match event {
                Event::Output { lease, chunk } if view.grant.control_matches(*lease) => {
                    view.delivered = view.delivered.max(chunk.sequence)
                }
                Event::ObserverOutput { observer, chunk }
                    if view.grant.observer_matches(*observer) =>
                {
                    view.delivered = view.delivered.max(chunk.sequence)
                }
                Event::ReplayLost { lease, .. } if view.grant.control_matches(*lease) => {
                    view.replay_lost = true
                }
                Event::ObserverReplayLost { observer, .. }
                    if view.grant.observer_matches(*observer) =>
                {
                    view.replay_lost = true
                }
                Event::Exited {
                    lease,
                    last_sequence,
                } if view.grant.control_matches(*lease) => view.exited = Some(*last_sequence),
                Event::ObserverExited {
                    observer,
                    last_sequence,
                } if view.grant.observer_matches(*observer) => view.exited = Some(*last_sequence),
                Event::Phase { phase, error } => {
                    view.phase = *phase;
                    view.error = error.clone();
                }
                Event::Started(Err(error)) => view.error = Some(error.to_string()),
                _ => {}
            }
        }
    }
}
impl crate::Service {
    /// The pump prunes authority only. Private frames are requested through the
    /// checked Presentation command; canonical take_screen is not a native view.
    pub(crate) fn native_capture_frames(&mut self) {
        let stale: Vec<_> = self
            .native
            .views
            .iter()
            .filter_map(|(key, view)| {
                let live = self.terminals.get(&key.tab).is_some_and(|terminal| {
                    terminal.runtime.identity().generation == view.grant.generation()
                });
                let valid = match &view.grant {
                    Grant::Control(a) => self.lease(key.tab, a).is_ok(),
                    Grant::Observer(_) => true,
                };
                (!live || !valid).then_some(*key)
            })
            .collect();
        for key in stale {
            self.native.remove(key);
        }
        self.native.prune();
    }
}
