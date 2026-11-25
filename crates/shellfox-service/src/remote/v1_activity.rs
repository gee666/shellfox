//! Per-view tab activity. Peers learn which tabs are working from `activity`
//! events, as 0.9 sent them. A view starts with no busy tabs reported, so its
//! first poll after pairing carries the tabs that were already working.
use super::*;
use crate::{activity::AgentState, NativeTabActivity};
use shellfox_core::TabId;
use std::{collections::HashSet, sync::Arc};

/// A poll carries at most this many busy tabs; the 1024-event poll limit holds.
const MAX_BUSY: usize = 256;

pub(super) struct Reported {
    rows: Arc<[NativeTabActivity]>,
    busy: HashSet<TabId>,
}
impl Reported {
    /// A view paired while tabs are busy is ready to poll at once.
    pub(super) fn new(rows: Arc<[NativeTabActivity]>) -> Self {
        let rows = if rows.iter().any(working) {
            Arc::from([])
        } else {
            rows
        };
        Self {
            rows,
            busy: HashSet::new(),
        }
    }
    pub(super) fn is_current(&self, rows: &Arc<[NativeTabActivity]>) -> bool {
        Arc::ptr_eq(&self.rows, rows)
    }
    /// Appends what changed since this view's last poll. `private` tabs stay
    /// local-only and never report as busy.
    pub(super) fn report(
        &mut self,
        rows: Arc<[NativeTabActivity]>,
        private: &HashSet<TabId>,
        revision: u64,
        events: &mut Vec<wire::HostEvent>,
    ) {
        // Activity has no metadata revision. As in 0.9, a "native" change
        // tells the client to read the catalog again.
        if events.is_empty() {
            events.push(wire::HostEvent::Changed {
                event: wire::ChangedEvent {
                    revision,
                    reason: wire::ChangeReason::Native,
                    select_session_id: None,
                },
            });
        }
        let busy: HashSet<TabId> = rows
            .iter()
            .filter(|row| working(row) && !private.contains(&row.tab))
            .map(|row| row.tab)
            .take(MAX_BUSY)
            .collect();
        let event = |tab_id, busy| wire::HostEvent::Terminal {
            event: wire::TerminalEvent::Activity { tab_id, busy },
        };
        events.extend(self.busy.difference(&busy).map(|&tab| event(tab, false)));
        events.extend(busy.difference(&self.busy).map(|&tab| event(tab, true)));
        self.busy = busy;
        self.rows = rows;
    }
}
fn working(row: &NativeTabActivity) -> bool {
    !row.closed && row.dot == AgentState::Busy
}
