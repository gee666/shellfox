//! A shell that exits on its own takes its tab with it, as 0.9's
//! `reconcileExits` deleted the record, so a restart never brings it back
//! (docs/database-retention.md). A tab whose start failed keeps its error, and
//! automation tabs keep their result. A host stop or a close isn't an exit.
use super::CloseTarget;
use crate::Service;
use shellfox_core::TerminalIdentity;
use shellfox_runtime::{Phase, SourceKind};

impl Service {
    /// Removes exited tabs through the close route, which confirms the
    /// runtime is gone before it deletes the row. Runs on every owner pass; a
    /// failed removal is reported once and retried.
    pub(crate) fn remove_exited_tabs(&mut self) {
        if self.assert_accepting().is_err() {
            return;
        }
        self.exit_failures
            .retain(|tab| self.terminals.contains_key(tab));
        let exited: Vec<_> = self
            .terminals
            .iter()
            .filter(|(tab, terminal)| {
                terminal.runtime.source_kind() == SourceKind::OwnedPty
                    && terminal.runtime.phase() == Phase::Closed
                    && matches!(terminal.startup, Some(Ok(())))
                    // Its create or activate reply reports the start first.
                    && !self.launch_replies.iter().any(|(waiting, _)| waiting == *tab)
            })
            .map(|(tab, terminal)| TerminalIdentity {
                tab_id: *tab,
                generation: terminal.runtime.identity().generation,
            })
            .collect();
        for identity in exited {
            if self.metadata_busy() {
                return;
            }
            let (send, outcome) = std::sync::mpsc::channel();
            self.begin_close(
                CloseTarget::Terminal(identity),
                Box::new(move |result| {
                    let _ = send.send(result);
                }),
            );
            if let Ok(Err(error)) = outcome.try_recv() {
                if self.exit_failures.insert(identity.tab_id) {
                    eprintln!("An exited tab could not be removed; it will retry: {error:#}");
                }
            }
        }
    }
}
