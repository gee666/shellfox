use shellfox_files::EditorCancellation;

/// The panel's operation tickets. Every started operation ends exactly once: its
/// own reply, a newer ticket, or abandonment. A stale reply never changes state.
///
/// The revision check on returning to a hidden draft runs in the background. It
/// never blocks browsing: any foreground operation cancels it, and the panel checks
/// again once idle.
#[derive(Default)]
pub(crate) struct Activity {
    request: u64,
    busy: bool,
    revalidation: Option<EditorCancellation>,
}

impl Activity {
    pub fn busy(&self) -> bool {
        self.busy
    }

    pub fn revalidating(&self) -> bool {
        self.revalidation.is_some()
    }

    /// Starts a foreground operation. The caller checks `busy` first.
    pub fn begin(&mut self) -> u64 {
        self.stop_revalidation();
        self.busy = true;
        self.next()
    }

    pub fn begin_revalidation(&mut self, cancel: EditorCancellation) -> u64 {
        self.revalidation = Some(cancel);
        self.next()
    }

    /// Whether `ticket` is the latest operation, so its reply may publish.
    pub fn current(&self, ticket: u64) -> bool {
        self.request == ticket
    }

    /// Ends the foreground operation `ticket`. False for a superseded reply.
    pub fn finish(&mut self, ticket: u64) -> bool {
        let current = self.current(ticket) && self.busy;
        if current {
            self.busy = false;
        }
        current
    }

    pub fn finish_revalidation(&mut self, ticket: u64) -> bool {
        self.current(ticket) && self.revalidation.take().is_some()
    }

    /// Ends every operation now; their late replies are ignored.
    pub fn abandon(&mut self) {
        self.stop_revalidation();
        self.busy = false;
        self.next();
    }

    pub fn stop_revalidation(&mut self) {
        if let Some(cancel) = self.revalidation.take() {
            cancel.cancel();
        }
    }

    fn next(&mut self) -> u64 {
        self.request += 1;
        self.request
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The macOS P4 order: a draft on peer A, leave and return to A, then Go.
    #[test]
    fn go_after_returning_to_a_draft_supersedes_the_revision_check() {
        let mut panel = Activity::default();
        let check = EditorCancellation::default();
        let revalidation = panel.begin_revalidation(check.clone());
        assert!(!panel.busy(), "the revision check must leave Go enabled");

        let go = panel.begin();
        assert!(check.is_cancelled());
        assert!(!panel.revalidating());
        assert!(!panel.finish_revalidation(revalidation));
        assert!(panel.busy());
        assert!(panel.finish(go), "the folder reply publishes");
        assert!(!panel.busy());
    }

    #[test]
    fn switching_source_ends_a_pending_read_and_ignores_its_reply() {
        let mut panel = Activity::default();
        let read = panel.begin();
        panel.abandon();
        assert!(!panel.busy());
        assert!(!panel.finish(read));

        let go = panel.begin();
        assert!(!panel.finish(read));
        assert!(panel.finish(go));
        assert!(!panel.busy());
    }

    #[test]
    fn a_revision_check_reply_ends_only_its_own_check() {
        let mut panel = Activity::default();
        let first = panel.begin_revalidation(EditorCancellation::default());
        panel.abandon();
        let second = panel.begin_revalidation(EditorCancellation::default());
        assert!(!panel.finish_revalidation(first));
        assert!(panel.revalidating());
        assert!(panel.finish_revalidation(second));
        assert!(!panel.revalidating());
    }
}
