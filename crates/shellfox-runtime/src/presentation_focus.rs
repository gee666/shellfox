use super::{optional, PresentationLease, Presentations, RuntimeError, Terminal};

impl Presentations {
    pub(crate) fn focus_report(
        &mut self,
        terminal: &Terminal,
        lease: PresentationLease,
        focused: bool,
    ) -> Result<Option<&'static [u8]>, RuntimeError> {
        let Some(entry) = self.entries.iter_mut().find(|entry| entry.lease == lease) else {
            return Ok(None);
        };
        let bytes = terminal.encode_xterm_focus(focused).map_err(optional)?;
        let next = bytes.map(|_| focused);
        if entry.reported_focus == next {
            return Ok(None);
        }
        entry.reported_focus = next;
        Ok(bytes)
    }
}
