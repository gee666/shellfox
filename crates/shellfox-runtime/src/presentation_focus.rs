use super::{optional, PresentationLease, Presentations, RuntimeError, Terminal};

impl Presentations {
    /// Ghostty's focus report for a changed focus state, if mode 1004 is on.
    pub(crate) fn focus_report(
        &mut self,
        terminal: &mut Terminal,
        lease: PresentationLease,
        focused: bool,
    ) -> Result<Option<Vec<u8>>, RuntimeError> {
        let Some(entry) = self.entries.iter_mut().find(|entry| entry.lease == lease) else {
            return Ok(None);
        };
        let bytes = terminal.encode_focus(focused).map_err(optional)?;
        let next = (!bytes.is_empty()).then_some(focused);
        if entry.reported_focus == next {
            return Ok(None);
        }
        entry.reported_focus = next;
        Ok(next.map(|_| bytes))
    }
}
