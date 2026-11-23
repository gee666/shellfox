use anyhow::{Result, ensure};
use shellfox_core::TabId;
use shellfox_service::native::NativeFrame;
use std::{collections::HashMap, sync::Arc};

const MAX_VIEWS: usize = 128;
const MAX_CELLS: usize = 2 * 1024 * 1024;
const MAX_TEXT: usize = 32 * 1024 * 1024;
#[derive(Default)]
pub(super) struct Frames {
    frames: HashMap<(TabId, bool), NativeFrame>,
}
impl Frames {
    pub fn accept(&mut self, tab: TabId, observer: bool, frame: NativeFrame) -> Result<()> {
        let key = (tab, observer);
        ensure!(
            frame.frame_id != 0 && frame.frame.identity.tab_id == tab,
            "Invalid native frame identity."
        );
        ensure!(
            matches!(
                (observer, frame.presentation.view),
                (false, shellfox_runtime::PresentationLease::Controller(_))
                    | (true, shellfox_runtime::PresentationLease::Observer(_))
            ),
            "Native presentation rights do not match the view."
        );
        ensure!(
            Arc::ptr_eq(&frame.frame, &frame.presentation.screen)
                && frame.presentation.identity == frame.frame.identity
                && frame.presentation.sequence == frame.frame.sequence,
            "Native presentation and screen identity differ."
        );
        let screen = &frame.frame.screen;
        ensure!(
            frame.presentation.row_metadata.len() == usize::from(screen.rows),
            "Native row metadata is incomplete."
        );
        ensure!(
            screen.cols != 0
                && screen.rows != 0
                && screen.cells.len() == usize::from(screen.cols) * usize::from(screen.rows),
            "Invalid native frame grid."
        );
        if let Some(current) = self.frames.get(&key) {
            ensure!(
                frame.frame_id >= current.frame_id,
                "Late native frame was rejected."
            );
            if frame.frame.identity.generation == current.frame.identity.generation {
                ensure!(
                    frame.frame.sequence >= current.frame.sequence,
                    "Native output sequence regressed."
                );
            }
        }
        let mut cells = screen.cells.len();
        let mut text: usize = screen.cells.iter().map(|cell| cell.text.len()).sum();
        for (other_key, other) in &self.frames {
            if *other_key != key {
                cells = cells.saturating_add(other.frame.screen.cells.len());
                text = text.saturating_add(
                    other
                        .frame
                        .screen
                        .cells
                        .iter()
                        .map(|cell| cell.text.len())
                        .sum(),
                );
            }
        }
        ensure!(
            (self.frames.contains_key(&key) || self.frames.len() < MAX_VIEWS)
                && cells <= MAX_CELLS
                && text <= MAX_TEXT,
            "Native client latest-frame cache is full."
        );
        self.frames.insert(key, frame);
        Ok(())
    }
    pub fn latest(&self, tab: TabId, observer: bool) -> Option<NativeFrame> {
        self.frames.get(&(tab, observer)).cloned()
    }
    pub fn remove(&mut self, tab: TabId, observer: bool) {
        self.frames.remove(&(tab, observer));
    }
    pub fn clear(&mut self) {
        self.frames.clear();
    }
    pub fn retain_snapshot(&mut self, snapshot: &shellfox_core::Snapshot) {
        self.frames.retain(|(tab, _), _| {
            snapshot
                .sessions
                .iter()
                .any(|session| session.tabs.iter().any(|candidate| candidate.id == *tab))
        });
    }
}
