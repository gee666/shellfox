//! Change signals for in-process desktop clients. Values are published by the
//! owner thread; clients await `changed()` instead of polling the owner.
use crate::{power::PowerReport, NativeTabActivity};
use shellfox_core::TabId;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use tokio::sync::watch;

/// Receivers for one client. Each starts at the current published value.
pub struct NativeWatches {
    /// Host metadata revision (sessions, tabs, settings).
    pub revision: watch::Receiver<u64>,
    pub power: watch::Receiver<PowerReport>,
    /// The same rows as `NativeAction::ReadTabActivity`.
    pub tab_activity: watch::Receiver<Arc<[NativeTabActivity]>>,
    pub frames: FrameWakes,
    /// Moves when any desktop launch receipt changes state.
    pub launches: watch::Receiver<u64>,
}

/// Per-tab counters bumped when a tab's presentation may have changed:
/// runtime events, terminal commands and non-frame presentation actions.
/// A closed tab drops its sender, which ends waiting receivers.
#[derive(Clone, Default)]
pub struct FrameWakes(Arc<Mutex<HashMap<TabId, watch::Sender<u64>>>>);
impl FrameWakes {
    pub fn subscribe(&self, tab: TabId) -> watch::Receiver<u64> {
        let mut tabs = self.0.lock().unwrap_or_else(|e| e.into_inner());
        tabs.entry(tab).or_insert_with(|| watch::channel(0).0).subscribe()
    }
    pub(crate) fn wake(&self, tab: TabId) {
        let tabs = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(sender) = tabs.get(&tab) {
            sender.send_modify(|count| *count = count.wrapping_add(1));
        }
    }
    pub(crate) fn retain(&self, live: impl Fn(TabId) -> bool) {
        let mut tabs = self.0.lock().unwrap_or_else(|e| e.into_inner());
        tabs.retain(|tab, sender| live(*tab) && sender.receiver_count() > 0);
    }
}

pub(crate) struct WatchSenders {
    revision: watch::Sender<u64>,
    power: watch::Sender<PowerReport>,
    tab_activity: watch::Sender<Arc<[NativeTabActivity]>>,
    pub(crate) frames: FrameWakes,
    launches: watch::Sender<u64>,
}
impl Default for WatchSenders {
    fn default() -> Self {
        Self {
            revision: watch::channel(0).0,
            power: watch::channel(PowerReport {
                status: crate::power::PowerStatus::Inactive,
                partial_reason: None,
                error: None,
                pending: false,
            })
            .0,
            tab_activity: watch::channel(Arc::from([])).0,
            frames: FrameWakes::default(),
            launches: watch::channel(0).0,
        }
    }
}
impl WatchSenders {
    pub(crate) fn subscribe(&self) -> NativeWatches {
        NativeWatches {
            revision: self.revision.subscribe(),
            power: self.power.subscribe(),
            tab_activity: self.tab_activity.subscribe(),
            frames: self.frames.clone(),
            launches: self.launches.subscribe(),
        }
    }
}
impl crate::Service {
    /// Runs at the end of every owner pump; sends only values that changed.
    pub(crate) fn native_watches(&self) -> NativeWatches {
        self.publish_native_watches();
        self.native.watches.subscribe()
    }
    /// Called by the launch router's owner with its receipt generation.
    pub(crate) fn publish_launch_receipts(&self, generation: u64) {
        self.native.watches.launches.send_if_modified(|current| {
            std::mem::replace(current, generation) != generation
        });
    }
    pub(crate) fn publish_native_watches(&self) {
        let watches = &self.native.watches;
        let revision = self.host().revision();
        watches.revision.send_if_modified(|current| {
            std::mem::replace(current, revision) != revision
        });
        let power = self.power_report();
        watches.power.send_if_modified(|current| {
            let changed = *current != power;
            if changed {
                *current = power;
            }
            changed
        });
        let activity = self.desktop_tab_activity();
        watches.tab_activity.send_if_modified(|current| {
            let changed = **current != *activity;
            if changed {
                *current = Arc::from(activity);
            }
            changed
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_wakes_count_per_tab_and_close_on_retain() {
        let wakes = FrameWakes::default();
        let (first, second) = (TabId::new(), TabId::new());
        let mut a = wakes.subscribe(first);
        let b = wakes.subscribe(second);
        wakes.wake(first);
        assert!(a.has_changed().unwrap());
        assert_eq!(*a.borrow_and_update(), 1);
        assert!(!b.has_changed().unwrap());
        wakes.retain(|tab| tab == second);
        assert!(a.has_changed().is_err(), "closed tab ends its receivers");
        assert!(!b.has_changed().unwrap());
    }
}
