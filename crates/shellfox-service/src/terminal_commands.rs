use super::*;
use crate::activity_integration::InputAdmission;

impl Service {
    fn submit_command(&mut self, tab: TabId, id: RequestId, command: Command) -> Result<()> {
        self.terminals
            .get(&tab)
            .context("Terminal is not running.")?
            .runtime
            .try_send(Request { id, command })
            .map_err(|error| match error {
                shellfox_runtime::SubmitError::Invalid(error) => {
                    anyhow::anyhow!("Runtime command invalid: {error}")
                }
                shellfox_runtime::SubmitError::Full(_) => {
                    anyhow::anyhow!("Runtime command queue is full.")
                }
                shellfox_runtime::SubmitError::Disconnected(_) => {
                    anyhow::anyhow!("Runtime command queue is closed.")
                }
            })
    }
    pub(crate) fn request_close(&mut self, tab: TabId) -> Result<()> {
        self.call(tab, Command::RetryClose)?;
        Ok(())
    }
    pub(crate) fn call(&mut self, tab: TabId, command: Command) -> Result<Response> {
        if matches!(
            command,
            Command::RawInput { .. }
                | Command::Key { .. }
                | Command::Paste { .. }
                | Command::Mouse { .. }
                | Command::KeyV2 { .. }
                | Command::InteractiveBytes { .. }
                | Command::MouseStamped { .. }
        ) {
            bail!("Input requires an admission-captured context.");
        }
        self.call_admitted(tab, command, None)
    }
    pub(crate) fn call_input(
        &mut self,
        tab: TabId,
        command: Command,
        admission: InputAdmission,
    ) -> Result<Response> {
        self.call_admitted(tab, command, Some(admission))
    }
    fn call_admitted(
        &mut self,
        tab: TabId,
        command: Command,
        admission: Option<InputAdmission>,
    ) -> Result<Response> {
        if matches!(command, Command::Claim { .. } | Command::Observe { .. }) {
            bail!("Ownership grants require retained Service request custody.");
        }
        let cleanup = matches!(command, Command::RetryClose);
        if !cleanup {
            self.assert_owned()?;
        }
        let id = self.request_id()?;
        let identity = self
            .terminals
            .get(&tab)
            .context("Terminal is not running.")?
            .runtime
            .identity();
        let sink = self.activity.owner.live_output_sink(identity);
        if let Some(admission) = admission {
            let context_sink = sink
                .as_ref()
                .context("Input context registry is unavailable; input was not submitted.")?;
            context_sink
                .register_input(identity, id, admission)
                .map_err(|_| {
                    anyhow::anyhow!(
                        "Input context admission failed; no runtime command was submitted."
                    )
                })?;
        }
        if let Err(error) = self.submit_command(tab, id, command) {
            if let Some(sink) = &sink {
                sink.cancel_input(identity, id);
            }
            return Err(error);
        }
        if let Some(terminal) = self.terminals.get_mut(&tab) {
            terminal.waiting = Some(id);
            terminal.replies.clear();
        }
        let deadline = Instant::now() + Duration::from_secs(12);
        let result = loop {
            if !cleanup {
                if let Err(error) = self.assert_owned() {
                    break Err(error);
                }
            }
            if let Err(error) = self.drain(tab) {
                break Err(error);
            }
            let Some(terminal) = self.terminals.get_mut(&tab) else {
                break Err(anyhow::anyhow!("Terminal owner disappeared."));
            };
            if let Some(index) = terminal
                .replies
                .iter()
                .position(|(reply, _)| reply.0 == id.0)
            {
                terminal.waiting = None;
                let Some((_, reply)) = terminal.replies.remove(index) else {
                    break Err(anyhow::anyhow!("Missing runtime reply."));
                };
                break reply.map_err(Into::into);
            }
            if Instant::now() >= deadline {
                break Err(anyhow::anyhow!("Runtime command deadline exceeded; outcome may be uncertain and was not retried."));
            }
            std::thread::sleep(Duration::from_millis(5));
        };
        // finish_input already consumed successful/error replies at arrival,
        // before a following Closed event. Cancel covers every other exit path.
        if let Some(sink) = sink {
            sink.cancel_input(identity, id);
        }
        if let Some(terminal) = self.terminals.get_mut(&tab) {
            if terminal.waiting.is_some_and(|waiting| waiting.0 == id.0) {
                terminal.waiting = None;
            }
        }
        result
    }
}
