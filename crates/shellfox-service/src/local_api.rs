//! The private local API used by the desktop and CLI. Not the mobile ManagerSnapshot protocol.
use crate::{Attachment, Service, TerminalOutput};
use serde::{Deserialize, Serialize};
use shellfox_core::{AddTab, CreateSession, SessionId, Settings, Snapshot, TabId};
pub use shellfox_host::SettingsRequest;
pub use crate::terminals::ManagerAttachment;
use shellfox_protocol::{validation::ensure, ReadTerminal, Validate, ValidationError};

pub const METHOD: &str = "private.local.v1";
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RecoveryCapability {
    VisibleScreen,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "camelCase", deny_unknown_fields)]
pub enum LocalApiRequest {
    Snapshot {},
    CreateSession {
        input: CreateSession,
    },
    RenameSession {
        id: SessionId,
        title: String,
    },
    /// Selects the session and, as in 0.9, opens its shell when none runs.
    ActivateSession {
        id: SessionId,
    },
    DeleteSession {
        id: SessionId,
    },
    AddTab {
        id: SessionId,
        input: AddTab,
    },
    RenameTab {
        id: TabId,
        title: String,
    },
    CloseTab {
        id: TabId,
    },
    Settings {
        request: SettingsRequest,
    },
    Attach {
        tab: TabId,
        takeover: bool,
    },
    Input {
        tab: TabId,
        attachment: Attachment,
        bytes: Vec<u8>,
    },
    /// 0.9 `api writeTerminal`, keeping its userInitiated and settledAt.
    Write {
        attachment: Attachment,
        input: shellfox_protocol::WriteTerminal,
    },
    /// Encoded by the host's Ghostty terminal with the application's live modes.
    Key {
        tab: TabId,
        attachment: Attachment,
        event: shellfox_ghostty_vt::key::KeyEvent,
    },
    Paste {
        tab: TabId,
        attachment: Attachment,
        text: String,
    },
    Resize {
        tab: TabId,
        attachment: Attachment,
        cols: u16,
        rows: u16,
    },
    Acknowledge {
        tab: TabId,
        attachment: Attachment,
        sequence: u64,
    },
    Recover {
        tab: TabId,
        attachment: Attachment,
    },
    Poll {
        tab: TabId,
        attachment: Attachment,
    },
    /// 0.9 `api attachTerminal`: joins a running terminal only if no other view
    /// controls it, and answers with its output after the cursor.
    ManagerAttach {
        input: ReadTerminal,
    },
    /// Output after the cursor, without taking or changing control.
    Read {
        input: ReadTerminal,
    },
    Detach {},
    Stop {},
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum LocalApiResponse {
    Snapshot(Snapshot),
    Settings(Settings),
    Attachment(Attachment),
    Output(Vec<TerminalOutput>),
    Recovery {
        bytes: Vec<u8>,
        last_sequence: u64,
        capability: RecoveryCapability,
    },
    RecoveryRequired {},
    TerminalExited {
        last_sequence: u64,
    },
    ManagerAttachment(ManagerAttachment),
    TerminalRead(Box<shellfox_protocol::TerminalRead>),
    Done {},
}
impl Validate for LocalApiRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        use LocalApiRequest::*;
        let core = |result: shellfox_core::Result<()>| result.map_err(|_| ValidationError);
        match self {
            CreateSession { input } => {
                core(shellfox_core::validation::cwd(&input.cwd))?;
                if let Some(title) = &input.title {
                    core(shellfox_core::validation::title(title))?;
                }
            }
            RenameSession { title, .. } | RenameTab { title, .. } => {
                core(shellfox_core::validation::title(title))?
            }
            AddTab { input, .. } => {
                if let Some(cwd) = &input.cwd {
                    core(shellfox_core::validation::cwd(cwd))?;
                }
                if let Some(title) = &input.title {
                    core(shellfox_core::validation::title(title))?;
                }
            }
            Settings { request } => {
                crate::native::NativeAction::Settings(request.clone())
                    .validate()
                    .map_err(|_| ValidationError)?;
            }
            Input {
                bytes, attachment, ..
            } => {
                ensure(bytes.len() <= 64 * 1024)?;
                validate_attachment(attachment)?;
            }
            Write { attachment, input } => {
                input.validate()?;
                validate_attachment(attachment)?;
            }
            Key {
                event, attachment, ..
            } => {
                ensure(event.text.len() <= 64 * 1024 && event.validate().is_ok())?;
                validate_attachment(attachment)?;
            }
            Paste {
                text, attachment, ..
            } => {
                ensure(text.len() <= 64 * 1024)?;
                validate_attachment(attachment)?;
            }
            Resize {
                attachment,
                cols,
                rows,
                ..
            } => {
                shellfox_runtime::GridLimits::default()
                    .validate_size(*cols, *rows)
                    .map_err(|_| ValidationError)?;
                validate_attachment(attachment)?;
            }
            ManagerAttach { input } | Read { input } => input.validate()?,
            Recover { attachment, .. }
            | Poll { attachment, .. }
            | Acknowledge { attachment, .. } => validate_attachment(attachment)?,
            _ => {}
        }
        Ok(())
    }
}
fn validate_attachment(attachment: &Attachment) -> Result<(), ValidationError> {
    ensure(attachment.controller > 0 && attachment.epoch > 0)
}
impl Validate for LocalApiResponse {
    fn validate(&self) -> Result<(), ValidationError> {
        match self {
            Self::Snapshot(snapshot) => validate_snapshot(snapshot)?,
            Self::Settings(settings) => settings.validate().map_err(|_| ValidationError)?,
            Self::Attachment(attachment) => validate_attachment(attachment)?,
            Self::Output(chunks) => {
                ensure(
                    chunks.len() <= 4096
                        && chunks.iter().map(|c| c.bytes.len()).sum::<usize>() <= 2 * 1024 * 1024,
                )?;
                ensure(
                    chunks
                        .windows(2)
                        .all(|pair| pair[0].sequence < pair[1].sequence),
                )?;
            }
            Self::Recovery { bytes, .. } => ensure(bytes.len() <= 2 * 1024 * 1024)?,
            Self::ManagerAttachment(ManagerAttachment::Owned {
                control_epoch,
                read,
            }) => {
                shellfox_protocol::validation::epoch(*control_epoch)?;
                read.validate()?;
            }
            Self::ManagerAttachment(ManagerAttachment::Blocked { control_epoch, .. }) => {
                shellfox_protocol::validation::epoch(*control_epoch)?;
            }
            Self::TerminalRead(read) => read.validate()?,
            Self::Done {} | Self::RecoveryRequired {} | Self::TerminalExited { .. } => {}
        }
        Ok(())
    }
}
pub(crate) fn validate_snapshot(snapshot: &Snapshot) -> Result<(), ValidationError> {
    ensure(snapshot.sessions.len() <= 10000)?;
    snapshot
        .preferences
        .validate()
        .map_err(|_| ValidationError)?;
    for session in &snapshot.sessions {
        session.validate().map_err(|_| ValidationError)?;
    }
    Ok(())
}

