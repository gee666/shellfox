use crate::{owner::Owner, *};
use shellfox_terminal::{PtyOutput, PtySession};
impl Owner {
    pub(crate) fn poll_output(&mut self, budget: usize) {
        if self.source_kind == SourceKind::ExternalBytes {
            self.poll_external(budget);
            return;
        }
        #[cfg(windows)]
        if self.elevated.is_some() {
            self.poll_administrator(budget);
            return;
        }
        if self.pty.is_none() || self.phase == Phase::Closed {
            return;
        }
        let status = self.pty.as_mut().map(PtySession::try_wait);
        match status {
            Some(Ok(Some(_))) if self.phase == Phase::Active => {
                self.set_phase(Phase::Draining, None)
            }
            Some(Err(error)) => {
                self.fail(RuntimeError::unavailable(error));
                return;
            }
            _ => {}
        }
        for _ in 0..budget {
            let result = self.pty.as_ref().map(PtySession::try_read);
            match result {
                Some(Ok(Some(PtyOutput::Data(bytes)))) => {
                    if let Err(error) = self.accept_source_bytes(&bytes) {
                        self.fail(error);
                        return;
                    }
                }
                Some(Ok(Some(PtyOutput::Eof))) => {
                    if let Err(error) = self.cleanup() {
                        self.fail(error);
                    }
                    break;
                }
                Some(Ok(Some(PtyOutput::ReadError(error)))) => {
                    self.fail(RuntimeError::unavailable(error));
                    break;
                }
                Some(Err(error)) => {
                    self.fail(RuntimeError::unavailable(format!(
                        "PTY disconnected before EOF: {error}"
                    )));
                    break;
                }
                _ => break,
            }
        }
    }
}
