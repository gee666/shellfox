//! Whether a terminal's control is free, so views waiting for it can take it back.
use shellfox_core::{Generation, TabId};

impl crate::Service {
    /// A view holds the control lease of `tab`; a detached or retiring one doesn't.
    pub(crate) fn controlled(&self, tab: TabId) -> bool {
        self.terminals
            .get(&tab)
            .is_some_and(|terminal| terminal.lease.is_some() && !terminal.lease_retiring)
    }
    /// The epoch the next claim of a free `generation` gets, or None while a
    /// view controls it or the generation is gone.
    pub(crate) fn free_control_epoch(&self, tab: TabId, generation: Generation) -> Option<u64> {
        let terminal = self.terminals.get(&tab)?;
        (terminal.runtime.identity().generation == generation && !self.controlled(tab))
            .then(|| terminal.control_epoch.saturating_add(1))
    }
}
