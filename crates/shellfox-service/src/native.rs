//! The owner's typed desktop actions and replies. Desktop windows reach them
//! through the local API (local/desktop.rs). Not a mobile or network wire
//! protocol, and no GPUI.
mod cache;
pub(crate) mod catalog;
mod commands;
mod dispatch;
pub(crate) mod endpoint_report;
mod observers;
mod presentation;
pub use presentation::{NativePresentation, NativeViewAttachment};
mod watches;
pub use watches::{FrameWakes, NativeWatches};
mod validation;
pub use validation::NativeAdmissionError;
#[cfg(test)]
mod integration_tests;
#[cfg(test)]
mod startup_remote_tests;
#[cfg(test)]
mod startup_tests;
#[cfg(test)]
mod tests;

use crate::Attachment;
pub(crate) use cache::NativeState;
use shellfox_core::{ChangedEvent, Generation, SessionId, Snapshot, TabId};
use shellfox_ghostty_vt::{
    key::KeyEvent, MouseEvent, MouseGeometry, Selection, TerminalColors, ViewportPoint,
};
use shellfox_host::HostRequest;
use shellfox_runtime::{Phase, Response, ScreenFrame};
use std::sync::Arc;

/// Captured by the frontend at bounded intent admission, never at dispatch/receipt.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeInputContext {
    pub captured_settlement: shellfox_host::CapturedSettlement,
    pub user_initiated: bool,
    pub hint: bool,
}

#[derive(Debug, Clone)]
pub enum NativeAction {
    Status,
    ReadWindowState,
    ReadTabActivity,
    SetVisibleSessions(shellfox_protocol::SetVisibleSessions),
    SaveWindowState(shellfox_core::window_state::WindowState),
    TerminalProfiles,
    /// Owned-process stats of the live tabs, captured on demand off the owner thread.
    TabStats,
    Power(crate::power::PowerRequest),
    Metadata(HostRequest),
    /// Original Host request record; never derives identity from selection.
    CreationRequestReceipt {
        request_id: SessionId,
    },
    Settings(shellfox_host::SettingsRequest),
    Ssh(crate::ssh::Request),
    System(crate::system::SystemNativeRequest),
    Remote(crate::remote::native::RemoteRequest),
    SelectTab {
        tab: TabId,
    },
    SessionCwd {
        session: SessionId,
        cwd: String,
    },
    TabCwd {
        tab: TabId,
        cwd: String,
    },
    SetSettled {
        session: SessionId,
        settled: bool,
    },
    ChangedSince {
        revision: u64,
    },
    ClearSessionError {
        session: SessionId,
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
    Frame {
        tab: TabId,
        attachment: Attachment,
    },
    Presentation {
        tab: TabId,
        attachment: NativeViewAttachment,
        action: NativePresentation,
    },
    AcceptFrame {
        tab: TabId,
        attachment: Attachment,
        frame_id: u64,
    },
    Observe {
        tab: TabId,
    },
    ObserverFrame {
        tab: TabId,
        attachment: NativeObserverAttachment,
    },
    RecoverObserver {
        tab: TabId,
        attachment: NativeObserverAttachment,
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

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub enum NativeCommand {
    Key(KeyEvent),
    /// Keys typed in a row, sent in one request.
    Keys(Vec<KeyEvent>),
    Paste(String),
    Mouse {
        event: MouseEvent,
        geometry: MouseGeometry,
    },
    Resize {
        cols: u16,
        rows: u16,
    },
    Colors(Box<TerminalColors>),
    Scroll(isize),
    Selection(Option<Selection>),
    Copy,
    Hyperlink(ViewportPoint),
    MouseTracking,
    Recover,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeObserverAttachment {
    pub viewer: u64,
    pub epoch: u64,
    pub generation: Generation,
}

#[derive(Debug, Clone)]
pub struct NativeFrame {
    /// Changes on every captured frame, even when no output sequence changed.
    pub frame_id: u64,
    pub frame: Arc<ScreenFrame>,
    pub presentation: Arc<shellfox_runtime::ViewFrame>,
    pub highest_delivered: u64,
    pub recovery_barrier: Option<u64>,
    pub exited: Option<u64>,
    pub phase: Phase,
    pub error: Option<String>,
    pub replay_lost: bool,
    /// Host-validated path selector from this generation's live PWD report.
    pub working_directory: Option<Arc<str>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeStatus {
    /// Actual owners retained by the common service, never a UI view count.
    pub active_terminals: usize,
}

#[derive(Debug)]
pub enum NativeResponse {
    Status(NativeStatus),
    WindowState(Option<shellfox_core::window_state::WindowState>),
    TabActivity(Vec<crate::NativeTabActivity>),
    Power(crate::power::PowerReport),
    TerminalProfiles(Box<crate::NativeLaunchCatalog>),
    TabStats(Vec<shellfox_protocol::TabStat>),
    Snapshot(Snapshot),
    CreationRequestReceipt {
        request_id: SessionId,
        receipt: shellfox_host::CreationRequestReceipt,
    },
    Settings(shellfox_core::Settings),
    Ssh(crate::ssh::Response),
    System(crate::system::SystemNativeResponse),
    Remote(crate::remote::native::RemoteResponse),
    RemoteConnect {
        state: crate::remote::native::RemoteResponse,
        result: crate::remote::native::ConnectResult,
    },
    Changed(Option<ChangedEvent>),
    Attached(Attachment),
    Observed(NativeObserverAttachment),
    Frame(Option<NativeFrame>),
    Terminal(Response),
    Accepted {
        sequence: u64,
    },
    Applied,
}
