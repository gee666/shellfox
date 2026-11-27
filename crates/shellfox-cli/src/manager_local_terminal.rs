//! 0.9 terminal methods (`api attachTerminal` and friends) for one local CLI call.
//! Each CLI process is its own view and its control ends when it exits, as in
//! 0.9. A call that needs control joins only a terminal no other view controls;
//! only takeTerminalControl takes it from the desktop.
use crate::{manager_local::Local, output::CliError};
use anyhow::Result;
use serde_json::{json, Value};
use shellfox_core::{Generation, TabId};
use shellfox_protocol::{ReadTerminal, RemoteCommand as R};
use shellfox_service::{
    local::LocalRequestFailure,
    local_api::{LocalApiRequest, LocalApiResponse, ManagerAttachment},
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

/// The host refused an attach because another client controls the terminal.
pub(crate) fn controlled_elsewhere(error: &anyhow::Error) -> bool {
    error
        .downcast_ref::<LocalRequestFailure>()
        .is_some_and(|failure| {
            failure.error.code == shellfox_protocol::ErrorCode::TerminalControlRequired
        })
}

/// 0.9's attachment JSON: the read plus this view's control.
fn attachment_value(tab: TabId, attachment: ManagerAttachment) -> Result<Value> {
    Ok(match attachment {
        ManagerAttachment::Owned {
            control_epoch,
            read,
        } => {
            let mut value = serde_json::to_value(read)?;
            value["control"] = json!("owned");
            value["controlEpoch"] = json!(control_epoch);
            value
        }
        ManagerAttachment::Blocked {
            session_id,
            generation,
            control_epoch,
        } => json!({
            "tabId": tab,
            "sessionId": session_id,
            "generation": generation,
            "control": "blocked",
            "controlEpoch": control_epoch,
        }),
    })
}

impl Local<'_> {
    /// `api readTerminal` and `terminals read`: output after the cursor, read
    /// while any other client keeps control.
    pub(crate) async fn read_terminal(&self, input: ReadTerminal) -> Result<Value> {
        match self.0.execute(LocalApiRequest::Read { input }).await? {
            LocalApiResponse::TerminalRead(read) => Ok(serde_json::to_value(read)?),
            _ => Err(unexpected()),
        }
    }

    /// The running terminal's generation, checked against the caller's. As in
    /// 0.9, a caller naming a generation that isn't running gets TARGET_LOST.
    async fn running(&self, tab: TabId, expected: Option<Generation>) -> Result<Generation> {
        let missing = || if expected.is_some() { stale() } else { not_found() };
        let generation = self.live(tab).await?.ok_or_else(missing)?;
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
            Err(error) if !controlled_elsewhere(&error) => {
                return Err(match error.downcast::<LocalRequestFailure>() {
                    Ok(failure) => CliError::app(failure.error),
                    Err(error) => error,
                })
            }
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
                // A replaced generation is answered with a replayLost snapshot.
                self.running(input.tab_id, None).await?;
                let input = ReadTerminal {
                    tab_id: input.tab_id,
                    after_sequence: input.after_sequence,
                    generation: input.generation,
                };
                // Control stays with this view until the process exits, as in 0.9.
                let tab = input.tab_id;
                match self.0.execute(LocalApiRequest::ManagerAttach { input }).await? {
                    LocalApiResponse::ManagerAttachment(attachment) => {
                        attachment_value(tab, attachment)
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
                let written = self
                    .with_control(input.tab_id, generation, false, |attachment| {
                        Some(LocalApiRequest::Write { attachment, input })
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