impl shellfox_protocol::LocalRequest for LocalApiRequest {
    type Response = LocalApiResponse;
    const METHOD: &'static str = METHOD;
}

impl Service {
    pub fn execute(
        &mut self,
        request: LocalApiRequest,
        controller: u64,
    ) -> anyhow::Result<LocalApiResponse> {
        use LocalApiRequest::*;
        self.assert_owned()?;
        request.validate()?;
        if self.stopping
            && !matches!(
                request,
                Snapshot {}
                    | Poll { .. }
                    | Acknowledge { .. }
                    | Recover { .. }
                    | Detach {}
                    | Stop {}
            )
        {
            self.assert_accepting()?;
        }
        let response = match request {
            Snapshot {} => LocalApiResponse::Snapshot(self.host().snapshot()?),
            CreateSession { input } => {
                let before = self.host().snapshot()?;
                let after = self.create_application_session(input)?;
                LocalApiResponse::Snapshot(self.start_new_tab(&before, after)?)
            }
            RenameSession { id, title } => {
                LocalApiResponse::Snapshot(self.host().rename_session(id, &title)?)
            }
            ActivateSession { id } => LocalApiResponse::Snapshot(self.activate_session(id)?),
            DeleteSession { .. } | CloseTab { .. } => {
                anyhow::bail!("Closes run on the owner's close route.")
            }
            AddTab { id, input } => {
                let before = self.host().snapshot()?;
                let after = self.host().add_tab(id, input)?;
                LocalApiResponse::Snapshot(self.start_new_tab(&before, after)?)
            }
            RenameTab { id, title } => {
                LocalApiResponse::Snapshot(self.host().rename_tab(id, &title)?)
            }
            Settings { request } => {
                LocalApiResponse::Settings(self.host().settings_request(request)?)
            }
            Attach { tab, takeover } => {
                LocalApiResponse::Attachment(self.attach(tab, controller, takeover)?)
            }
            Input {
                tab,
                attachment,
                bytes,
            } => {
                owner(&attachment, controller)?;
                self.input(tab, &attachment, bytes)?;
                LocalApiResponse::Done {}
            }
            Write { attachment, input } => {
                owner(&attachment, controller)?;
                self.remote_v1_input(input, &attachment)
                    .map_err(anyhow::Error::new)?;
                LocalApiResponse::Done {}
            }
            Key {
                tab,
                attachment,
                event,
            } => {
                owner(&attachment, controller)?;
                self.key(tab, &attachment, event)?;
                LocalApiResponse::Done {}
            }
            Paste {
                tab,
                attachment,
                text,
            } => {
                owner(&attachment, controller)?;
                self.paste(tab, &attachment, text)?;
                LocalApiResponse::Done {}
            }
            Resize {
                tab,
                attachment,
                cols,
                rows,
            } => {
                owner(&attachment, controller)?;
                self.resize(tab, &attachment, cols, rows)?;
                LocalApiResponse::Done {}
            }
            Acknowledge {
                tab,
                attachment,
                sequence,
            } => {
                owner(&attachment, controller)?;
                self.acknowledge(tab, &attachment, sequence)?;
                LocalApiResponse::Done {}
            }
            Recover { tab, attachment } => {
                owner(&attachment, controller)?;
                let (bytes, last_sequence, capability) = self.recover(tab, &attachment)?;
                if capability != shellfox_runtime::AnsiRecoveryCapability::VisibleScreen {
                    anyhow::bail!("Unnegotiated terminal recovery capability.");
                }
                LocalApiResponse::Recovery {
                    bytes,
                    last_sequence,
                    capability: RecoveryCapability::VisibleScreen,
                }
            }
            Poll { tab, attachment } => {
                owner(&attachment, controller)?;
                let output = self.poll(tab, &attachment)?;
                match self.poll_status(tab)? {
                    (true, _) => LocalApiResponse::RecoveryRequired {},
                    (false, Some(last_sequence)) => {
                        LocalApiResponse::TerminalExited { last_sequence }
                    }
                    _ => LocalApiResponse::Output(output),
                }
            }
            ManagerAttach { input } => {
                LocalApiResponse::ManagerAttachment(self.manager_attachment(&input, controller)?)
            }
            Read { input } => LocalApiResponse::TerminalRead(Box::new(self.read_terminal(&input)?)),
            Detach {} => {
                self.detach_controller(controller);
                LocalApiResponse::Done {}
            }
            Stop {} => {
                self.shutdown()?;
                LocalApiResponse::Done {}
            }
        };
        response.validate()?;
        Ok(response)
    }
}
fn owner(attachment: &Attachment, controller: u64) -> anyhow::Result<()> {
    ensure(attachment.controller == controller).map_err(Into::into)
}
