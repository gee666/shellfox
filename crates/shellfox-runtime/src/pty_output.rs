use crate::{owner::Owner, *};
use shellfox_terminal::{PtyOutput, PtySession};
impl Owner {
    /// Reads at most `budget` chunks. True means the budget ran out and more
    /// output may be waiting, so the caller must poll again before blocking.
    pub(crate) fn poll_output(&mut self, budget: usize) -> bool {
        if self.source_kind == SourceKind::ExternalBytes {
            return self.poll_external(budget);
        }
        #[cfg(windows)]
        if self.elevated.is_some() {
            return self.poll_administrator(budget);
        }
        if self.pty.is_none() || self.phase == Phase::Closed {
            return false;
        }
        let status = self.pty.as_mut().map(PtySession::try_wait);
        match status {
            Some(Ok(Some(_))) if self.phase == Phase::Active => {
                self.set_phase(Phase::Draining, None)
            }
            Some(Err(error)) => {
                self.fail(RuntimeError::unavailable(error));
                return false;
            }
            _ => {}
        }
        for _ in 0..budget {
            let result = self.pty.as_ref().map(PtySession::try_read);
            match result {
                Some(Ok(Some(PtyOutput::Data(bytes)))) => {
                    if let Err(error) = self.accept_source_bytes(&bytes) {
                        self.fail(error);
                        return false;
                    }
                }
                Some(Ok(Some(PtyOutput::Eof))) => {
                    if let Err(error) = self.cleanup() {
                        self.fail(error);
                    }
                    return false;
                }
                Some(Ok(Some(PtyOutput::ReadError(error)))) => {
                    self.fail(RuntimeError::unavailable(error));
                    return false;
                }
                Some(Err(error)) => {
                    self.fail(RuntimeError::unavailable(format!(
                        "PTY disconnected before EOF: {error}"
                    )));
                    return false;
                }
                _ => return false,
            }
        }
        true
    }
}
