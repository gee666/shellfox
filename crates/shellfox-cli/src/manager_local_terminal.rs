//! 0.9 terminal methods (`api attachTerminal` and friends) for one local CLI call.
//! Each CLI process is its own view and its control ends when it exits, as in
//! 0.9. A call that needs control joins only a terminal no other view controls;
//! only takeTerminalControl takes it from the desktop.
use crate::{manager_local::Local, output::CliError};
use anyhow::Result;
use serde_json::{json, Value};
use shellfox_core::{Generation, TabId};
use shellfox_protocol::RemoteCommand as R;
use shellfox_service::{
    local_api::{LocalApiRequest, LocalApiResponse},
    Attachment,
};

fn not_found() -> anyhow::Error {
    CliError::coded("NOT_FOUND", "Terminal not found.", false)
}
fn stale() -> anyhow::Error {
    CliError::coded("TARGET_LOST", "This terminal generation is stale.", false)
}
pub(crate) fn control_required() -> anyhow::Error {
    CliError::coded(
        "TERMINAL_CONTROL_REQUIRED",
        "This terminal is in use by another view. Take it to view or control it.",
        false,
    )
}
fn unexpected() -> anyhow::Error {
    anyhow::anyhow!("Unexpected host response.")
}
fn unconfirmed() -> anyhow::Error {
    CliError::coded(
        "TARGET_LOST",
        "Terminal input was not confirmed. Reattach before sending more.",
        false,
    )
}

impl Local<'_> {
    /// The running terminal's generation, checked against the caller's.
    async fn running(&self, tab: TabId, expected: Option<Generation>) -> Result<Generation> {
        let generation = self.live(tab).await?.ok_or_else(not_found)?;
        if expected.is_some_and(|expected| expected != generation) {
            return Err(stale());
        }
        Ok(generation)
    }

    /// Runs `request`, if any, with this process's control of `tab`, then gives
    /// control back. Without `take` it fails when another view controls the terminal.
    pub(crate) async fn with_control(
        &self,
        tab: TabId,
        generation: Generation,
        take: bool,
        request: impl FnOnce(Attachment) -> Option<LocalApiRequest>,
    ) -> Result<LocalApiResponse> {
        let attach = |takeover| self.0.execute(LocalApiRequest::Attach { tab, takeover });
        let attachment = match attach(take).await {
            Ok(LocalApiResponse::Attachment(attachment)) => attachment,
            _ => return Err(control_required()),
        };
        let result = if attachment.generation != generation {
            Err(stale())
        } else {
            match request(attachment) {
                Some(request) => self.0.execute(request).await,
                None => Ok(LocalApiResponse::Done {}),
            }
        };
        // The shell keeps running; only this view's control ends.
        let _ = self.0.execute(LocalApiRequest::Detach {}).await;
        result
    }

    pub(crate) async fn terminal_call(&self, command: R) -> Result<Value> {
        match command {
            R::AttachTerminal(input) => {
                self.running(input.tab_id, input.generation).await?;
                let attach = LocalApiRequest::ManagerAttach { tab: input.tab_id };
                // Control stays with this view until the process exits, as in 0.9.
                match self.0.execute(attach).await? {
                    LocalApiResponse::ManagerAttachment(attachment) => {
                        Ok(serde_json::to_value(attachment)?)
                    }
                    _ => Err(unexpected()),
                }
            }
            R::TakeTerminalControl(input) => {
                let generation = self.running(input.tab_id, Some(input.generation)).await?;
                self.with_control(input.tab_id, generation, true, |_| None)
                    .await?;
                Ok(json!({ "taken": true }))
            }
            R::WriteTerminal(input) => {
                let generation = self.running(input.tab_id, Some(input.generation)).await?;
                let tab = input.tab_id;
                let bytes = input.data.into_bytes();
                let written = self
                    .with_control(tab, generation, false, |attachment| {
                        Some(LocalApiRequest::Input {
                            tab,
                            attachment,
                            bytes,
                        })
                    })
                    .await;
                match written {
                    Ok(LocalApiResponse::Done {}) => Ok(json!({ "written": true })),
                    Err(error) if error.downcast_ref::<CliError>().is_some() => Err(error),
                    _ => Err(unconfirmed()),
                }
            }
            R::ResizeTerminal(input) => {
                let generation = self.running(input.tab_id, Some(input.generation)).await?;
                let tab = input.tab_id;
                let (cols, rows) = (input.cols, input.rows);
                let resized = self
                    .with_control(tab, generation, false, |attachment| {
                        Some(LocalApiRequest::Resize {
                            tab,
                            attachment,
                            cols,
                            rows,
                        })
                    })
                    .await?;
                match resized {
                    LocalApiResponse::Done {} => Ok(json!({ "resized": true })),
                    _ => Err(unexpected()),
                }
            }
            // A one-shot view has no output in flight to acknowledge or detach.
            R::AcknowledgeTerminal(input) => {
                self.running(input.tab_id, Some(input.generation)).await?;
                Ok(json!({ "acknowledged": true }))
            }
            R::DetachTerminal(input) => match self.live(input.tab_id).await? {
                Some(generation) if generation != input.generation => Err(stale()),
                _ => Ok(json!({ "detached": true })),
            },
            command => Err(CliError::coded(
                "UNSUPPORTED",
                format!(
                    "{} is not available from the local CLI. Use the matching shellfox command.",
                    command.method_name()
                ),
                false,
            )),
        }
    }
}
