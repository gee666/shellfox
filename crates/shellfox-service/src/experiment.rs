//! Private local experiment contract. Not the mobile ManagerSnapshot protocol.
use crate::{Attachment, Service, TerminalOutput};
use serde::{Deserialize, Serialize};
use shellfox_core::{AddTab, CreateSession, SessionId, Settings, Snapshot, TabId};
pub use shellfox_host::SettingsRequest;
use shellfox_protocol::{validation::ensure, Validate, ValidationError};

pub const METHOD: &str = "experimental.rust.v1";
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RecoveryCapability {
    VisibleScreen,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "camelCase", deny_unknown_fields)]
pub enum ExperimentRequest {
    Snapshot {},
    CreateSession {
        input: CreateSession,
    },
    RenameSession {
        id: SessionId,
        title: String,
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
pub enum ExperimentResponse {
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
    Done {},
}
impl Validate for ExperimentRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        use ExperimentRequest::*;
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
            Resize {
                attachment,
                cols,
                rows,
                ..
            } => {
                shellfox_runtime::GridLimits::default().validate_size(*cols, *rows).map_err(|_|ValidationError)?;
                validate_attachment(attachment)?;
            }
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
impl Validate for ExperimentResponse {
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
            Self::Done {} | Self::RecoveryRequired {} | Self::TerminalExited { .. } => {}
        }
        Ok(())
    }
}
pub(crate) fn validate_snapshot(snapshot: &Snapshot) -> Result<(), ValidationError> {
    ensure(snapshot.sessions.len() <= 10000)?;
    snapshot.preferences.validate().map_err(|_| ValidationError)?;
    for session in &snapshot.sessions { session.validate().map_err(|_| ValidationError)?; }
    Ok(())
}

impl shellfox_protocol::LocalRequest for ExperimentRequest {
    type Response = ExperimentResponse;
    const METHOD: &'static str = METHOD;
}

impl Service {
    pub fn execute(
        &mut self,
        request: ExperimentRequest,
        controller: u64,
    ) -> anyhow::Result<ExperimentResponse> {
        use ExperimentRequest::*;
        self.assert_owned()?;
        request.validate()?;
        if self.stopping && !matches!(request, Snapshot {} | Poll { .. } | Acknowledge { .. } | Recover { .. } | Detach {} | Stop {}) {
            self.assert_accepting()?;
        }
        let response = match request {
            Snapshot {} => ExperimentResponse::Snapshot(self.host().snapshot()?),
            CreateSession { input } => {
                ExperimentResponse::Snapshot(self.create_application_session(input)?)
            }
            RenameSession { id, title } => {
                ExperimentResponse::Snapshot(self.host().rename_session(id, &title)?)
            }
            DeleteSession { id } => ExperimentResponse::Snapshot(self.delete_session(id)?),
            AddTab { id, input } => ExperimentResponse::Snapshot(self.host().add_tab(id, input)?),
            RenameTab { id, title } => {
                ExperimentResponse::Snapshot(self.host().rename_tab(id, &title)?)
            }
            CloseTab { id } => ExperimentResponse::Snapshot(self.close_tab(id)?),
            Settings { request } => {
                ExperimentResponse::Settings(self.host().settings_request(request)?)
            }
            Attach { tab, takeover } => {
                ExperimentResponse::Attachment(self.attach(tab, controller, takeover)?)
            }
            Input {
                tab,
                attachment,
                bytes,
            } => {
                owner(&attachment, controller)?;
                self.input(tab, &attachment, bytes)?;
                ExperimentResponse::Done {}
            }
            Resize {
                tab,
                attachment,
                cols,
                rows,
            } => {
                owner(&attachment, controller)?;
                self.resize(tab, &attachment, cols, rows)?;
                ExperimentResponse::Done {}
            }
            Acknowledge {
                tab,
                attachment,
                sequence,
            } => {
                owner(&attachment, controller)?;
                self.acknowledge(tab, &attachment, sequence)?;
                ExperimentResponse::Done {}
            }
            Recover { tab, attachment } => {
                owner(&attachment, controller)?;
                let (bytes, last_sequence, capability) = self.recover(tab, &attachment)?;
                if capability != shellfox_runtime::AnsiRecoveryCapability::VisibleScreen {
                    anyhow::bail!("Unnegotiated terminal recovery capability.");
                }
                ExperimentResponse::Recovery {
                    bytes,
                    last_sequence,
                    capability: RecoveryCapability::VisibleScreen,
                }
            }
            Poll { tab, attachment } => {
                owner(&attachment, controller)?;
                let output = self.poll(tab, &attachment)?;
                match self.poll_status(tab)? {
                    (true, _) => ExperimentResponse::RecoveryRequired {},
                    (false, Some(last_sequence)) => {
                        ExperimentResponse::TerminalExited { last_sequence }
                    }
                    _ => ExperimentResponse::Output(output),
                }
            }
            Detach {} => {
                self.detach_controller(controller);
                ExperimentResponse::Done {}
            }
            Stop {} => {
                self.shutdown()?;
                ExperimentResponse::Done {}
            }
        };
        response.validate()?;
        Ok(response)
    }
}
fn owner(attachment: &Attachment, controller: u64) -> anyhow::Result<()> {
    ensure(attachment.controller == controller).map_err(Into::into)
}
