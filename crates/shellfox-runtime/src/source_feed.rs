use crate::{owner::Owner, *};
impl Owner {
    pub(crate) fn accept_source_bytes(
        &mut self,
        bytes: &[u8],
    ) -> Result<(DeliverySequence, DeliverySequence), RuntimeError> {
        let first = self
            .delivery
            .current_sequence()
            .checked_add(1)
            .ok_or_else(|| RuntimeError::unavailable("Output sequence exhausted."))?;
        self.terminal
            .feed(bytes)
            .map_err(RuntimeError::unavailable)?;
        self.replies()?;
        let last = self
            .delivery
            .append_bytes(bytes)
            .map_err(RuntimeError::unavailable)?;
        self.presentations.source_changed()?;
        if self.source_kind == SourceKind::OwnedPty {
            metadata::collect(
                &mut self.terminal,
                &self.shared.metadata,
                self.identity,
                last,
            )?;
        } else {
            // Passive mirrors never publish VT callback effects as live bells,
            // clipboard, title or CWD changes.
            self.terminal
                .take_metadata()
                .map_err(RuntimeError::unavailable)?;
        }
        // External hooks are explicit publication-gated DISPLAY observers; no
        // hooks are installed by default, and replay/render never use this path.
        self.live_output
            .emit(self.identity, last, bytes, &self.shared.live_output_status);
        self.dirty = true;
        Ok((DeliverySequence(first), DeliverySequence(last)))
    }
}
