//! Native rendering over the existing verified local client lifetime. This is
//! not a remote/tailnet method and has no host Stop or system mutation command.
use crate::{native::*, Attachment, NativeLaunchCatalog};
use serde::{Deserialize, Serialize};
use shellfox_core::{
    window_state::WindowState, ChangedEvent, SessionId, Settings, Snapshot, TabId,
};
use shellfox_host::{CreationRequestReceipt, HostRequest, SettingsRequest};
use shellfox_protocol::{validation::ensure, LocalRequest, Validate, ValidationError};
use shellfox_runtime::{InputReceipt, Response};
mod frame;
pub use frame::DesktopFrame;
mod provider;
pub use provider::DesktopProviderRequest;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(
    tag = "action",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum DesktopRequest {
    Status,
    /// Sessions as 0.9's v1 records, for the CLI.
    SessionRecords,
    TerminalProfiles,
    TabStats,
    ReadWindowState,
    ReadTabActivity,
    SetVisibleSessions(shellfox_protocol::SetVisibleSessions),
    Provider(DesktopProviderRequest),
    SaveWindowState(WindowState),
    Metadata(HostRequest),
    Settings(SettingsRequest),
    CreationRequestReceipt {
        request_id: SessionId,
    },
    SelectTab {
        tab: TabId,
    },
    SetSettled {
        session: SessionId,
        settled: bool,
    },
    ClearSessionError {
        session: SessionId,
    },
    ChangedSince {
        revision: u64,
    },
    CloseTab {
        tab: TabId,
    },
    CloseTerminal(shellfox_core::TerminalIdentity),
    DeleteSession {
        session: SessionId,
    },
    Attach {
        tab: TabId,
        takeover: bool,
    },
    Detach {
        tab: TabId,
        attachment: Attachment,
    },
    Terminal {
        tab: TabId,
        attachment: Attachment,
        command: NativeCommand,
        input: Option<NativeInputContext>,
    },
    Presentation {
        tab: TabId,
        attachment: NativeViewAttachment,
        action: NativePresentation,
    },
    Observe {
        tab: TabId,
    },
    RecoverObserver {
        tab: TabId,
        attachment: NativeObserverAttachment,
    },
    AcceptFrame {
        tab: TabId,
        attachment: Attachment,
        frame_id: u64,
    },
    AcceptObserverFrame {
        tab: TabId,
        attachment: NativeObserverAttachment,
        frame_id: u64,
    },
    Unobserve {
        tab: TabId,
        attachment: NativeObserverAttachment,
    },
    DetachAll,
}

// Keep this allowlist symmetric without making every in-process action a wire command.
macro_rules! native_actions {
    ($($name:ident $(: $data:ident)? $({ $($field:ident),* })?),* $(,)?) => {
        impl TryFrom<DesktopRequest> for NativeAction {
            type Error = anyhow::Error;
            fn try_from(value: DesktopRequest) -> anyhow::Result<Self> {
                Ok(match value {
                    $(DesktopRequest::$name $(($data))? $({ $($field),* })? =>
                        Self::$name $(($data))? $({ $($field),* })?),*,
                    DesktopRequest::Provider(_) => anyhow::bail!("Desktop provider requires a registered renderer"),
                })
            }
        }
        impl TryFrom<NativeAction> for DesktopRequest {
            type Error = anyhow::Error;
            fn try_from(value: NativeAction) -> anyhow::Result<Self> {
                Ok(match value {
                    $(NativeAction::$name $(($data))? $({ $($field),* })? =>
                        Self::$name $(($data))? $({ $($field),* })?),*,
                    _ => anyhow::bail!("This action is not available to an attached desktop.")
                })
            }
        }
    };
}
native_actions!(Status, SessionRecords, TerminalProfiles, TabStats, ReadWindowState, ReadTabActivity, SetVisibleSessions: visibility, SaveWindowState: state,
    Metadata: request, Settings: request, CreationRequestReceipt { request_id }, SelectTab { tab },
    SetSettled { session, settled }, ClearSessionError { session }, ChangedSince { revision },
    CloseTab { tab }, CloseTerminal: identity, DeleteSession { session }, Attach { tab, takeover }, Detach { tab, attachment },
    Terminal { tab, attachment, command, input }, Presentation { tab, attachment, action }, Observe { tab },
    RecoverObserver { tab, attachment }, AcceptFrame { tab, attachment, frame_id },
    AcceptObserverFrame { tab, attachment, frame_id }, Unobserve { tab, attachment }, DetachAll
);

impl Validate for DesktopRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        let action = match self {
            Self::Provider(provider) => provider.clone().into_native(),
            request => NativeAction::try_from(request.clone()),
        };
        action
            .and_then(|action| action.validate())
            .map_err(|_| ValidationError)
    }
}
impl LocalRequest for DesktopRequest {
    type Response = DesktopReply;
    const METHOD: &str = shellfox_protocol::DESKTOP_VIEW_NAMESPACE;
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum DesktopReply {
    Status(NativeStatus),
    SessionRecords(Vec<shellfox_protocol::SessionDto>),
    WindowState(Option<WindowState>),
    TabActivity(Vec<crate::NativeTabActivity>),
    TerminalProfiles(Box<NativeLaunchCatalog>),
    TabStats(Vec<shellfox_protocol::TabStat>),
    Snapshot(Snapshot),
    Settings(Settings),
    System(crate::system::SystemNativeResponse),
    Ssh(crate::ssh::Response),
    CreationRequestReceipt {
        request_id: SessionId,
        receipt: CreationRequestReceipt,
    },
    Changed(Option<ChangedEvent>),
    Attached(Attachment),
    Observed(NativeObserverAttachment),
    Frame(Option<DesktopFrame>),
    Input(InputReceipt),
    Text(Option<String>),
    MouseTracking(bool),
    TerminalApplied,
    Accepted {
        sequence: u64,
    },
    Applied,
}

impl TryFrom<NativeResponse> for DesktopReply {
    type Error = anyhow::Error;
    fn try_from(value: NativeResponse) -> anyhow::Result<Self> {
        Ok(match value {
            NativeResponse::Status(value) => Self::Status(value),
            NativeResponse::SessionRecords(value) => Self::SessionRecords(value),
            NativeResponse::WindowState(value) => Self::WindowState(value),
            NativeResponse::TabActivity(value) => Self::TabActivity(value),
            NativeResponse::TerminalProfiles(value) => Self::TerminalProfiles(value),
            NativeResponse::TabStats(value) => Self::TabStats(value),
            NativeResponse::Snapshot(value) => Self::Snapshot(value),
            NativeResponse::Settings(value) => Self::Settings(value),
            NativeResponse::System(value) => Self::System(value),
            NativeResponse::Ssh(value) => Self::Ssh(value),
            NativeResponse::CreationRequestReceipt {
                request_id,
                receipt,
            } => Self::CreationRequestReceipt {
                request_id,
                receipt,
            },
            NativeResponse::Changed(value) => Self::Changed(value),
            NativeResponse::Attached(value) => Self::Attached(value),
            NativeResponse::Observed(value) => Self::Observed(value),
            NativeResponse::Frame(value) => Self::Frame(value.map(DesktopFrame::from)),
            NativeResponse::Terminal(Response::Input(value)) => Self::Input(value),
            NativeResponse::Terminal(Response::Text(value)) => Self::Text(value),
            NativeResponse::Terminal(Response::MouseTracking(value)) => Self::MouseTracking(value),
            NativeResponse::Terminal(Response::Applied) => Self::TerminalApplied,
            NativeResponse::Accepted { sequence } => Self::Accepted { sequence },
            NativeResponse::Applied => Self::Applied,
            _ => anyhow::bail!("Unsupported attached desktop response."),
        })
    }
}
impl From<DesktopReply> for NativeResponse {
    fn from(value: DesktopReply) -> Self {
        match value {
            DesktopReply::Status(value) => Self::Status(value),
            DesktopReply::SessionRecords(value) => Self::SessionRecords(value),
            DesktopReply::WindowState(value) => Self::WindowState(value),
            DesktopReply::TabActivity(value) => Self::TabActivity(value),
            DesktopReply::TerminalProfiles(value) => Self::TerminalProfiles(value),
            DesktopReply::TabStats(value) => Self::TabStats(value),
            DesktopReply::Snapshot(value) => Self::Snapshot(value),
            DesktopReply::Settings(value) => Self::Settings(value),
            DesktopReply::System(value) => Self::System(value),
            DesktopReply::Ssh(value) => Self::Ssh(value),
            DesktopReply::CreationRequestReceipt {
                request_id,
                receipt,
            } => Self::CreationRequestReceipt {
                request_id,
                receipt,
            },
            DesktopReply::Changed(value) => Self::Changed(value),
            DesktopReply::Attached(value) => Self::Attached(value),
            DesktopReply::Observed(value) => Self::Observed(value),
            DesktopReply::Frame(value) => Self::Frame(value.map(NativeFrame::from)),
            DesktopReply::Input(value) => Self::Terminal(Response::Input(value)),
            DesktopReply::Text(value) => Self::Terminal(Response::Text(value)),
            DesktopReply::MouseTracking(value) => Self::Terminal(Response::MouseTracking(value)),
            DesktopReply::TerminalApplied => Self::Terminal(Response::Applied),
            DesktopReply::Accepted { sequence } => Self::Accepted { sequence },
            DesktopReply::Applied => Self::Applied,
        }
    }
}
impl Validate for DesktopReply {
    fn validate(&self) -> Result<(), ValidationError> {
        match self {
            Self::WindowState(Some(state)) => ensure(state.valid()),
            Self::TabActivity(states) => ensure(
                states.len() <= 10_000
                    && states.iter().all(|state| {
                        state.error.as_ref().is_none_or(|text| text.len() <= 2048)
                            && state
                                .process_title
                                .as_deref()
                                .is_none_or(|title| shellfox_core::validation::title(title).is_ok())
                    }),
            ),
            Self::Snapshot(snapshot) => crate::local_api::validate_snapshot(snapshot),
            Self::Settings(settings) => settings.validate().map_err(|_| ValidationError),
            Self::TerminalProfiles(catalog) => {
                catalog.probe.validate()?;
                catalog.profiles.validate()
            }
            Self::TabStats(stats) => {
                ensure(stats.len() <= 64)?;
                stats
                    .iter()
                    .try_for_each(shellfox_protocol::Validate::validate)
            }
            Self::Attached(a) => ensure(a.controller > 0 && a.epoch > 0),
            Self::Observed(a) => ensure(a.viewer > 0 && a.epoch > 0),
            Self::Frame(Some(frame)) => frame.validate(),
            _ => Ok(()),
        }
    }
}

impl crate::Service {
    pub(crate) fn execute_attached_desktop(
        &mut self,
        action: NativeAction,
        controller: u64,
    ) -> anyhow::Result<NativeResponse> {
        use crate::system::{NativeSystemFiles, SystemFiles};
        let window_file = || -> anyhow::Result<std::path::PathBuf> {
            Ok(self.directory.join("desktop-window.json"))
        };
        match action {
            NativeAction::ReadWindowState => {
                self.assert_owned()?;
                let file = NativeSystemFiles
                    .read(&window_file()?)
                    .map_err(|error| anyhow::anyhow!(error.message))?;
                Ok(NativeResponse::WindowState(
                    file.as_ref()
                        .filter(|file| file.bytes.len() < 4096)
                        .and_then(|file| WindowState::parse(&file.bytes)),
                ))
            }
            NativeAction::SaveWindowState(ref state) => {
                self.assert_accepting()?;
                action.validate()?;
                let path = window_file()?;
                let file = NativeSystemFiles
                    .read(&path)
                    .map_err(|error| anyhow::anyhow!(error.message))?;
                NativeSystemFiles
                    .replace(
                        &path,
                        file.as_ref(),
                        Some(&serde_json::to_vec(state)?),
                        false,
                    )
                    .map_err(|error| anyhow::anyhow!(error.message))?;
                Ok(NativeResponse::Applied)
            }
            other => self.execute_native(other, controller),
        }
    }
}

#[cfg(test)]
mod tests;
