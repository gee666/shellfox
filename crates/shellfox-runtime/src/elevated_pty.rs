use super::Owner;
use crate::{ErrorCode, Event, Phase, RuntimeError};
use shellfox_terminal::{elevated as helper, PtyConfig};
use std::time::Duration;

pub(crate) struct ElevatedPty {
    session: helper::Session,
    generation: helper::Generation,
    pending_size: Option<(u16, u16)>,
}
impl ElevatedPty {
    pub(crate) fn write(&mut self, bytes: &[u8]) -> Result<(), RuntimeError> {
        self.session
            .write(self.generation, bytes)
            .map(|_| ())
            .map_err(command_error)
    }
    pub(crate) fn resize(&mut self, cols: u16, rows: u16) -> Result<(), RuntimeError> {
        if !(2..=500).contains(&cols) || !(2..=500).contains(&rows) {
            return Err(command_error(helper::AdapterError::InvalidInput));
        }
        if self.session.phase() == helper::Phase::Starting {
            self.pending_size = Some((cols, rows));
            return Ok(());
        }
        self.session
            .resize(self.generation, cols, rows)
            .map_err(command_error)
    }
    pub(crate) fn begin_close(&mut self) -> Result<(), RuntimeError> {
        self.session
            .begin_close(self.generation)
            .map_err(RuntimeError::unavailable)
    }
    pub(crate) fn shutdown(&mut self) -> Result<(), RuntimeError> {
        if self.session.phase() != helper::Phase::Closed {
            let _ = self.begin_close();
            for _ in 0..64 {
                match self.poll()? {
                    Some(helper::Event::ExitConfirmed(_)) | None => break,
                    _ => {} // Retired output is never parsed or answered.
                }
            }
        }
        self.session
            .finish(self.generation, Duration::ZERO)
            .map_err(RuntimeError::unavailable)
    }
    fn poll(&mut self) -> Result<Option<helper::Event>, RuntimeError> {
        self.session
            .poll(self.generation)
            .map_err(RuntimeError::unavailable)
    }
}

fn command_error(error: helper::AdapterError) -> RuntimeError {
    match error {
        helper::AdapterError::InvalidInput => RuntimeError::validation(error.to_string()),
        helper::AdapterError::QueueFull => RuntimeError {
            code: ErrorCode::ResourceLimit,
            message: error.to_string(),
        },
        _ => RuntimeError::unavailable(error),
    }
}

impl Owner {
    pub(crate) fn start_administrator(
        &mut self,
        factory: helper::InteractiveHelper,
        config: &PtyConfig,
    ) {
        // This channel is owned by one Runtime generation; it is never reused.
        let generation = helper::Generation::new(1).expect("nonzero helper generation");
        let approval = helper::UserApproval::from_user_action(generation);
        let (session, failure) = match factory.launch(approval, generation, config) {
            Ok(session) => (Some(session), None),
            Err(error) => (error.retained.map(|session| *session), Some(error.error)),
        };
        self.elevated = session.map(|session| ElevatedPty {
            session,
            generation,
            pending_size: None,
        });
        if let Some(error) = failure {
            self.administrator_failed(RuntimeError::unavailable(error));
        }
    }

    pub(crate) fn poll_administrator(&mut self, budget: usize) {
        for _ in 0..budget {
            let Some(pty) = self.elevated.as_mut() else {
                return;
            };
            if pty.session.phase() == helper::Phase::Closed {
                // The ordered exit was already drained. The launcher and pipe
                // workers can still be exiting; keep polling until they join.
                let _ = self.cleanup();
                return;
            }
            let result = match pty.poll() {
                Ok(Some(helper::Event::Ready(_))) if self.phase == Phase::Starting => {
                    if let Some((cols, rows)) = pty.pending_size.take() {
                        if let Err(error) = pty.resize(cols, rows) {
                            self.administrator_failed(error);
                            return;
                        }
                    }
                    self.set_phase(Phase::Active, None);
                    self.outgoing.push_front(Event::Started(Ok(())));
                    Ok(())
                }
                Ok(Some(helper::Event::Data(bytes))) => {
                    self.accept_source_bytes(&bytes).map(|_| ())
                }
                Ok(Some(helper::Event::ExitConfirmed(_))) => {
                    if self.phase == Phase::Starting {
                        self.administrator_failed(RuntimeError::unavailable(
                            "Administrator PowerShell did not start.",
                        ));
                        return;
                    }
                    self.set_phase(Phase::Draining, None);
                    continue;
                }
                Ok(Some(helper::Event::Failed(error))) if self.phase != Phase::Draining => {
                    Err(RuntimeError::unavailable(error))
                }
                Ok(Some(_)) => Ok(()),
                Ok(None) => break,
                Err(error) => Err(error),
            };
            if let Err(error) = result {
                self.administrator_failed(error);
                break;
            }
        }
    }

    fn administrator_failed(&mut self, error: RuntimeError) {
        if self.phase == Phase::Starting {
            self.outgoing.push_front(Event::Started(Err(error.clone())));
        }
        self.fail(error);
    }
}
